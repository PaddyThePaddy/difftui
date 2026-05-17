use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use anyhow::Result;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{Event, KeyCode},
    layout::{Constraint, Direction, Layout},
    widgets::{ListState, StatefulWidget},
};

use crate::{
    DiffTuiError,
    diff::{
        DiffSide,
        dir::{DirDiffTree, build_diff_tree, cmp_tree},
    },
    ui::{
        ControlEvent, EventHandler,
        folder_view::{FolderView, FolderViewState},
        run_ext_tui_app,
    },
};

#[derive(Debug, Clone, Copy)]
enum FocusState {
    Synced(ListState),
    FocusOn(DiffSide),
}

#[derive(Debug, Clone)]
pub struct FolderCmpState {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    tree: Arc<Mutex<DirDiffTree>>,
    lhs_state: FolderViewState,
    rhs_state: FolderViewState,
    expanded_pathes: HashSet<PathBuf>,
    focus: FocusState,
    horizontal_scroll: usize,
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
        })
    }
}

impl EventHandler for FolderCmpState {
    fn handler(
        &mut self,
        event: &ControlEvent,
        terminal: &mut DefaultTerminal,
    ) -> Result<(), DiffTuiError> {
        match &mut self.focus {
            FocusState::Synced(list_state) => {
                match event {
                    ControlEvent::NavUp => {
                        list_state.scroll_up_by(1);
                        *self.lhs_state.selected_mut() = *list_state;
                        *self.rhs_state.selected_mut() = *list_state;
                    }
                    ControlEvent::NavDown => {
                        list_state.scroll_down_by(1);
                        *self.lhs_state.selected_mut() = *list_state;
                        *self.rhs_state.selected_mut() = *list_state;
                    }
                    ControlEvent::ToggleSelected => {
                        if let Some(selected_p) = self.lhs_state.selected_path() {
                            if self.expanded_pathes.contains(selected_p) {
                                self.expanded_pathes.remove(selected_p);
                            } else {
                                self.expanded_pathes.insert(selected_p.clone());
                            }
                        }
                    }
                    ControlEvent::CompareSelected => {
                        if let Some(selected_p) = self.lhs_state.selected_path() {
                            let tree = self.tree.lock().unwrap();
                            cmp_tree(&tree, selected_p, &self.lhs_path, &self.rhs_path)?;
                        }
                    }
                    ControlEvent::CompareAll => {
                        let tree = self.tree.lock().unwrap();
                        cmp_tree(&tree, Path::new(""), &self.lhs_path, &self.rhs_path)?;
                    }
                    ControlEvent::LauchExtCompare => {
                        if let Some(selected_path) = self.lhs_state.selected_path() {
                            let lhs_path = self.lhs_path.join(selected_path);
                            let rhs_path = self.rhs_path.join(selected_path);
                            let mut cmd = std::process::Command::new("nvim");
                            cmd.arg("-d").arg(lhs_path).arg(rhs_path);
                            run_ext_tui_app(&mut cmd, terminal)?;
                        }
                    }
                    ControlEvent::NavLeft => {
                            self.horizontal_scroll = self.horizontal_scroll.saturating_sub(1);
                            self.lhs_state.set_horizontal_scroll(self.horizontal_scroll);
                            self.rhs_state.set_horizontal_scroll(self.horizontal_scroll);
                    }
                    ControlEvent::NavRight => {
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
        let layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        );
        let [lhs_area, rhs_area] = area.layout(&layout);
        FolderView::new(
            state.lhs_path.to_string_lossy().to_string(),
            &state.expanded_pathes,
        )
        .render(lhs_area, buf, &mut state.lhs_state);
        FolderView::new(
            state.lhs_path.to_string_lossy().to_string(),
            &state.expanded_pathes,
        )
        .render(rhs_area, buf, &mut state.rhs_state);
    }
}
