use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
};

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use globset::{Glob, GlobSet, GlobSetBuilder};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Spacing},
    widgets::{Block, Clear, ListState, StatefulWidget, Widget as _},
};
use ratatui_textarea::{CursorMove, TextArea};
use tracing::{error, trace};

use crate::{
    DiffTuiError,
    diff::{DiffSide, dir::DirDiffTree},
    ui::{
        Action, EventHandler, Notification, Popup, TuiTerminal,
        folder_view::{FolderView, FolderViewState},
        loading_msg::{LoadingMsg, LoadingMsgState},
        run_ext_tui_app, tui,
    },
};

#[derive(Debug, Clone, Copy)]
enum FocusState {
    Synced(ListState),
    FocusOn(DiffSide),
}

#[derive(Debug)]
pub struct FolderCmpState {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    tree: Arc<DirDiffTree>,
    lhs_state: FolderViewState,
    rhs_state: FolderViewState,
    expanded_pathes: HashSet<PathBuf>,
    focus: FocusState,
    horizontal_scroll: usize,
    cmp_in_progress: Option<JoinHandle<Result<(), DiffTuiError>>>,
    loading_tree: Option<JoinHandle<Result<DirDiffTree, DiffTuiError>>>,
    loading_msg_state: LoadingMsgState,
    filter_text: Vec<String>,
    filters: GlobSet,
    display_map: HashSet<PathBuf>,
    filtered_tree: Option<Arc<DirDiffTree>>,
}

impl FolderCmpState {
    pub fn new(lhs: impl AsRef<Path>, rhs: impl AsRef<Path>) -> Result<Self, DiffTuiError> {
        let lhs = lhs.as_ref();
        let rhs = rhs.as_ref();

        let tree_loading_handle = {
            let lhs = lhs.to_path_buf();
            let rhs = rhs.to_path_buf();
            std::thread::spawn(|| DirDiffTree::new(lhs, rhs))
        };
        let tree = Arc::new(DirDiffTree::new_empty());
        let lhs_state = FolderViewState::new(
            DiffSide::Left,
            tree.clone(),
            ListState::default().with_selected(Some(0)),
        );
        let rhs_state = FolderViewState::new(
            DiffSide::Right,
            tree.clone(),
            ListState::default().with_selected(Some(0)),
        );
        let default_glob_set = GlobSetBuilder::new().build()?;

        Ok(Self {
            lhs_path: lhs.to_path_buf(),
            rhs_path: rhs.to_path_buf(),
            lhs_state: lhs_state,
            rhs_state: rhs_state,
            expanded_pathes: HashSet::new(),
            focus: FocusState::Synced(ListState::default().with_selected(Some(0))),
            tree,
            horizontal_scroll: 0,
            cmp_in_progress: None,
            loading_tree: Some(tree_loading_handle),
            loading_msg_state: LoadingMsgState::default(),
            filter_text: vec![],
            filters: default_glob_set,
            display_map: HashSet::new(),
            filtered_tree: None,
        })
    }

    pub fn set_filters(&mut self, filters: GlobSet) {
        if filters.is_empty() {
            self.display_map.clear();
            self.filtered_tree = None;
            self.lhs_state.set_tree(self.tree.clone());
            self.rhs_state.set_tree(self.tree.clone());
        } else {
            self.filters = filters;
            self.display_map = self.build_display_map();
            let new_tree = Arc::new(self.tree.clone_filtered_tree(&self.display_map));
            self.lhs_state.set_tree(new_tree.clone());
            self.rhs_state.set_tree(new_tree.clone());
            self.filtered_tree = Some(new_tree);
        }
    }

    fn build_display_map(&self) -> HashSet<PathBuf> {
        let mut map = HashSet::new();
        map.insert(PathBuf::from(""));
        Self::build_display_map_worker(&self.tree, &self.filters, Path::new(""), &mut map);
        return map;
    }

    fn build_display_map_worker(
        tree: &DirDiffTree,
        filters: &GlobSet,
        p: &Path,
        map: &mut HashSet<PathBuf>,
    ) -> bool {
        let node = match tree.get_fs_node(p) {
            Some(n) => n,
            None => return false,
        };
        let mut should_display = false;

        if node.metadata().is_dir() {
            for child in node.children() {
                if Self::build_display_map_worker(tree, filters, child, map) {
                    should_display = true;
                }
            }
        }

        if filters.is_match(p) {
            should_display = true;
        }

        if should_display {
            map.insert(p.to_path_buf());
        }

        return should_display;
    }
}

