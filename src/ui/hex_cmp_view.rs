use std::{path::PathBuf, str::FromStr};

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    prelude::{Buffer, Rect},
    style::Style,
    symbols::merge::MergeStrategy,
    widgets::{Block, BorderType, StatefulWidget, Widget as _},
};
use ratatui_textarea::TextArea;
use regex::bytes::Regex;
use uuid::Uuid;

use crate::{
    DiffTuiError,
    ui::{
        self, EventHandler, Notification, Popup, TabState,
        hex_view::{HexView, HexViewState, HighlightGroup},
        menu::Menu,
        text_cmp_view::TextCmpView,
        tui,
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
                    vec![
                        ("Reopen with text cmp view".to_string(), Some('t')),
                        ("Search for guid".to_string(), Some('g')),
                        ("Search for bytes".to_string(), Some('b')),
                    ],
                )))));
            }
            Action::PopupReturn(id, Some(item)) if id == "HexCmpView action" => {
                match item.as_str() {
                    "Reopen with text cmp view" => {
                        return Ok(Some(Action::CreateTabAndSwitch(Box::new(
                            TextCmpView::new(self.lhs_path.clone(), self.rhs_path.clone())?,
                        ))));
                    }
                    "Search for guid" => {
                        return Ok(Some(Action::ShowPopup(Box::new(GuidInput::default()))));
                    }
                    "Search for bytes" => {
                        return Ok(Some(Action::ShowPopup(Box::new(BytesInput::default()))));
                    }
                    _ => {}
                }
            }
            Action::PopupReturn(id, Some(item)) if id == "GuidInput" => {
                let mut parsed: Option<Uuid> = None;
                if let Ok(uuid) = Uuid::try_parse(item) {
                    parsed = Some(uuid);
                } else if let Some(uuid) = parse_c_format_guid(item) {
                    parsed = Some(uuid);
                }
                if let Some(uuid) = parsed {
                    let bytes = uuid.to_bytes_le();
                    let mut search_str = String::new();

                    for b in bytes {
                        search_str.push_str(format!("\\x{b:02x}").as_str());
                    }

                    return Ok(Some(Action::EditSearch(Some(search_str))));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Guid search".to_string(),
                        body: "Not a valid GUID".to_string(),
                    })));
                }
            }
            Action::PopupReturn(id, Some(item)) if id == "BytesInput" => {
                if let Some(bytes) = parse_byte_string(item) {
                    let mut search_str = String::new();
                    for b in bytes {
                        search_str.push_str(format!("\\x{b:02x}").as_str());
                    }
                    return Ok(Some(Action::EditSearch(Some(search_str))));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Byte string search".to_string(),
                        body: "Not a valid byte string".to_string(),
                    })));
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
        )?)))
    }
}

#[derive(Debug)]
pub struct GuidInput<'a> {
    ta: TextArea<'a>,
}

impl<'a> Default for GuidInput<'a> {
    fn default() -> Self {
        let mut ta = TextArea::default();
        ta.set_block(
            Block::bordered()
                .title("Helper for guid search")
                .title_bottom("Press enter twice when completed")
                .border_style(Style::default().green())
                .border_type(BorderType::Rounded),
        );
        Self { ta }
    }
}

impl<'a> Popup for GuidInput<'a> {
    fn handler(&mut self, event: &ui::tui::Event) -> Option<Action> {
        if let tui::Event::Key(key_evt) = event {
            if key_evt.code == KeyCode::Enter {
                return Some(Action::PopupReturn(
                    "GuidInput".to_string(),
                    Some(self.ta.lines()[0].clone()),
                ));
            } else if key_evt.code == KeyCode::Esc {
                return Some(Action::PopupReturn("GuidInput".to_string(), None));
            } else {
                self.ta.input(*key_evt);
            }
        }
        None
    }

    fn render(&mut self, frame: &mut ratatui::prelude::Frame) {
        let (area, buf) = self.prepare(frame, Constraint::Max(100), Constraint::Length(3));
        self.ta.render(area, buf);
    }
}

#[derive(Debug)]
pub struct BytesInput<'a> {
    ta: TextArea<'a>,
}

impl<'a> Default for BytesInput<'a> {
    fn default() -> Self {
        let mut ta = TextArea::default();
        ta.set_wrap_mode(ratatui_textarea::WrapMode::Glyph);
        ta.set_block(
            Block::bordered()
                .title("Helper for byte string search")
                .title_bottom("Press enter twice when completed")
                .border_style(Style::default().green())
                .border_type(BorderType::Rounded),
        );
        Self { ta }
    }
}

impl<'a> Popup for BytesInput<'a> {
    fn handler(&mut self, event: &ui::tui::Event) -> Option<Action> {
        if let tui::Event::Key(key_evt) = event {
            if key_evt.code == KeyCode::Enter {
                return Some(Action::PopupReturn(
                    "BytesInput".to_string(),
                    Some(self.ta.lines()[0].clone()),
                ));
            } else if key_evt.code == KeyCode::Esc {
                return Some(Action::PopupReturn("BytesInput".to_string(), None));
            } else {
                self.ta.input(*key_evt);
            }
        }
        None
    }

    fn render(&mut self, frame: &mut ratatui::prelude::Frame) {
        let (area, buf) = self.prepare(
            frame,
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        );
        self.ta.render(area, buf);
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

fn parse_c_format_guid(s: &str) -> Option<Uuid> {
    let mut components = s
        .split(',')
        .map(|s| s.trim_matches(['{', '}', ' ']).trim_start_matches("0x"));
    let p1 = u32::from_str_radix(components.next()?, 16).ok()?;
    let p2 = u16::from_str_radix(components.next()?, 16).ok()?;
    let p3 = u16::from_str_radix(components.next()?, 16).ok()?;
    let p4: Vec<u8> = components
        .map(|s| u8::from_str_radix(s, 16))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    Some(Uuid::from_fields(
        p1,
        p2,
        p3,
        p4.as_slice().try_into().ok()?,
    ))
}

fn parse_byte_string(s: &str) -> Option<Vec<u8>> {
    let mut bytes: Vec<u8> = vec![];
    for mut s in s.split([',', ' ']).map(|s| s.trim_start_matches("0x")) {
        if s.len() % 2 != 0 {
            bytes.push(u8::from_str_radix(&s[0..1], 16).ok()?);
            s = &s[1..];
        }

        for idx in (0..s.len()).step_by(2) {
            bytes.push(u8::from_str_radix(&s[idx..idx + 2], 16).ok()?);
        }
    }

    Some(bytes)
}

#[cfg(test)]
mod test {
    use crate::ui::hex_cmp_view::{parse_byte_string, parse_c_format_guid};

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
