use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    symbols::merge::MergeStrategy,
    text::{Line, Span, Text},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use regex::bytes::Regex;
use tracing::{error, trace};

use crate::{
    diff::{DiffSide, DiffState, dir::DirDiffTree},
    ui::{Action, EventHandler, Notification},
};

#[derive(Debug, Clone)]
pub struct FolderViewState {
    side: DiffSide,
    tree: Arc<DirDiffTree>,
    selection: ListState,
    expanded_pathes: HashSet<PathBuf>,
    items_full_name: Vec<PathBuf>,
    horizontal_scroll: usize,
    page_height: Option<u16>,
    highlight: Option<Regex>,
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
            page_height: None,
            highlight: None,
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
            self.get_item_full_name(idx)
        } else {
            None
        }
    }

    pub fn get_item_full_name(&self, idx: usize) -> Option<&PathBuf> {
        self.items_full_name.get(idx)
    }

    pub fn horizontal_scroll(&self) -> usize {
        self.horizontal_scroll
    }

    pub fn set_horizontal_scroll(&mut self, scroll: usize) {
        self.horizontal_scroll = scroll;
    }

    pub fn set_tree(&mut self, new_tree: Arc<DirDiffTree>) {
        self.tree = new_tree;
    }

    pub fn len(&self) -> usize {
        self.items_full_name.len()
    }

    pub fn set_hl(&mut self, pattern: Option<Regex>) {
        self.highlight = pattern;
    }
}

