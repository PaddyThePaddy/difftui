use std::path::{Path, PathBuf};

use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    prelude::{Buffer, Rect},
    style::Style,
    symbols::merge::MergeStrategy,
    widgets::{Block, StatefulWidget},
};
use regex::bytes::Regex;

use crate::{
    DiffTuiConfig, DiffTuiError,
    ui::{
        EventHandler, GotoMenu, JumpToPopup, Notification, TabState,
        folder_cmp_view::FolderCmpState,
        hex_view::{HexSearchHelper, HexView, HexViewState, HighlightGroup, get_search_hl},
        menu::Menu,
        text_cmp_view::TextCmpView,
    },
};

use super::Action;

#[derive(Debug)]
pub struct HexCmpView {
    title: String,
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
    config: DiffTuiConfig,
}

impl HexCmpView {
    pub fn new(lhs: PathBuf, rhs: PathBuf, config: &DiffTuiConfig) -> Result<Self, DiffTuiError> {
        let lhs_buf = std::fs::read(&lhs)?;
        let rhs_buf = std::fs::read(&rhs)?;

        let diff_hl = compare_diff_hunks(&lhs_buf, &rhs_buf);

        Ok(Self {
            title: build_title(lhs.as_path(), rhs.as_path()).unwrap_or("HEX".to_string()),
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
            config: config.clone(),
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
            Action::Goto => {
                return Ok(Some(Action::ShowPopup(Box::new(GotoMenu::default()))));
            }
            Action::PopupReturn(id, Some(action)) if id == GotoMenu::ID => match action.as_str() {
                GotoMenu::TOP => {
                    self.lhs_state.set_selected(Some(0));
                    self.rhs_state.set_selected(Some(0));
                }
                GotoMenu::BOTTOM => {
                    self.lhs_state.set_selected(Some(usize::MAX));
                    self.rhs_state.set_selected(Some(usize::MAX));
                }
                GotoMenu::JUMP => {
                    return Ok(Some(Action::ShowPopup(Box::new(JumpToPopup::default()))));
                }
                _ => {}
            },
            Action::TabCustomAction => {
                let mut opts = vec![
                    ("Reopen with text cmp view".to_string(), Some('t')),
                    ("Hex search helper".to_string(), Some('h')),
                    ("Jump to offset".to_string(), Some(':')),
                ];

                if self
                    .lhs_path
                    .parent()
                    .zip(self.rhs_path.parent())
                    .is_some_and(|(l, r)| l.exists() && r.exists() && l.is_dir() && r.is_dir())
                {
                    opts.push((
                        "Open parent folder in folder cmp view".to_string(),
                        Some('p'),
                    ));
                }

                return Ok(Some(Action::ShowPopup(Box::new(Menu::new(
                    "HexCmpView action".to_string(),
                    opts,
                )))));
            }
            Action::PopupReturn(id, Some(item)) if id == "HexCmpView action" => {
                match item.as_str() {
                    "Reopen with text cmp view" => {
                        return Ok(Some(Action::CreateTabAndSwitch(Box::new(
                            TextCmpView::new(
                                self.lhs_path.clone(),
                                self.rhs_path.clone(),
                                &self.config,
                            )?,
                        ))));
                    }
                    "Hex search helper" => {
                        return Ok(Some(Action::ShowPopup(Box::new(
                            HexSearchHelper::default().auto_select(true),
                        ))));
                    }
                    "Open parent folder in folder cmp view" => {
                        if let Some((lhs, rhs)) = self.lhs_path.parent().zip(self.rhs_path.parent())
                        {
                            return Ok(Some(Action::CreateTabAndSwitch(Box::new(
                                FolderCmpState::new(lhs, rhs, &self.config)?,
                            ))));
                        }
                    }
                    _ => {}
                }
            }
            Action::PopupReturn(id, Some(item)) if id == "JumpTo" => {
                let item = item.trim();

                if let Some(item) = item.strip_prefix("0x") {
                    match usize::from_str_radix(item, 16) {
                        Ok(i) => {
                            self.lhs_state.set_selected(Some(i));
                            self.rhs_state.set_selected(Some(i));
                            return Ok(None);
                        }
                        Err(e) => {
                            return Ok(Some(Action::Notification(Notification {
                                title: "Parse index failed".to_string(),
                                body: format!("{e}"),
                            })));
                        }
                    }
                }
                match usize::from_str_radix(item, 10) {
                    Ok(i) => {
                        self.lhs_state.set_selected(Some(i));
                        self.rhs_state.set_selected(Some(i));
                    }
                    Err(e) => {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Parse index failed".to_string(),
                            body: format!("{e}"),
                        })));
                    }
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
        self.title.clone()
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

        StatefulWidget::render(
            HexView::new(&self.lhs_buf)
                .set_hl_groups(self.lhs_cached_hl.as_ref().map(|v| v.as_slice()))
                .block(Block::bordered().merge_borders(MergeStrategy::Exact)),
            lhs_area,
            buf,
            &mut self.lhs_state,
        );
        StatefulWidget::render(
            HexView::new(&self.rhs_buf)
                .set_hl_groups(self.rhs_cached_hl.as_ref().map(|v| v.as_slice()))
                .block(Block::bordered().merge_borders(MergeStrategy::Exact)),
            rhs_area,
            buf,
            &mut self.rhs_state,
        );
    }