impl EventHandler for FolderCmpState {
    fn handler(
        &mut self,
        event: &Action,
        terminal: &mut TuiTerminal,
    ) -> Result<Option<Action>, DiffTuiError> {
        trace!("Handling event: {event:?}");

        if let Action::Tick = *event {
            self.loading_msg_state.step();

            if self
                .cmp_in_progress
                .as_ref()
                .is_some_and(|h| h.is_finished())
            {
                if let Some(h) = self.cmp_in_progress.take() {
                    if let Err(e) = h.join() {
                        error!("Comparing thread panic: {e:?}");
                        return Ok(Some(Action::Notification(Notification {
                            title: "Alert".to_string(),
                            body: format!("Comparing thread panic: {e:?}"),
                        })));
                    }
                }
            }

            if self.loading_tree.as_ref().is_some_and(|h| h.is_finished()) {
                if let Some(h) = self.loading_tree.take() {
                    match h.join() {
                        Ok(tree) => match tree {
                            Ok(tree) => {
                                let tree = Arc::new(tree);
                                self.lhs_state = FolderViewState::new(
                                    DiffSide::Left,
                                    tree.clone(),
                                    ListState::default().with_selected(Some(0)),
                                );
                                self.rhs_state = FolderViewState::new(
                                    DiffSide::Right,
                                    tree.clone(),
                                    ListState::default().with_selected(Some(0)),
                                );
                                self.tree = tree;
                                self.display_map = self.build_display_map();
                            }
                            Err(e) => {
                                error!("Build tree failed: {e:?}");
                                return Ok(Some(Action::ExitApp(Some(format!(
                                    "Build tree failed: {e:?}"
                                )))));
                            }
                        },
                        Err(e) => {
                            error!("Build tree failed: {e:?}");
                            return Ok(Some(Action::ExitApp(Some(format!(
                                "Build tree failed: {e:?}"
                            )))));
                        }
                    }
                }
            }
            return Ok(None);
        }

        match event {
            Action::PopupFilter => {
                trace!("Request showing popup filter");
                return Ok(Some(Action::ShowPopup(Box::new(FilterPopup::new(Some(
                    self.filter_text.clone(),
                ))))));
            }
            Action::PopupReturn(id, body) => {
                if id == FilterPopup::ID_FILTER_CONFIRMED {
                    let mut glob_builder = GlobSetBuilder::new();
                    let lines: Vec<String> = body.lines().map(|s| s.to_string()).collect();

                    for line in lines.iter() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        let glob = match Glob::new(line) {
                            Ok(g) => g,
                            Err(e) => {
                                return Ok(Some(Action::Notification(Notification {
                                    title: "Error".to_string(),
                                    body: format!("Parsing glob failed: {e}"),
                                })));
                            }
                        };
                        glob_builder.add(glob);
                    }
                    let filters = match glob_builder.build() {
                        Ok(f) => f,
                        Err(e) => {
                            return Ok(Some(Action::Notification(Notification {
                                title: "Error".to_string(),
                                body: format!("Build glob set failed: {e}"),
                            })));
                        }
                    };
                    self.filter_text = lines;
                    self.set_filters(filters);
                    return Ok(None);
                }
            }
            Action::LauchExtCompare => {
                if let Some((lhs, rhs)) = self
                    .lhs_state
                    .selected_path()
                    .zip(self.rhs_state.selected_path())
                {
                    let lhs_path = self.lhs_path.join(lhs);
                    let rhs_path = self.rhs_path.join(rhs);
                    let mut cmd = std::process::Command::new("nvim");
                    cmd.arg("-d").arg(lhs_path).arg(rhs_path);
                    run_ext_tui_app(&mut cmd, terminal)?;
                }
            }
            _ => {}
        }

