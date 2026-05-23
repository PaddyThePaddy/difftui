use std::{
    cmp::min,
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style, Styled, Stylize},
    symbols::merge::MergeStrategy,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};

use crate::{
    diff::{DiffSide, DiffState, dir::DirDiffTree},
    ui::{Action, EventHandler, TuiTerminal},
};

#[derive(Debug, Clone)]
pub struct FolderViewState {
    side: DiffSide,
    tree: Arc<DirDiffTree>,
    selection: ListState,
    expanded_pathes: HashSet<PathBuf>,
    items_full_name: Vec<PathBuf>,
    horizontal_scroll: usize,
}

static NORMAL_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default());
static DIFF_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default().fg(Color::Red));
static ORPHAN_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default().fg(Color::Yellow));
static SAME_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default().fg(Color::Green));

impl FolderViewState {
    pub fn new(side: DiffSide, tree: Arc<DirDiffTree>, selection: ListState) -> Self {
        Self {
            side,
            tree,
            selection,
            expanded_pathes: HashSet::new(),
            items_full_name: Vec::new(),
            horizontal_scroll: 0,
        }
    }

    pub fn expand_path(&mut self, p: impl Into<PathBuf>) {
        self.expanded_pathes.insert(p.into());
    }

    pub fn collapse_path(&mut self, p: impl AsRef<Path>) {
        self.expanded_pathes.remove(p.as_ref());
    }

    pub fn toggle_path(&mut self, p: impl Into<PathBuf>) {
        let p = p.into();
        if self.expanded_pathes.contains(&p) {
            self.expanded_pathes.remove(&p);
        } else {
            self.expanded_pathes.insert(p);
        }
    }

    pub fn expand_idx(&mut self, idx: usize) {
        if let Some(p) = self.items_full_name.get(idx) {
            let p = p.clone();
            self.expand_path(p);
        }
    }

    pub fn collapse_idx(&mut self, idx: usize) {
        if let Some(p) = self.items_full_name.get(idx) {
            let p = p.clone();
            self.collapse_path(p);
        }
    }

    pub fn toggle_idx(&mut self, idx: usize) {
        if let Some(p) = self.items_full_name.get(idx) {
            let p = p.clone();
            self.toggle_path(p);
        }
    }

    pub fn expand_selected(&mut self) {
        if let Some(selected) = self.selection.selected() {
            self.expand_idx(selected);
        }
    }

    pub fn collapse_selected(&mut self) {
        if let Some(selected) = self.selection.selected() {
            self.collapse_idx(selected);
        }
    }

    pub fn toggle_selected(&mut self) {
        if let Some(selected) = self.selection.selected() {
            self.toggle_idx(selected);
        }
    }

    pub fn selected(&self) -> ListState {
        self.selection
    }

    pub fn selected_mut(&mut self) -> &mut ListState {
        &mut self.selection
    }

    pub fn selected_path(&self) -> Option<&PathBuf> {
        if let Some(idx) = self.selected().selected() {
            self.items_full_name.get(idx)
        } else {
            None
        }
    }

    pub fn horizontal_scroll(&self) -> usize {
        self.horizontal_scroll
    }

    pub fn set_horizontal_scroll(&mut self, scroll: usize) {
        self.horizontal_scroll = scroll;
    }
}

impl EventHandler for FolderViewState {
    fn handler(&mut self, event: &Action, _: &mut TuiTerminal) -> Result<Option<Action>, crate::DiffTuiError> {
        match event {
            Action::NavUp => self.selection.scroll_up_by(1),
            Action::NavDown => self.selection.scroll_down_by(1),
            Action::ToggleSelected => self.toggle_selected(),
            _ => {}
        }
        Ok(None)
    }
}

#[derive(Debug)]
pub struct FolderView<'a> {
    title: String,
    expanded_pathes: &'a HashSet<PathBuf>,
}

impl<'a> StatefulWidget for FolderView<'a> {
    type State = FolderViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        state.items_full_name.clear();
        let list_border = Block::new()
            .title(self.title.as_str())
            .borders(Borders::all())
            .merge_borders(MergeStrategy::Exact);

        let tree = state.tree.clone();
        if let Some(root) = tree.fs_tree().get(Path::new("")) {
            let mut list_items = Vec::new();
            for child in root.children() {
                self.generate_list_item(
                    &tree,
                    &mut state.items_full_name,
                    child,
                    &mut list_items,
                    0,
                    state.side,
                    state.horizontal_scroll,
                );
            }

            StatefulWidget::render(
                List::new(list_items)
                    .highlight_style(Style::new().bg(Color::Blue))
                    .block(list_border),
                area,
                buf,
                &mut state.selection,
            );
        } else {
            Paragraph::new("Loading...")
                .block(list_border)
                .render(area, buf);
        }
    }
}

impl<'a> FolderView<'a> {
    const INDENTION: &'static str = "  ";

