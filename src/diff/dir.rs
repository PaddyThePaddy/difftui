use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Result;
use crossbeam::channel::{Sender, bounded};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator as _};
use tracing::{error, trace};

use crate::{
    DiffTuiError,
    diff::{DiffSide, DiffState, compare_file},
};

const CHANNEL_CAPACITY: usize = 100;

#[derive(Debug, Clone)]
pub struct DirDiffTree {
    lhs: PathBuf,
    rhs: PathBuf,
    fs_tree: HashMap<PathBuf, TreeNode>,
    diff_map: Arc<Mutex<HashMap<PathBuf, DiffState>>>,
}

impl DirDiffTree {
    pub fn fs_tree(&self) -> &HashMap<PathBuf, TreeNode> {
        &self.fs_tree
    }
    pub fn diff_map(&self) -> Arc<Mutex<HashMap<PathBuf, DiffState>>> {
        self.diff_map.clone()
    }

    pub fn new_empty() -> Self {
        Self{
            lhs: PathBuf::new(),
            rhs: PathBuf::new(),
            fs_tree: HashMap::new(),
            diff_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn new(lhs: impl Into<PathBuf>, rhs: impl Into<PathBuf>) -> Result<Self, DiffTuiError> {
        let lhs = lhs.into();
        let rhs = rhs.into();
        let (tx, rx) = bounded(CHANNEL_CAPACITY);
        let mut tree: HashMap<PathBuf, TreeNode> = HashMap::new();
        let mut diff_map: HashMap<PathBuf, DiffState> = HashMap::new();

        trace!("Dispatching tree walkers");
        let lhs_handle = {
            let tx = tx.clone();
            let cwd = lhs.clone();
            std::thread::spawn(move || {
                walk_tree(cwd, tx, DiffSide::Left);
            })
        };

        let rhs_handle = {
            let cwd = rhs.clone();
            std::thread::spawn(move || {
                walk_tree(cwd, tx, DiffSide::Right);
            })
        };
        trace!("Tree walkers started");

        trace!("Recving tree nodes");
        while let Ok((p, meta, side)) = rx.recv() {
            trace!("Got {}", p.display());
            if let Some(state) = diff_map.get_mut(&p) {
                if tree.get(&p).is_some_and(|n|n.metadata.file_type() != meta.file_type()) {
                    *state = DiffState::Different;
                }
                *state = DiffState::Unknown;
            } else {
                diff_map.insert(p.clone(), DiffState::Orphan(side));
            }

            if let Some(parent) = p.parent() {
                if let Some(parent_node) = tree.get_mut(parent) {
                    if meta.is_dir() && !parent_node.children.contains(&p){
                        parent_node.children.push(p.clone());
                    } else if !meta.is_dir() && !parent_node.children_non_dir.contains(&p){
                        parent_node.children_non_dir.push(p.clone());
                    }
                } else {
                    let mut children = vec![];
                    let mut children_non_dir = vec![];

                    if meta.is_dir() {
                        children.push(p.clone());
                    } else {
                        children_non_dir.push(p.clone());
                    }

                    tree.insert(
                        parent.to_path_buf(),
                        TreeNode {
                            metadata: meta.clone(),
                            children,
                            children_non_dir,
                        },
                    );
                }
            }

            if !tree.contains_key(&p) {
                tree.insert(
                    p,
                    TreeNode {
                        metadata: meta,
                        children: vec![],
                        children_non_dir: vec![],
                    },
                );
            }
        }
        trace!("All tree nodes enumerated");

        if let Err(e) = lhs_handle.join() {
            error!("Lhs walker panic: {e:?}");
        }
        if let Err(e) = rhs_handle.join() {
            error!("Rhs walker panic: {e:?}");
        }
        trace!("Walkers joined");

        for node in tree.values_mut() {
            node.children.extend(node.children_non_dir.iter().cloned());
            node.children_non_dir.clear();
        }
        trace!("DiffTree built");

        Ok(Self {
            lhs,
            rhs,
            fs_tree: tree,
            diff_map: Arc::new(Mutex::new(diff_map)),
        })
    }

    fn cmp_file(&self, p: &Path) -> Result<DiffState, DiffTuiError> {
        let ds = compare_file(self.lhs.join(&p), self.rhs.join(&p))?;
        self.diff_map
            .lock()
            .expect("Lock diff_map failed")
            .insert(p.to_path_buf(), ds);
        Ok(ds)
    }

    fn cmp_symlink(&self, p: &Path) -> Result<DiffState, DiffTuiError> {
        let lhs_path = self.lhs.join(p);
        let rhs_path = self.rhs.join(p);
        let ds: DiffState;

        #[cfg(not(target_os = "windows"))]
        {
            let lhs_link = std::fs::symlink_metadata(&lhs_path)?;
            let rhs_link = std::fs::symlink_metadata(&rhs_path)?;
            use std::os::unix::fs::MetadataExt;

            ds = if lhs_link.dev() == rhs_link.dev() && lhs_link.ino() == rhs_link.ino() {
                DiffState::Same
            } else {
                DiffState::Different
            };
            self.diff_map
                .lock()
                .expect("Lock diff_map failed")
                .insert(p.to_path_buf(), ds);
        }
        #[cfg(target_os = "windows")]
        {
            error!(
                "Can't compare symlink in windows: {} <=> {}",
                lhs_path.display(),
                rhs_path.display()
            );
        }
        Ok(ds)
    }

    fn cmp_tree(&self, p: &Path, node: &TreeNode) -> Result<DiffState, DiffTuiError> {
        let child_ds = node
            .children
            .par_iter()
            .map(|p| self.cmp_node(p))
            .collect::<Result<Vec<DiffState>, _>>()?;
        let ds = if child_ds.iter().all(|ds| *ds == DiffState::Same) {
            DiffState::Same
        } else {
            DiffState::Different
        };
        self.diff_map
            .lock()
            .expect("Lock diff_map failed")
            .insert(p.to_path_buf(), ds);
        Ok(ds)
    }

    pub fn cmp_node(&self, root: &Path) -> Result<DiffState, DiffTuiError> {
        let node = self.fs_tree().get(root).ok_or(DiffTuiError::NodeNotFound)?;
        let ds: DiffState;

        if !self.lhs.join(root).try_exists()? {
            return Ok(DiffState::Orphan(DiffSide::Right));
        } else if !self.rhs.join(root).try_exists()? {
            return Ok(DiffState::Orphan(DiffSide::Left));
        }

        if node.metadata.is_file() {
            ds = self.cmp_file(root)?;
        } else if node.metadata.is_dir() {
            ds = self.cmp_tree(root, node)?;
        } else if node.metadata.is_symlink() {
            ds = self.cmp_symlink(root)?;
        } else {
            let lhs_path = self.lhs.join(root);
            let rhs_path = self.rhs.join(root);
            unreachable!(
                "Unhandled type: {} <=> {}",
                lhs_path.display(),
                rhs_path.display()
            )
        }

        return Ok(ds);
    }

    pub fn get_diff_state(&self, p: &Path) -> DiffState {
        self.diff_map
            .lock()
            .expect("Locking diff_map failed")
            .get(p)
            .map(|ds| *ds)
            .unwrap_or(DiffState::Unknown)
    }

    pub fn get_fs_node(&self, p: &Path) -> Option<&TreeNode> {
        self.fs_tree().get(p)
    }
}

/// A single node in the directory diff tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    metadata: std::fs::Metadata,
    /// Relative paths of direct children (populated for directories).
    children: Vec<PathBuf>,
    children_non_dir: Vec<PathBuf>,
}

impl TreeNode {
    pub fn children(&self) -> &[PathBuf] {
        self.children.as_slice()
    }
    pub fn metadata(&self) -> &std::fs::Metadata {
        &self.metadata
    }
}

/// Walks `cwd` recursively using gitignore-aware traversal and sends each
/// entry as a `(relative_path, file_type, side)` tuple to `sender`.
///
/// Errors from individual entries are logged and skipped; channel send errors
/// are also logged (they indicate the receiver has been dropped early).
fn walk_tree(cwd: PathBuf, sender: Sender<(PathBuf, std::fs::Metadata, DiffSide)>, side: DiffSide) {
    trace!("Tree walker started");
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
                    // TODO: error handling
                    entry.metadata().unwrap(),
                    side,
                )) {
                    error!("Send while channel disconnected");
                };
            }
        }
    }
    trace!("Tree walker completed");
}

/// Builds a [`DirDiffTree`] by walking both `lhs` and `rhs` in parallel.
///
/// Each path is inserted as [`DiffState::Orphan`] on first sight. If the same
/// relative path arrives from the other side, its state is promoted to
/// [`DiffState::Unknown`], indicating it exists on both sides but has not yet
/// been content-compared. Call [`cmp_tree`] afterwards to resolve all
/// `Unknown` entries.
pub fn build_diff_tree(lhs: &Path, rhs: &Path) -> Result<DirDiffTree, DiffTuiError> {
    unimplemented!()
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
    unimplemented!()
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
        // let base = PathBuf::from(format!("test/folder_cmp/{scenario}"));
        // let lhs = base.join("lhs");
        // let rhs = base.join("rhs");
        // let tree = build_diff_tree(&lhs, &rhs).unwrap();
        // cmp_tree(&tree, Path::new(""), &lhs, &rhs).unwrap();
        // trace!("tree = {tree:#?}");
        // tree.iter()
        //     .map(|(k, v)| (k.clone(), v.borrow().diff_state))
        //     .collect()
        unimplemented!()
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
