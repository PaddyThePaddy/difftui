use std::path::PathBuf;

use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    prelude::{Buffer, Rect},
    symbols::merge::MergeStrategy,
    widgets::{Block, StatefulWidget},
};

use crate::{
    DiffTuiError,
    ui::{
        EventHandler, Notification, TabState,
        hex_view::{HexView, HexViewState},
    },
};

use super::Action;

#[derive(Debug)]
pub struct HexCmpView {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    lhs_buf: Vec<u8>,
    rhs_buf: Vec<u8>,
    diff_hl: Vec<(usize, usize)>,
    lhs_state: HexViewState,
    rhs_state: HexViewState,
}

impl HexCmpView {
    pub fn new(lhs: PathBuf, rhs: PathBuf) -> Result<Self, DiffTuiError> {
        let lhs_buf = std::fs::read(&lhs)?;
        let rhs_buf = std::fs::read(&rhs)?;

        let diff_hl = compare_diff_hunks(&lhs_buf, &rhs_buf);

        Ok(Self {
            lhs_path: lhs,
            rhs_path: rhs,
            lhs_buf,
            rhs_buf,
            diff_hl,
            lhs_state: HexViewState::default().with_selected(Some(0)),
            rhs_state: HexViewState::default().with_selected(Some(0)),
        })
    }
}

impl EventHandler for HexCmpView {
    fn handler(&mut self, event: &Action) -> Result<Option<Action>, DiffTuiError> {
        match event {
            Action::NavUp => {
                self.lhs_state.move_sel_up();
                self.rhs_state.move_sel_up();
            }
            Action::NavDown => {
                self.lhs_state.move_sel_down();
                self.rhs_state.move_sel_down();
            }
            Action::NavLeft => {
                self.lhs_state.move_sel_left();
                self.rhs_state.move_sel_left();
            }
            Action::NavRight => {
                self.lhs_state.move_sel_right();
                self.rhs_state.move_sel_right();
            }
            Action::PageUp(fac) => {
                self.lhs_state.move_sel_up_page(*fac);
                self.rhs_state.move_sel_up_page(*fac);
            }
            Action::PageDown(fac) => {
                self.lhs_state.move_sel_down_page(*fac);
                self.rhs_state.move_sel_down_page(*fac);
            }
            Action::NextDiff => {
                let selected = self.lhs_state.selected().unwrap_or(0);

                if let Some(next_hunk) = self.diff_hl.iter().find(|h| h.0 > selected) {
                    self.lhs_state.set_selected(Some(next_hunk.0));
                    self.rhs_state.set_selected(Some(next_hunk.0));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Next diff".to_string(),
                        body: "Reached last diff".to_string(),
                    })));
                }
            }
            Action::PrevDiff => {
                let selected = self.lhs_state.selected().unwrap_or(0);

                if let Some(prev_hunk) = self.diff_hl.iter().rev().find(|h| h.1 < selected) {
                    self.lhs_state.set_selected(Some(prev_hunk.0));
                    self.rhs_state.set_selected(Some(prev_hunk.0));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Previous diff".to_string(),
                        body: "Reached first diff".to_string(),
                    })));
                }
            }
            _ => {}
        }
        Ok(None)
    }
}

impl TabState for HexCmpView {
    fn title(&self) -> String {
        "HEX".to_string()
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Percentage(50), Constraint::Percentage(50)],
        )
        .spacing(Spacing::Overlap(1));
        let [lhs_area, rhs_area] = area.layout(&layout);

        HexView::new(&self.lhs_buf)
            .set_hl_groups(Some(&self.diff_hl))
            .block(Block::bordered().merge_borders(MergeStrategy::Exact))
            .render(lhs_area, buf, &mut self.lhs_state);
        HexView::new(&self.rhs_buf)
            .set_hl_groups(Some(&self.diff_hl))
            .block(Block::bordered().merge_borders(MergeStrategy::Exact))
            .render(rhs_area, buf, &mut self.rhs_state);
    }
}

fn compare_diff_hunks(lhs: &[u8], rhs: &[u8]) -> Vec<(usize, usize)> {
    let mut hunks: Vec<(usize, usize)> = vec![];
    let shared_limit = lhs.len().min(rhs.len());

    for i in 0..shared_limit {
        if lhs[i] == rhs[i] {
            continue;
        }

        if let Some(last) = hunks.last_mut() {
            if last.0 + last.1 <= i && last.0 < i {
                last.1 = i - last.0;
            }
        } else {
            hunks.push((i, 1));
        }
    }

    for i in shared_limit..lhs.len() {
        if let Some(last) = hunks.last_mut() {
            if last.0 + last.1 <= i && last.0 < i {
                last.1 = i - last.0;
            }
        } else {
            hunks.push((i, 1));
        }
    }

    for i in shared_limit..rhs.len() {
        if let Some(last) = hunks.last_mut() {
            if last.0 + last.1 <= i && last.0 < i {
                last.1 = i - last.0;
            }
        } else {
            hunks.push((i, 1));
        }
    }
    hunks
}