impl EventHandler for FolderViewState {
    fn handler(&mut self, event: &Action) -> Result<Option<Action>, crate::DiffTuiError> {
        match event {
            Action::NavUp => self.selection.scroll_up_by(1),
            Action::NavDown => self.selection.scroll_down_by(1),
            Action::PageDown(factor) => {
                if let Some(page_height) = self.page_height {
                    let line = (page_height as f32 * *factor).floor() as u16;
                    for _ in 0..line {
                        self.selection.select_next();
                    }
                }
            }
            Action::PageUp(factor) => {
                if let Some(page_height) = self.page_height {
                    let line = (page_height as f32 * *factor).floor() as u16;
                    for _ in 0..line {
                        self.selection.select_previous();
                    }
                }
            }
            Action::ToggleSelected => self.toggle_selected(),
            Action::NextDiff => {
                if let Some(selected) = self.selection.selected() {
                    let mut next = selected + 1;
                    let mut changed = false;
                    while let Some(p) = self.get_item_full_name(next) {
                        let state = self.tree.get_diff_state(p);
                        if state != DiffState::Same && state != DiffState::Unknown {
                            self.selection.select(Some(next));
                            changed = true;
                            break;
                        }
                        next += 1;
                    }
                    if !changed {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Next diff".to_string(),
                            body: "Reached last diff".to_string(),
                        })));
                    }
                }
            }
            Action::PrevDiff => {
                if let Some(selected) = self.selection.selected() {
                    let mut prev = selected.saturating_sub(1);
                    let mut changed = false;
                    while let Some(p) = self.get_item_full_name(prev) {
                        let state = self.tree.get_diff_state(p);
                        if state != DiffState::Same && state != DiffState::Unknown {
                            self.selection.select(Some(prev));
                            changed = true;
                            break;
                        }
                        if prev == 0 {
                            break;
                        }
                        prev = prev.saturating_sub(1);
                    }
                    if !changed {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Previous diff".to_string(),
                            body: "Reached first diff".to_string(),
                        })));
                    }
                }
            }
            Action::SearchNext(r) => {
                self.highlight = Some(r.clone());
                error!("highligh= {:?}", self.highlight);
                if let Some(current) = self.selection.selected() {
                    let mut idx = current + 1;
                    while let Some(p) = self.get_item_full_name(idx) {
                        if let Some(name) = p.file_name() {
                            let name = name.to_string_lossy();
                            if r.is_match(name.as_bytes()) {
                                self.selection.select(Some(idx));
                                return Ok(None);
                            }
                        }
                        idx += 1;
                    }
                    idx = 0;
                    while let Some(p) = self.get_item_full_name(idx) {
                        if idx == current {
                            return Ok(Some(Action::Notification(Notification {
                                title: "Search".to_string(),
                                body: "No matches found".to_string(),
                            })));
                        }
                        if let Some(name) = p.file_name() {
                            let name = name.to_string_lossy();
                            if r.is_match(name.as_bytes()) {
                                self.selection.select(Some(idx));
                                return Ok(None);
                            }
                        }
                        idx += 1;
                    }
                }
            }
            Action::SearchPrev(r) => {
                self.highlight = Some(r.clone());
                if let Some(current) = self.selection.selected() {
                    let mut idx = current;
                    if idx != 0 {
                        idx -= 1;
                        while let Some(p) = self.get_item_full_name(idx) {
                            if let Some(name) = p.file_name() {
                                let name = name.to_string_lossy();
                                if r.is_match(name.as_bytes()) {
                                    self.selection.select(Some(idx));
                                    return Ok(None);
                                }
                            }
                            if idx == 0 {
                                break;
                            }
                            idx -= 1;
                        }
                    }
                    idx = self.len().saturating_sub(1);
                    while let Some(p) = self.get_item_full_name(idx) {
                        if idx == current {
                            return Ok(Some(Action::Notification(Notification {
                                title: "Search".to_string(),
                                body: "No matches found".to_string(),
                            })));
                        }
                        if let Some(name) = p.file_name() {
                            let name = name.to_string_lossy();
                            if r.is_match(name.as_bytes()) {
                                self.selection.select(Some(idx));
                                return Ok(None);
                            }
                        }
                        idx -= 1;
                    }
                }
            }
            Action::RemoveHighlight => {
                self.highlight = None;
            }
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
        state.page_height = Some(area.height - 2);
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
                    state.highlight.as_ref(),
                );
            }

            StatefulWidget::render(
                List::new(list_items)
                    .scroll_padding(5)
                    .highlight_style(Style::new().bg(Color::DarkGray))
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
        hl_pattern: Option<&Regex>,
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
            if root.metadata().is_dir() {
                if self.expanded_pathes.contains(root_path) {
                    item_str.push('▾');
                } else {
                    item_str.push('▸');
                }
                item_str.push(' ');
            }
            item_str.push_str(
                root_path
                    .file_name()
                    .unwrap_or(OsStr::new(""))
                    .to_string_lossy()
                    .as_ref(),
            );
            let scroll_point = item_str
                .char_indices()
                .nth(horizontal_scroll)
                .map(|(i, _)| i)
                .unwrap_or(item_str.len());
            let list_item = if let Some(hl_pat) = hl_pattern {
                let mut text_objs = vec![];
                let mut line_str = &item_str[scroll_point..];
                for m in hl_pat.find_iter(line_str.as_bytes()) {
                    text_objs.push(Span::raw(line_str[..m.start()].to_string()));
                    text_objs.push(Span::styled(
                        String::from_utf8_lossy(m.as_bytes()).to_string(),
                        Style::default().on_light_red(),
                    ));
                    line_str = &line_str[m.end()..];
                }
                text_objs.push(Span::raw(line_str.to_string()));
                ListItem::from(Line::from_iter(text_objs))
            } else {
                ListItem::from(item_str[scroll_point..].to_string())
            }
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
                    hl_pattern,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{
        Terminal, TerminalOptions, Viewport, buffer::Buffer, layout::Rect, widgets::StatefulWidget,
    };

    fn make_terminal() -> crate::ui::TuiTerminal {
        Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(std::io::stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
            },
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
        state.handler(&Action::NavDown).unwrap();
        assert_eq!(state.selected().selected(), Some(1));
    }

    #[test]
    fn nav_up_at_zero_stays_at_zero() {
        let mut state = empty_state(DiffSide::Left);
        state.handler(&Action::NavUp).unwrap();
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

        assert!(
            expanded > collapsed,
            "expanded {expanded} > collapsed {collapsed}"
        );
    }
}
