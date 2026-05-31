use std::path::PathBuf;

use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    prelude::{Buffer, Rect},
    style::Style,
    symbols::merge::MergeStrategy,
    widgets::{Block, StatefulWidget},
};
use regex::bytes::Regex;

use crate::{
    DiffTuiError,
    ui::{
        EventHandler, Notification, TabState,
        hex_view::{HexView, HexViewState, HighlightGroup},
        menu::Menu,
        text_cmp_view::TextCmpView,
    },
};

use super::Action;

#[derive(Debug)]
pub struct HexCmpView {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    lhs_buf: Vec<u8>,
    rhs_buf: Vec<u8>,
    diff_hl: Vec<HighlightGroup>,
    lhs_state: HexViewState,
    rhs_state: HexViewState,
    search_hl: Option<Regex>,
    lhs_search_hl: Vec<HighlightGroup>,
    rhs_search_hl: Vec<HighlightGroup>,
    lhs_cached_hl: Option<Vec<HighlightGroup>>,
    rhs_cached_hl: Option<Vec<HighlightGroup>>,
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
            search_hl: None,
            lhs_search_hl: vec![],
            rhs_search_hl: vec![],
            lhs_cached_hl: None,
            rhs_cached_hl: None,
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

                if let Some(next_hunk) = self.diff_hl.iter().find(|h| h.start > selected) {
                    self.lhs_state.set_selected(Some(next_hunk.start));
                    self.rhs_state.set_selected(Some(next_hunk.start));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Next diff".to_string(),
                        body: "Reached last diff".to_string(),
                    })));
                }
            }
            Action::PrevDiff => {
                let selected = self.lhs_state.selected().unwrap_or(0);

                if let Some(prev_hunk) = self.diff_hl.iter().rev().find(|h| h.end() < selected) {
                    self.lhs_state.set_selected(Some(prev_hunk.start));
                    self.rhs_state.set_selected(Some(prev_hunk.start));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Previous diff".to_string(),
                        body: "Reached first diff".to_string(),
                    })));
                }
            }
            Action::SearchNext(r) => {
                if !self
                    .search_hl
                    .as_ref()
                    .is_some_and(|hl| hl.as_str() == r.as_str())
                {
                    self.search_hl = Some(r.clone());
                    self.lhs_search_hl = get_search_hl(&self.lhs_buf, &r);
                    self.rhs_search_hl = get_search_hl(&self.rhs_buf, &r);
                    self.lhs_cached_hl = None;
                    self.rhs_cached_hl = None;
                }

                let current = self.lhs_state.selected().unwrap_or(0);
                let mut jump_to = None;
                if let Some(hl) = self.lhs_search_hl.iter().find(|hl| hl.start > current) {
                    jump_to = Some(hl.start);
                }
                if let Some(m) = self.rhs_search_hl.iter().find(|hl| hl.start > current) {
                    if jump_to.is_none() || jump_to.is_some_and(|n| m.start < n) {
                        jump_to = Some(m.start);
                    }
                }
                if jump_to.is_none() && current != 0 {
                    if let Some(m) = self.lhs_search_hl.iter().find(|hl| hl.end() <= current) {
                        jump_to = Some(m.start);
                    }
                    if let Some(m) = self.rhs_search_hl.iter().find(|hl| hl.end() <= current) {
                        if jump_to.is_none() || jump_to.is_some_and(|n| n > m.start) {
                            jump_to = Some(m.start);
                        }
                    }
                }
                if let Some(jump_to) = jump_to {
                    self.lhs_state.set_selected(Some(jump_to));
                    self.rhs_state.set_selected(Some(jump_to));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Search".to_string(),
                        body: "No matches found".to_string(),
                    })));
                }
            }
            Action::SearchPrev(r) => {
                if !self
                    .search_hl
                    .as_ref()
                    .is_some_and(|hl| hl.as_str() == r.as_str())
                {
                    self.search_hl = Some(r.clone());
                    self.lhs_search_hl = get_search_hl(&self.lhs_buf, &r);
                    self.rhs_search_hl = get_search_hl(&self.rhs_buf, &r);
                    self.lhs_cached_hl = None;
                    self.rhs_cached_hl = None;
                }

                let current = self.lhs_state.selected().unwrap_or(0);
                let mut jump_to = None;
                if let Some(m) = self
                    .lhs_search_hl
                    .iter()
                    .filter(|hl| hl.end() <= current)
                    .last()
                {
                    jump_to = Some(m.start);
                }
                if let Some(m) = self
                    .rhs_search_hl
                    .iter()
                    .filter(|hl| hl.end() <= current)
                    .last()
                {
                    if jump_to.is_none() || jump_to.is_some_and(|n| m.start > n) {
                        jump_to = Some(m.start);
                    }
                }
                if jump_to.is_none() && current != 0 {
                    if let Some(m) = self
                        .lhs_search_hl
                        .iter()
                        .filter(|hl| hl.start > current)
                        .last()
                    {
                        jump_to = Some(m.start);
                    }
                    if let Some(m) = self
                        .rhs_search_hl
                        .iter()
                        .filter(|hl| hl.start > current)
                        .last()
                    {
                        if jump_to.is_none() || jump_to.is_some_and(|n| m.start > n) {
                            jump_to = Some(m.start);
                        }
                    }
                }
                if let Some(jump_to) = jump_to {
                    self.lhs_state.set_selected(Some(jump_to));
                    self.rhs_state.set_selected(Some(jump_to));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Search".to_string(),
                        body: "No matches found".to_string(),
                    })));
                }
            }
            Action::RemoveHighlight => {
                self.search_hl = None;
                self.lhs_search_hl.clear();
                self.rhs_search_hl.clear();
                self.lhs_cached_hl = None;
                self.rhs_cached_hl = None;
            }
            Action::NavTop => {
                self.lhs_state.set_selected(Some(0));
                self.rhs_state.set_selected(Some(0));
            }
            Action::NavBottom => {
                self.lhs_state.set_selected(Some(usize::MAX));
                self.rhs_state.set_selected(Some(usize::MAX));
            }
            Action::TabCustomAction => {
                return Ok(Some(Action::ShowPopup(Box::new(Menu::new(
                    "HexCmpView action".to_string(),
                    vec![("Reopen with text cmp view".to_string(), Some('t'))],
                )))));
            }
            Action::PopupReturn(id, Some(item)) if id == "HexCmpView action" => {
                match item.as_str() {
                    "Reopen with text cmp view" => {
                        return Ok(Some(Action::CreateTabAndSwitch(Box::new(
                            TextCmpView::new(self.lhs_path.clone(), self.rhs_path.clone())?,
                        ))));
                    }
                    _ => {}
                }
            }
            Action::SwapSide => {
                std::mem::swap(&mut self.lhs_buf, &mut self.rhs_buf);
                std::mem::swap(&mut self.lhs_path, &mut self.rhs_path);
                std::mem::swap(&mut self.lhs_search_hl, &mut self.rhs_search_hl);
                std::mem::swap(&mut self.lhs_cached_hl, &mut self.rhs_cached_hl);
                std::mem::swap(&mut self.lhs_state, &mut self.rhs_state);
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

        if self.lhs_cached_hl.is_none() {
            let mut lhs_hl = self.lhs_search_hl.clone();
            lhs_hl.extend(&self.diff_hl);
            self.lhs_cached_hl = Some(lhs_hl);
        }
        if self.rhs_cached_hl.is_none() {
            let mut rhs_hl = self.rhs_search_hl.clone();
            rhs_hl.extend(&self.diff_hl);
            self.rhs_cached_hl = Some(rhs_hl);
        }

        HexView::new(&self.lhs_buf)
            .set_hl_groups(self.lhs_cached_hl.as_ref().map(|v| v.as_slice()))
            .block(Block::bordered().merge_borders(MergeStrategy::Exact))
            .render(lhs_area, buf, &mut self.lhs_state);
        HexView::new(&self.rhs_buf)
            .set_hl_groups(self.rhs_cached_hl.as_ref().map(|v| v.as_slice()))
            .block(Block::bordered().merge_borders(MergeStrategy::Exact))
            .render(rhs_area, buf, &mut self.rhs_state);
    }

    fn reload(&mut self) -> Result<Option<Box<dyn TabState>>, DiffTuiError> {
        Ok(Some(Box::new(HexCmpView::new(
            self.lhs_path.clone(),
            self.rhs_path.clone(),
        )?)))
    }
}

