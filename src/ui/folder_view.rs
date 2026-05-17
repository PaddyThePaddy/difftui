use std::{
    collections::HashSet,
    ffi::OsStr,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
};

use ratatui::{
    DefaultTerminal, buffer::Buffer, layout::Rect, style::{Color, Style, Styled, Stylize}, widgets::{
        Block, Borders, HighlightSpacing, List, ListItem, ListState, Paragraph, StatefulWidget,
        Widget,
    }
};

use crate::{
    diff::{DiffSide, DiffState, dir::DirDiffTree},
    ui::{ControlEvent, EventHandler},
};

#[derive(Debug, Clone)]
pub struct FolderViewState {
    side: DiffSide,
    tree: Arc<Mutex<DirDiffTree>>,
    selection: ListState,
    expanded_pathes: HashSet<PathBuf>,
    items_full_name: Vec<PathBuf>,
}

static NORMAL_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default());
static DIFF_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default().fg(Color::Red));
static ORPHAN_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default().fg(Color::Red));
static SAME_LIST_STYLE: LazyLock<Style> = LazyLock::new(|| Style::default().fg(Color::Green));

impl FolderViewState {
    pub fn new(side: DiffSide, tree: Arc<Mutex<DirDiffTree>>, selection: ListState) -> Self {
        Self {
            side,
            tree,
            selection,
            expanded_pathes: HashSet::new(),
            items_full_name: Vec::new(),
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
}

impl EventHandler for FolderViewState {
    fn handler(&mut self, event: &ControlEvent, _: &mut DefaultTerminal) -> Result<(), crate::DiffTuiError> {
        match event {
            ControlEvent::NavUp => self.selection.scroll_up_by(1),
            ControlEvent::NavDown => self.selection.scroll_down_by(1),
            ControlEvent::ToggleSelected => self.toggle_selected(),
            _ => {}
        }
        Ok(())
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
            .borders(Borders::all());

        let tree = state.tree.lock().unwrap();
        if let Some(root) = tree.get(Path::new("")) {
            let mut list_items = Vec::new();
            for child in root.borrow().children() {
                self.generate_list_item(
                    &tree,
                    &mut state.items_full_name,
                    child,
                    &mut list_items,
                    0,
                    state.side,
                );
            }

            if let Some(selected_item) = state.selection.selected().and_then(|idx| list_items.get_mut(idx)) {
                *selected_item = selected_item.clone().on_blue();
            }
            
            StatefulWidget::render(
                List::new(list_items)
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
        Self { title, expanded_pathes }
    }

    fn generate_list_item(
        &self,
        tree: &DirDiffTree,
        items_full_name: &mut Vec<PathBuf>,
        root_path: &Path,
        list: &mut Vec<ListItem>,
        level: usize,
        side: DiffSide,
    ) {
        let root = match tree.get(root_path) {
            Some(n) => n,
            None => return,
        };

        let mut item_str = String::new();
        for _ in 0..level {
            item_str.push_str(Self::INDENTION);
        }

        if root.borrow().diff_state().is_orphan(side.oppsite()) {
            list.push("".into());
        } else {
            item_str.push_str(
                root_path
                    .file_name()
                    .unwrap_or(OsStr::new(""))
                    .to_string_lossy()
                    .as_ref(),
            );
            if root.borrow().ent_type().is_dir() {
                item_str.push('/');
            }
            let list_item = ListItem::from(item_str).style(match root.borrow().diff_state() {
                DiffState::Unknown => NORMAL_LIST_STYLE.clone(),
                DiffState::Orphan(_) => ORPHAN_LIST_STYLE.clone(),
                DiffState::Different => DIFF_LIST_STYLE.clone(),
                DiffState::Same => SAME_LIST_STYLE.clone(),
            });

            list.push(list_item);
        }
        items_full_name.push(root_path.to_path_buf());

        if self.expanded_pathes.contains(root_path) {
            for entry in root.borrow().children().iter() {
                self.generate_list_item(tree, items_full_name, entry, list, level + 1, side);
            }
        }
    }
}
