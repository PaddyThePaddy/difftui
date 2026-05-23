use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
    thread::JoinHandle,
};

use anyhow::Result;
use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    widgets::{ListState, Paragraph, StatefulWidget, Widget},
};
use tracing::{error, trace};

use crate::{
    DiffTuiError,
    diff::{DiffSide, dir::DirDiffTree},
    ui::{
        Action, EventHandler, TuiTerminal,
        folder_view::{FolderView, FolderViewState},
        loading_msg::{LoadingMsg, LoadingMsgState},
        run_ext_tui_app,
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
        })
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
                        } else {
                            let tree = self.tree.clone();
                            self.cmp_in_progress = Some(std::thread::spawn(move || {
                                tree.cmp_node(Path::new(""))?;
                                return Ok::<(), DiffTuiError>(());
                            }));
                        }
                    }
                    Action::LauchExtCompare => {
                        if let Some(selected_path) = self.lhs_state.selected_path() {
                            let lhs_path = self.lhs_path.join(selected_path);
                            let rhs_path = self.rhs_path.join(selected_path);
                            let mut cmd = std::process::Command::new("nvim");
                            cmd.arg("-d").arg(lhs_path).arg(rhs_path);
                            run_ext_tui_app(&mut cmd, terminal)?;
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
                    _ => {}
                }
                Ok(None)
            }
            FocusState::FocusOn(diff_side) => match diff_side {
                DiffSide::Left => self.lhs_state.handler(event, terminal),
                DiffSide::Right => self.rhs_state.handler(event, terminal),
            },
        }
    }
}

#[derive(Debug, Default)]
pub struct FolderCmpView;

impl StatefulWidget for FolderCmpView {
    type State = FolderCmpState;

    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::TuiTerminal;
    use ratatui::{Terminal, TerminalOptions, Viewport, layout::Rect};

    fn make_terminal() -> TuiTerminal {
        Terminal::with_options(
            ratatui::backend::CrosstermBackend::new(std::io::stderr()),
            TerminalOptions { viewport: Viewport::Fixed(Rect::new(0, 0, 80, 24)) },
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
        assert_eq!(state.handler(&Action::Tick, &mut term).unwrap(), None);
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