fn compare_diff_hunks(lhs: &[u8], rhs: &[u8]) -> Vec<HighlightGroup> {
    let mut hunks: Vec<HighlightGroup> = vec![];
    let shared_limit = lhs.len().min(rhs.len());
    let style = Style::default().on_red().not_dim();

    for i in 0..shared_limit {
        if lhs[i] == rhs[i] {
            continue;
        }

        if let Some(last) = hunks.last_mut() {
            if last.end() == i {
                last.extend_to_include(i);
            }
        } else {
            hunks.push((i, 1, style).into());
        }
    }

    for i in shared_limit..lhs.len() {
        if let Some(last) = hunks.last_mut() {
            if last.end() == i {
                last.extend_to_include(i);
            }
        } else {
            hunks.push((i, 1, style).into());
        }
    }

    for i in shared_limit..rhs.len() {
        if let Some(last) = hunks.last_mut() {
            if last.end() == i {
                last.extend_to_include(i);
            }
        } else {
            hunks.push((i, 1, style).into());
        }
    }
    hunks
}

fn get_search_hl(buf: &[u8], re: &Regex) -> Vec<HighlightGroup> {
    let mut output: Vec<HighlightGroup> = vec![];
    let style = Style::default().on_yellow();
    for m in re.find_iter(buf) {
        output.push((m.start(), m.len(), style).into());
    }
    output
}
