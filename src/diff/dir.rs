use std::{
    cell::RefCell, clone, collections::HashMap, fs::FileType, path::{Path, PathBuf}
};

use anyhow::Result;
use crossbeam::channel::{Sender, bounded};
use tracing::{error, trace};

use crate::{
    DiffTuiError,
    diff::{DiffSide, DiffState, compare_file},
};

const CHANNEL_CAPACITY: usize = 100;
/// Maps every relative path seen during a directory walk to its diff node.
pub type DirDiffTree = HashMap<PathBuf, RefCell<DirDiff>>;

/// A single node in the directory diff tree.
#[derive(Debug, Clone)]
pub struct DirDiff {
    /// Relative path of this entry from the comparison root.
    path: PathBuf,
    /// Whether the entry is a file or directory.
    ent_type: std::fs::FileType,
    /// Relative paths of direct children (populated for directories).
    children: Vec<PathBuf>,
    children_non_dir: Vec<PathBuf>,
    /// Current comparison result for this entry.
    diff_state: DiffState,
}

impl DirDiff {
    pub fn path(&self) -> &Path {
        &self.path
    }
    pub fn ent_type(&self) -> &std::fs::FileType {
        &self.ent_type
    }
    pub fn children(&self) -> &[PathBuf] {
        self.children.as_slice()
    }
    pub fn diff_state(&self) -> DiffState {
        self.diff_state
    }
}

/// Walks `cwd` recursively using gitignore-aware traversal and sends each
/// entry as a `(relative_path, file_type, side)` tuple to `sender`.
///
/// Errors from individual entries are logged and skipped; channel send errors
/// are also logged (they indicate the receiver has been dropped early).
fn walk_tree(cwd: PathBuf, sender: Sender<(PathBuf, Option<FileType>, DiffSide)>, side: DiffSide) {
    let walker = ignore::WalkBuilder::new(cwd.as_path()).build();

    for entry in walker {
        match entry {
            Err(e) => {
                error!("Walk {} error {e}", cwd.as_path().display());
            }
            Ok(entry) => {
                if let Err(_) = sender.send((
                    entry
                        .path()
                        .strip_prefix(&cwd)
                        .map(|p| p.to_path_buf())
                        .unwrap_or(entry.path().to_path_buf()),
                    entry.file_type(),
                    side,
                )) {
                    error!("Send while channel disconnected");
                };
            }
        }
    }
}

/// Builds a [`DirDiffTree`] by walking both `lhs` and `rhs` in parallel.
///
/// Each path is inserted as [`DiffState::Orphan`] on first sight. If the same
/// relative path arrives from the other side, its state is promoted to
/// [`DiffState::Unknown`], indicating it exists on both sides but has not yet
/// been content-compared. Call [`cmp_tree`] afterwards to resolve all
/// `Unknown` entries.
pub fn build_diff_tree(lhs: &Path, rhs: &Path) -> Result<DirDiffTree, DiffTuiError> {
    let mut tree: DirDiffTree = HashMap::new();
    let (send, recv) = bounded(CHANNEL_CAPACITY);

    let lhs_handle = std::thread::spawn({
        let send = send.clone();
        let cwd = lhs.to_path_buf();
        move || {
            walk_tree(cwd, send, DiffSide::Left);
        }
    });
    let rhs_handle = std::thread::spawn({
        let cwd = rhs.to_path_buf();
        move || {
            walk_tree(cwd, send, DiffSide::Right);
        }
    });

    while let Ok((p, t, side)) = recv.recv() {
        trace!("{}, {t:?}, {side}", p.display());

        if let Some(parent) = p.parent() {
            match tree.get_mut(parent) {
                None => error!("Parent {} not found", parent.display()),
                Some(entry) => {
                    if t.is_some_and(|t| t.is_dir()) {
                        if !entry.borrow().children.contains(&p) {
                            entry.borrow_mut().children.push(p.clone())
                        }
                    } else {
                        if !entry.borrow().children_non_dir.contains(&p) {
                            entry.borrow_mut().children_non_dir.push(p.clone());
                        }
                    }
                }
            }
        }
        if let Some(entry) = tree.get_mut(&p) {
            entry.borrow_mut().diff_state = DiffState::Unknown;
        } else {
            tree.insert(
                p.clone(),
                RefCell::new(DirDiff {
                    path: p.clone(),
                    children_non_dir: Vec::new(),
                    children: Vec::new(),
                    diff_state: DiffState::Orphan(side),
                    ent_type: t.unwrap(),
                }),
            );
        }
    }

    rhs_handle.join().map_err(|_| DiffTuiError::ThreadPaniced)?;
    lhs_handle.join().map_err(|_| DiffTuiError::ThreadPaniced)?;

    for (_, node) in tree.iter_mut() {
        let mut node = node.borrow_mut();
        let non_dir_items = node.children_non_dir.iter().cloned().collect::<Vec<_>>();
        node.children.extend(non_dir_items);
        node.children_non_dir.clear();
    }

    Ok(tree)
}