        match &mut self.focus {
            FocusState::Synced(list_state) => {
                match event {
                    Action::NavUp => {
                        list_state.scroll_up_by(1);
                        *self.lhs_state.selected_mut() = *list_state;
                        *self.rhs_state.selected_mut() = *list_state;
                    }
                    Action::NavDown => {
                        list_state.scroll_down_by(1);
                        *self.lhs_state.selected_mut() = *list_state;
                        *self.rhs_state.selected_mut() = *list_state;
                    }
                    Action::NavTop => {
                        list_state.select_first();
                        *self.lhs_state.selected_mut() = *list_state;
                        *self.rhs_state.selected_mut() = *list_state;
                    }
                    Action::NavBottom => {
                        list_state.select_last();
                        *self.lhs_state.selected_mut() = *list_state;
                        *self.rhs_state.selected_mut() = *list_state;
                    }
                    Action::ToggleSelected => {
                        if let Some(selected_p) = self.lhs_state.selected_path() {
                            if self.expanded_pathes.contains(selected_p) {
                                self.expanded_pathes.remove(selected_p);
                            } else {
                                self.expanded_pathes.insert(selected_p.clone());
                            }
                        }
                    }
                    Action::CompareSelected => {
                        if let Some(selected_p) = self.lhs_state.selected_path() {
                            if self.cmp_in_progress.is_some() {
                                error!("There is a comparison in progress");
                            } else {
                                let tree = self.tree.clone();
                                let p = selected_p.clone();
                                self.cmp_in_progress = Some(std::thread::spawn(move || {
                                    tree.cmp_node(&p)?;
                                    return Ok::<(), DiffTuiError>(());
                                }));
                            }
                        }
                    }
                    Action::CompareAll => {
                        if self.cmp_in_progress.is_some() {
                            error!("There is a comparison in progress");
                            return Ok(Some(Action::Notification(Notification {
                                title: "Abort".to_string(),
                                body: "There is a comparison in progress".to_string(),
                            })));
                        } else {
                            let tree = if let Some(tree) = &self.filtered_tree {
                                trace!("Comparing filtered tree");
                                tree.clone()
                            } else {
                                self.tree.clone()
                            };
                            self.cmp_in_progress = Some(std::thread::spawn(move || {
                                tree.cmp_node(Path::new(""))?;
                                return Ok::<(), DiffTuiError>(());
                            }));
                        }
                    }
                    Action::NavLeft => {
                        self.horizontal_scroll = self.horizontal_scroll.saturating_sub(1);
                        self.lhs_state.set_horizontal_scroll(self.horizontal_scroll);
                        self.rhs_state.set_horizontal_scroll(self.horizontal_scroll);
                    }
                    Action::NavRight => {
                        self.horizontal_scroll = self.horizontal_scroll.saturating_add(1);
                        self.lhs_state.set_horizontal_scroll(self.horizontal_scroll);
                        self.rhs_state.set_horizontal_scroll(self.horizontal_scroll);
                    }
                    Action::ToggleCoupling => {
                        self.focus = FocusState::FocusOn(DiffSide::Left);
                    }
                    _ => {}
                }
                Ok(None)
            }
            FocusState::FocusOn(diff_side) => match event {
                Action::ToggleCoupling => {
                    self.focus = FocusState::Synced(match diff_side {
                        DiffSide::Left => self.lhs_state.selected(),
                        DiffSide::Right => self.rhs_state.selected(),
                    });
                    *self.rhs_state.selected_mut() = self.lhs_state.selected();
                    Ok(None)
                }
                Action::NavLeft => {
                    self.focus = FocusState::FocusOn(DiffSide::Left);
                    Ok(None)
                }
                Action::NavRight => {
                    self.focus = FocusState::FocusOn(DiffSide::Right);
                    Ok(None)
                }
                Action::ToggleSelected => {
                    if let Some(selected_p) = match diff_side {
                        DiffSide::Left => self.lhs_state.selected_path(),
                        DiffSide::Right => self.rhs_state.selected_path(),
                    } {
                        if self.expanded_pathes.contains(selected_p) {
                            self.expanded_pathes.remove(selected_p);
                        } else {
                            self.expanded_pathes.insert(selected_p.clone());
                        }
                    }
                    Ok(None)
                }
                _ => match diff_side {
                    DiffSide::Left => self.lhs_state.handler(event, terminal),
                    DiffSide::Right => self.rhs_state.handler(event, terminal),
                },
            },
        }
    }
}

#[derive(Debug, Default)]
pub struct FolderCmpView;

impl FolderCmpView {
    pub fn render(&self, frame: &mut Frame, state: &mut FolderCmpState) {
        let area = frame.area();
        let buf = frame.buffer_mut();

        if state.loading_tree.is_some() {
            let center_area = area.centered_vertically(Constraint::Length(1));
            LoadingMsg::new("Loading file tree").render(
                center_area,
                buf,
                &mut state.loading_msg_state,
            );
            return;
        }

        let vertical_layout = Layout::new(
            Direction::Vertical,
            [
                Constraint::Fill(1),
                Constraint::Max(if state.cmp_in_progress.is_some() {
                    1
                } else {
                    0
                }),
            ],
        );
        let horizontal_layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .spacing(Spacing::Overlap(1));
        let [main_area, status_line] = area.layout(&vertical_layout);
        let [lhs_area, rhs_area] = main_area.layout(&horizontal_layout);
        FolderView::new(
            state.lhs_path.to_string_lossy().to_string(),
            &state.expanded_pathes,
        )
        .render(lhs_area, buf, &mut state.lhs_state);
        FolderView::new(
            state.rhs_path.to_string_lossy().to_string(),
            &state.expanded_pathes,
        )
        .render(rhs_area, buf, &mut state.rhs_state);
        if state.cmp_in_progress.is_some() {
            LoadingMsg::new("Comparing...").render(status_line, buf, &mut state.loading_msg_state);
        }

        if let FocusState::Synced(list_state) = &mut state.focus {
            *list_state = state.lhs_state.selected();
        }
    }
}

#[derive(Debug)]
pub struct FilterPopup<'ta> {
    text: TextArea<'ta>,
}