    fn reload(&mut self) -> Result<Option<Box<dyn TabState>>, DiffTuiError> {
        Ok(Some(Box::new(HexCmpView::new(
            self.lhs_path.clone(),
            self.rhs_path.clone(),
            &self.config,
        )?)))
    }
}

fn build_title(lhs: &Path, rhs: &Path) -> Option<String> {
    let mut title: Option<String> = None;
    if let Some((lhs_base, rhs_base)) = lhs.file_name().zip(rhs.file_name()) {
        let lhs_base = lhs_base.to_string_lossy();
        let rhs_base = rhs_base.to_string_lossy();
        if lhs_base == rhs_base {
            let lhs_parent = lhs.parent().and_then(|p| p.file_name());
            let rhs_parent = rhs.parent().and_then(|p| p.file_name());
            if lhs_parent == rhs_parent {
                for (lhs_comp, rhs_comp) in lhs.components().rev().zip(rhs.components().rev()) {
                    if lhs_comp != rhs_comp {
                        let lhs_comp = lhs_comp.as_os_str().to_string_lossy();
                        let rhs_comp = rhs_comp.as_os_str().to_string_lossy();
                        title = Some(format!("{}<=>{}/../{}:X", lhs_comp, rhs_comp, lhs_base));
                        break;
                    }
                }
                if title.is_none() {
                    title = Some(format!("{}:X", lhs_base));
                }
            } else {
                title = Some(format!(
                    "{}<=>{}/{}:X",
                    lhs_parent.map(|s| s.to_str()).flatten().unwrap_or("\"\""),
                    rhs_parent.map(|s| s.to_str()).flatten().unwrap_or("\"\""),
                    lhs_base
                ));
            }
        } else {
            title = Some(format!("{}<=>{}:X", lhs_base, rhs_base));
        }
    }
    title
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
            } else {
                hunks.push((i, 1, style).into());
            }
        } else {
            hunks.push((i, 1, style).into());
        }
    }

    for i in shared_limit..lhs.len() {
        if let Some(last) = hunks.last_mut() {
            if last.end() == i {
                last.extend_to_include(i);
            } else {
                hunks.push((i, 1, style).into());
            }
        } else {
            hunks.push((i, 1, style).into());
        }
    }

    for i in shared_limit..rhs.len() {
        if let Some(last) = hunks.last_mut() {
            if last.end() == i {
                last.extend_to_include(i);
            } else {
                hunks.push((i, 1, style).into());
            }
        } else {
            hunks.push((i, 1, style).into());
        }
    }
    hunks
}

#[cfg(test)]
mod test {
    use crate::ui::hex_view::{parse_byte_string, parse_c_format_guid};

    #[test]
    fn test_c_format_guid_parsing() {
        assert_eq!(
            parse_c_format_guid(
                "{ 0x1FBD2960, 0x4130, 0x41E5, {0x94, 0xAC, 0xD2, 0xCF, 0x03, 0x7F, 0xB3, 0x7C }}"
            ),
            Some(uuid::uuid!("1fbd2960-4130-41e5-94ac-d2cf037fb37c"))
        );
    }

    #[test]
    fn test_parse_byte_string() {
        assert_eq!(
            parse_byte_string("aabbccdd"),
            Some(vec![0xaa, 0xbb, 0xcc, 0xdd])
        );
        assert_eq!(
            parse_byte_string("1aabbccdd"),
            Some(vec![0x01, 0xaa, 0xbb, 0xcc, 0xdd])
        );
        assert_eq!(
            parse_byte_string("1, aa, bb, cc, dd"),
            Some(vec![0x01, 0xaa, 0xbb, 0xcc, 0xdd])
        );
        assert_eq!(
            parse_byte_string("1, 0xaabb, cc, 0xdd"),
            Some(vec![0x01, 0xaa, 0xbb, 0xcc, 0xdd])
        );
    }
}
