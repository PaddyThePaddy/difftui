use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{Event, KeyCode},
    layout::{Constraint, Direction, Layout, Spacing},
    widgets::{ListState, Paragraph, StatefulWidget, Widget},
};
use tracing::{error, trace};

use crate::{
    DiffTuiError,
    diff::{
        DiffSide,
        dir::{DirDiffTree, build_diff_tree, cmp_tree},
    },
    ui::{
        Action, EventHandler, TuiTerminal, folder_view::{FolderView, FolderViewState}, run_ext_tui_app
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
    tree: Arc<Mutex<DirDiffTree>>,
    lhs_state: FolderViewState,
    rhs_state: FolderViewState,
    expanded_pathes: HashSet<PathBuf>,
    focus: FocusState,
    horizontal_scroll: usize,
    cmp_in_progress: Option<JoinHandle<Result<DirDiffTree, DiffTuiError>>>,
}

impl FolderCmpState {
    pub fn new(lhs: impl AsRef<Path>, rhs: impl AsRef<Path>) -> Result<Self, DiffTuiError> {
        let lhs = lhs.as_ref();
        let rhs = rhs.as_ref();

        let tree = Arc::new(Mutex::new(build_diff_tree(lhs, rhs)?));
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
            tree: tree.clone(),
            horizontal_scroll: 0,
            cmp_in_progress: None,
        })
    }
}

impl EventHandler for FolderCmpState {
    fn handler(
        &mut self,
        event: &Action,
        terminal: &mut TuiTerminal,
    ) -> Result<(), DiffTuiError> {
        trace!("Handling event: {event:?}");
        if let Some(handle) = self.cmp_in_progress.take_if(|h| h.is_finished()) {
            let result = handle.join().map_err(|_| DiffTuiError::ThreadPaniced)??;
            *self.tree.lock().unwrap() = result;
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
                                let tree = self.tree.lock().unwrap();
                                let copied_tree: DirDiffTree = tree.clone();
                                let lhs_path = self.lhs_path.clone();
                                let rhs_path = self.rhs_path.clone();
                                let root = selected_p.clone();
                                self.cmp_in_progress = Some(std::thread::spawn(move || {
                                    cmp_tree(&copied_tree, &root, &lhs_path, &rhs_path)?;
                                    return Ok::<DirDiffTree, DiffTuiError>(copied_tree);
                                }));
                            }
                        }
                    }
                    Action::CompareAll => {
                        if self.cmp_in_progress.is_some() {
                            error!("There is a comparison in progress");
                        } else {
                            let tree = self.tree.lock().unwrap();
                            let copied_tree: DirDiffTree = tree.clone();
                            let lhs_path = self.lhs_path.clone();
                            let rhs_path = self.rhs_path.clone();
                            self.cmp_in_progress = Some(std::thread::spawn(move || {
                                cmp_tree(&copied_tree, Path::new(""), &lhs_path, &rhs_path)?;
                                return Ok::<DirDiffTree, DiffTuiError>(copied_tree);
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
                Ok(())
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
            Paragraph::new("Comparing...").render(status_line, buf);
        }

        if let FocusState::Synced(list_state) = &mut state.focus {
            *list_state = state.lhs_state.selected();
        }
    }
}