    pub fn new(title: String, expanded_pathes: &'a HashSet<PathBuf>) -> FolderView<'a> {
        Self {
            title,
            expanded_pathes,
        }
    }

    fn generate_list_item(
        &self,
        tree: &DirDiffTree,
        items_full_name: &mut Vec<PathBuf>,
        root_path: &Path,
        list: &mut Vec<ListItem>,
        level: usize,
        side: DiffSide,
        horizontal_scroll: usize,
    ) {
        let root = match tree.fs_tree().get(root_path) {
            Some(n) => n,
            None => return,
        };
        let ds = tree.get_diff_state(root_path);

        let mut item_str = String::new();
        for _ in 0..level {
            item_str.push_str(Self::INDENTION);
        }

        if ds.is_orphan(side.oppsite()) {
            list.push("".into());
        } else {
            item_str.push_str(
                root_path
                    .file_name()
                    .unwrap_or(OsStr::new(""))
                    .to_string_lossy()
                    .as_ref(),
            );
            if root.metadata().is_dir() {
                item_str.push('/');
            }
            let list_item =
                ListItem::from(item_str[min(horizontal_scroll, item_str.len())..].to_string())
                    .style(match ds {
                        DiffState::Unknown => NORMAL_LIST_STYLE.clone(),
                        DiffState::Orphan(_) => ORPHAN_LIST_STYLE.clone(),
                        DiffState::Different => DIFF_LIST_STYLE.clone(),
                        DiffState::Same => SAME_LIST_STYLE.clone(),
                    });

            list.push(list_item);
        }
        items_full_name.push(root_path.to_path_buf());

        if self.expanded_pathes.contains(root_path) {
            for entry in root.children().iter() {
                self.generate_list_item(
                    tree,
                    items_full_name,
                    entry,
                    list,
                    level + 1,
                    side,
                    horizontal_scroll,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, TerminalOptions, Viewport, buffer::Buffer, layout::Rect, widgets::StatefulWidget};

    fn make_terminal() -> crate::ui::TuiTerminal {
        Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(std::io::stdout()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)) },
        )
        .unwrap()
    }

    fn empty_state(side: DiffSide) -> FolderViewState {
        FolderViewState::new(
            side,
            Arc::new(DirDiffTree::new_empty()),
            ListState::default().with_selected(Some(0)),
        )
    }

    #[test]
    fn expand_path_adds_entry() {
        let mut state = empty_state(DiffSide::Left);
        state.expand_path("a/b");
        assert!(state.expanded_pathes.contains(Path::new("a/b")));
    }

    #[test]
    fn collapse_path_removes_entry() {
        let mut state = empty_state(DiffSide::Left);
        state.expand_path("a/b");
        state.collapse_path("a/b");
        assert!(!state.expanded_pathes.contains(Path::new("a/b")));
    }

    #[test]
    fn toggle_path_adds_then_removes() {
        let mut state = empty_state(DiffSide::Left);
        state.toggle_path("c/d");
        assert!(state.expanded_pathes.contains(Path::new("c/d")));
        state.toggle_path("c/d");
        assert!(!state.expanded_pathes.contains(Path::new("c/d")));
    }

    #[test]
    fn horizontal_scroll_roundtrip() {
        let mut state = empty_state(DiffSide::Left);
        state.set_horizontal_scroll(7);
        assert_eq!(state.horizontal_scroll(), 7);
        state.set_horizontal_scroll(0);
        assert_eq!(state.horizontal_scroll(), 0);
    }

    #[test]
    fn selected_path_is_none_before_render() {
        let state = empty_state(DiffSide::Left);
        assert!(state.selected_path().is_none());
    }

    #[test]
    fn nav_down_increments_selection() {
        let mut state = empty_state(DiffSide::Left);
        let mut term = make_terminal();
        state.handler(&Action::NavDown, &mut term).unwrap();
        assert_eq!(state.selected().selected(), Some(1));
    }

    #[test]
    fn nav_up_at_zero_stays_at_zero() {
        let mut state = empty_state(DiffSide::Left);
        let mut term = make_terminal();
        state.handler(&Action::NavUp, &mut term).unwrap();
        assert_eq!(state.selected().selected(), Some(0));
    }

    #[test]
    fn render_with_real_tree_populates_items() {
        let base = PathBuf::from("test/folder_cmp/same");
        let tree = Arc::new(DirDiffTree::new(base.join("lhs"), base.join("rhs")).unwrap());
        let mut state = FolderViewState::new(DiffSide::Left, tree, ListState::default());
        let area = Rect::new(0, 0, 80, 24);
        let mut buf = Buffer::empty(area);
        FolderView::new("test".to_string(), &HashSet::new()).render(area, &mut buf, &mut state);
        assert!(!state.items_full_name.is_empty());
    }

    #[test]
    fn render_expanded_dir_shows_more_items() {
        let base = PathBuf::from("test/folder_cmp/same");
        let tree = Arc::new(DirDiffTree::new(base.join("lhs"), base.join("rhs")).unwrap());
        let mut state = FolderViewState::new(DiffSide::Left, tree, ListState::default());
        let area = Rect::new(0, 0, 80, 24);

        let mut buf = Buffer::empty(area);
        FolderView::new("test".to_string(), &HashSet::new()).render(area, &mut buf, &mut state);
        let collapsed = state.items_full_name.len();

        let mut expanded = HashSet::new();
        expanded.insert(PathBuf::from("b"));
        buf = Buffer::empty(area);
        FolderView::new("test".to_string(), &expanded).render(area, &mut buf, &mut state);
        let expanded = state.items_full_name.len();

        assert!(expanded > collapsed, "expanded {expanded} > collapsed {collapsed}");
    }
}