/// Recursively resolves the [`DiffState`] of every node reachable from `p`.
///
/// - **Orphan** entries are returned immediately without further recursion.
/// - **Directories** are `Same` when all children are `Same`, otherwise
///   `Different`.
/// - **Files** are compared by size first, then by full content.
///
/// Each node's `diff_state` is updated in place inside the tree so that the
/// full result is readable from the [`DirDiffTree`] after this call returns.
///
/// # Errors
/// Returns [`DiffTuiError::NodeNotFound`] if `p` is absent from `tree`, or an
/// I/O error if a file cannot be read.
pub fn cmp_tree(
    tree: &DirDiffTree,
    p: &Path,
    lhs: &Path,
    rhs: &Path,
) -> Result<DiffState, DiffTuiError> {
    let root = tree.get(p).ok_or(DiffTuiError::NodeNotFound)?;
    let mut ds = root.borrow().diff_state;
    if let DiffState::Orphan(_) = ds {
        return Ok(ds);
    }

    if root.borrow().ent_type.is_dir() {
        ds = DiffState::Same;
        for child in root.borrow().children.iter() {
            let child_ds = cmp_tree(tree, child, lhs, rhs)?;
            if child_ds != DiffState::Same {
                ds = DiffState::Different;
            }
        }
    } else {
        let lhs_path = lhs.join(p);
        let rhs_path = rhs.join(p);
        let lhs_stat = std::fs::metadata(&lhs_path)?;
        let rhs_stat = std::fs::metadata(&rhs_path)?;

        if lhs_stat.len() != rhs_stat.len() {
            ds = DiffState::Different;
        } else {
            if compare_file(lhs_path, rhs_path)? {
                ds = DiffState::Same;
            } else {
                ds = DiffState::Different;
            }
        }
    }

    root.borrow_mut().diff_state = ds;

    Ok(ds)
}

#[cfg(test)]
mod test {
    use super::*;

    /// Runs [`build_diff_tree`] and [`cmp_tree`] for one test scenario and
    /// returns a flat map of every relative path to its final [`DiffState`].
    ///
    /// `scenario` must name a sub-directory of `test/folder_cmp/` that
    /// contains `lhs/` and `rhs/` sub-directories.
    fn run_scenario(scenario: &str) -> HashMap<PathBuf, DiffState> {
        let base = PathBuf::from(format!("test/folder_cmp/{scenario}"));
        let lhs = base.join("lhs");
        let rhs = base.join("rhs");
        let tree = build_diff_tree(&lhs, &rhs).unwrap();
        cmp_tree(&tree, Path::new(""), &lhs, &rhs).unwrap();
        trace!("tree = {tree:#?}");
        tree.iter()
            .map(|(k, v)| (k.clone(), v.borrow().diff_state))
            .collect()
    }

    // same/ — both sides have identical files (all empty).
    // Every file must be Same; every directory must be Same.
    #[test]
    fn same_files_are_same() {
        let s = run_scenario("same");
        assert_eq!(s[&PathBuf::from("d.txt")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b/dummy.txt")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b/c/dummy_c.txt")], DiffState::Same);
    }

    #[test]
    fn same_dirs_are_same() {
        let s = run_scenario("same");
        assert_eq!(s[&PathBuf::from("b/c")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("")], DiffState::Same);
    }

    // diff/ — b/c/dummy_c.txt has content "diff\n" on lhs, empty on rhs.
    // The change must bubble up through every ancestor to the root.
    #[test]
    fn diff_changed_file_is_different() {
        let s = run_scenario("diff");
        assert_eq!(s[&PathBuf::from("b/c/dummy_c.txt")], DiffState::Different);
    }

    #[test]
    fn diff_unchanged_files_are_same() {
        let s = run_scenario("diff");
        assert_eq!(s[&PathBuf::from("d.txt")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b/dummy.txt")], DiffState::Same);
    }

    #[test]
    fn diff_change_bubbles_to_root() {
        let s = run_scenario("diff");
        assert_eq!(s[&PathBuf::from("b/c")], DiffState::Different);
        assert_eq!(s[&PathBuf::from("b")], DiffState::Different);
        assert_eq!(s[&PathBuf::from("")], DiffState::Different);
    }

    // added/ — lhs has a/e.txt (not on rhs); rhs has g/h.txt (not on lhs).
    // Shared files b/** and d.txt are identical.
    #[test]
    fn added_lhs_only_entries_are_orphan_left() {
        let s = run_scenario("added");
        assert_eq!(s[&PathBuf::from("a")], DiffState::Different);
        assert_eq!(
            s[&PathBuf::from("a/e.txt")],
            DiffState::Orphan(DiffSide::Left)
        );
    }

    #[test]
    fn added_rhs_only_entries_are_orphan_right() {
        let s = run_scenario("added");
        assert_eq!(s[&PathBuf::from("g")], DiffState::Orphan(DiffSide::Right));
        assert_eq!(
            s[&PathBuf::from("g/h.txt")],
            DiffState::Orphan(DiffSide::Right)
        );
    }

    #[test]
    fn added_shared_entries_are_same() {
        let s = run_scenario("added");
        assert_eq!(s[&PathBuf::from("d.txt")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b/dummy.txt")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b/c/dummy_c.txt")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b/c")], DiffState::Same);
        assert_eq!(s[&PathBuf::from("b")], DiffState::Same);
    }

    #[test]
    fn added_root_is_different_due_to_orphans() {
        let s = run_scenario("added");
        assert_eq!(s[&PathBuf::from("")], DiffState::Different);
    }
}