impl<'ta> FilterPopup<'ta> {
    const ID_FILTER_CONFIRMED: &'static str = "filters_popup::confirmed";
    const ID_FILTER_CENCELED: &'static str = "filters_popup::canceled";
    pub fn new(text: Option<Vec<String>>) -> Self {
        Self {
            text: Self::build_filter_text_area(text),
        }
    }

    fn build_filter_text_area<'a>(text: Option<Vec<String>>) -> TextArea<'a> {
        let mut default_text_area = if let Some(text) = text {
            TextArea::new(text)
        } else {
            TextArea::default()
        };
        default_text_area.set_block(
            Block::bordered()
                .title("Filters")
                .title_bottom("<ESC> to cancel / <Ctrl-S> to confirm"),
        );
        default_text_area
    }
}

impl<'ta> Popup for FilterPopup<'ta> {
    fn handler(&mut self, event: &crate::ui::tui::Event) -> Option<Action> {
        if let tui::Event::Key(key_evt) = event {
            if key_evt.modifiers == KeyModifiers::CONTROL && key_evt.code == KeyCode::Char('s') {
                return Some(Action::PopupReturn(
                    Self::ID_FILTER_CONFIRMED.to_string(),
                    self.text.lines().join("\n"),
                ));
            } else if key_evt.code == KeyCode::Esc {
                return Some(Action::PopupReturn(
                    Self::ID_FILTER_CENCELED.to_string(),
                    self.text.lines().join("\n"),
                ));
            } else if key_evt.code == KeyCode::Char('d')
                && key_evt.modifiers == KeyModifiers::CONTROL
            {
                let mut lines = self.text.lines().to_vec();
                let cursor = self.text.cursor();
                lines.remove(cursor.0);
                let mut new_area = Self::build_filter_text_area(Some(lines));
                new_area.move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
                self.text = new_area;
            } else {
                self.text.input(*key_evt);
            }
            return None;
        }
        return None;
    }

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let buf = frame.buffer_mut();
        let popup_area = area.centered(Constraint::Percentage(50), Constraint::Percentage(50));

        Clear::default().render(popup_area, buf);
        self.text.render(popup_area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::TuiTerminal;
    use ratatui::{Terminal, TerminalOptions, Viewport, layout::Rect};

    fn make_terminal() -> TuiTerminal {
        Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(std::io::stdout()),
            TerminalOptions {
                viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)),
            },
        )
        .unwrap()
    }

    fn fixture_state() -> FolderCmpState {
        let base = PathBuf::from("test/folder_cmp/same");
        FolderCmpState::new(base.join("lhs"), base.join("rhs")).unwrap()
    }

    #[test]
    fn new_starts_loading_tree_in_background() {
        let state = fixture_state();
        assert!(state.loading_tree.is_some());
    }

    #[test]
    fn tick_returns_none() {
        let mut state = fixture_state();
        let mut term = make_terminal();
        assert!(state.handler(&Action::Tick, &mut term).unwrap().is_none());
    }

    #[test]
    fn nav_right_increments_horizontal_scroll_on_both_panels() {
        let mut state = fixture_state();
        let mut term = make_terminal();
        assert_eq!(state.horizontal_scroll, 0);
        state.handler(&Action::NavRight, &mut term).unwrap();
        assert_eq!(state.horizontal_scroll, 1);
        assert_eq!(state.lhs_state.horizontal_scroll(), 1);
        assert_eq!(state.rhs_state.horizontal_scroll(), 1);
    }

    #[test]
    fn nav_left_decrements_horizontal_scroll() {
        let mut state = fixture_state();
        let mut term = make_terminal();
        state.handler(&Action::NavRight, &mut term).unwrap();
        state.handler(&Action::NavRight, &mut term).unwrap();
        state.handler(&Action::NavLeft, &mut term).unwrap();
        assert_eq!(state.horizontal_scroll, 1);
    }

    #[test]
    fn nav_left_saturates_at_zero() {
        let mut state = fixture_state();
        let mut term = make_terminal();
        state.handler(&Action::NavLeft, &mut term).unwrap();
        assert_eq!(state.horizontal_scroll, 0);
    }

    #[test]
    fn synced_nav_down_keeps_both_panels_in_sync() {
        let mut state = fixture_state();
        let mut term = make_terminal();
        state.handler(&Action::NavDown, &mut term).unwrap();
        assert_eq!(
            state.lhs_state.selected().selected(),
            state.rhs_state.selected().selected(),
        );
    }

    #[test]
    fn synced_nav_up_keeps_both_panels_in_sync() {
        let mut state = fixture_state();
        let mut term = make_terminal();
        state.handler(&Action::NavDown, &mut term).unwrap();
        state.handler(&Action::NavUp, &mut term).unwrap();
        assert_eq!(
            state.lhs_state.selected().selected(),
            state.rhs_state.selected().selected(),
        );
    }
}
