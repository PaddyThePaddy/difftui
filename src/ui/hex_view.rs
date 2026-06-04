use std::path::PathBuf;

use crossterm::event::KeyCode;
use ratatui::{
    layout::{Constraint, Direction, Layout, Offset, Position},
    prelude::{Buffer, Rect},
    style::Style,
    text::Span,
    widgets::{
        Block, BorderType, List, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, StatefulWidget, Widget,
    },
};
use ratatui_textarea::{TextArea, WrapMode};
use regex::bytes::{Regex, RegexBuilder};
use strum::IntoEnumIterator;
use uuid::Uuid;

use crate::{
    DiffTuiError,
    ui::{EventHandler, GotoMenu, JumpToPopup, Notification, Popup, TabState, menu::Menu, tui},
};

use super::Action;

const DIV_EVERY_BYTES: usize = 8;

#[derive(Debug, Clone)]
pub struct HexViewTab {
    file: PathBuf,
    buf: Vec<u8>,
    state: HexViewState,
    search_hl: Option<Regex>,
    cached_hl: Vec<HighlightGroup>,
}

impl HexViewTab {
    pub fn new(p: PathBuf) -> Result<Self, DiffTuiError> {
        let buf = std::fs::read(&p)?;
        Ok(Self {
            file: p,
            buf,
            state: HexViewState::default(),
            search_hl: None,
            cached_hl: vec![],
        })
    }
}

impl EventHandler for HexViewTab {
    fn handler(&mut self, event: &Action) -> Result<Option<Action>, DiffTuiError> {
        match event {
            Action::NavUp => {
                self.state.move_sel_up();
            }
            Action::NavDown => {
                self.state.move_sel_down();
            }
            Action::NavLeft => {
                self.state.move_sel_left();
            }
            Action::NavRight => {
                self.state.move_sel_right();
            }
            Action::PageUp(fac) => {
                self.state.move_sel_up_page(*fac);
            }
            Action::PageDown(fac) => {
                self.state.move_sel_down_page(*fac);
            }
            Action::SearchNext(r) => {
                if !self
                    .search_hl
                    .as_ref()
                    .is_some_and(|hl| hl.as_str() == r.as_str())
                {
                    self.search_hl = Some(r.clone());
                    self.cached_hl = get_search_hl(&self.buf, &r);
                }

                let current = self.state.selected().unwrap_or(0);
                let mut jump_to = None;
                if let Some(hl) = self.cached_hl.iter().find(|hl| hl.start > current) {
                    jump_to = Some(hl.start);
                }
                if let Some(m) = self.cached_hl.iter().find(|hl| hl.start > current) {
                    if jump_to.is_none() || jump_to.is_some_and(|n| m.start < n) {
                        jump_to = Some(m.start);
                    }
                }
                if jump_to.is_none() && current != 0 {
                    if let Some(m) = self.cached_hl.iter().find(|hl| hl.end() <= current) {
                        jump_to = Some(m.start);
                    }
                    if let Some(m) = self.cached_hl.iter().find(|hl| hl.end() <= current) {
                        if jump_to.is_none() || jump_to.is_some_and(|n| n > m.start) {
                            jump_to = Some(m.start);
                        }
                    }
                }
                if let Some(jump_to) = jump_to {
                    self.state.set_selected(Some(jump_to));
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
                    self.cached_hl = get_search_hl(&self.buf, &r);
                }

                let current = self.state.selected().unwrap_or(0);
                let mut jump_to = None;
                if let Some(m) = self
                    .cached_hl
                    .iter()
                    .filter(|hl| hl.end() <= current)
                    .last()
                {
                    jump_to = Some(m.start);
                }
                if let Some(m) = self
                    .cached_hl
                    .iter()
                    .filter(|hl| hl.end() <= current)
                    .last()
                {
                    if jump_to.is_none() || jump_to.is_some_and(|n| m.start > n) {
                        jump_to = Some(m.start);
                    }
                }
                if jump_to.is_none() && current != 0 {
                    if let Some(m) = self.cached_hl.iter().filter(|hl| hl.start > current).last() {
                        jump_to = Some(m.start);
                    }
                    if let Some(m) = self.cached_hl.iter().filter(|hl| hl.start > current).last() {
                        if jump_to.is_none() || jump_to.is_some_and(|n| m.start > n) {
                            jump_to = Some(m.start);
                        }
                    }
                }
                if let Some(jump_to) = jump_to {
                    self.state.set_selected(Some(jump_to));
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Search".to_string(),
                        body: "No matches found".to_string(),
                    })));
                }
            }
            Action::RemoveHighlight => {
                self.search_hl = None;
                self.cached_hl.clear();
            }
            Action::Goto => {
                return Ok(Some(Action::ShowPopup(Box::new(GotoMenu::default()))));
            }
            Action::PopupReturn(id, Some(action)) if id == GotoMenu::ID => match action.as_str() {
                GotoMenu::TOP => {
                    self.state.set_selected(Some(0));
                }
                GotoMenu::BOTTOM => {
                    self.state.set_selected(Some(usize::MAX));
                }
                GotoMenu::JUMP => {
                    return Ok(Some(Action::ShowPopup(Box::new(JumpToPopup::default()))));
                }
                _ => {}
            },
            Action::TabCustomAction => {
                let opts = vec![
                    ("Hex search helper".to_string(), Some('h')),
                    ("Jump to offset".to_string(), Some(':')),
                ];
                return Ok(Some(Action::ShowPopup(Box::new(Menu::new(
                    "HexView action".to_string(),
                    opts,
                )))));
            }
            Action::PopupReturn(id, Some(item)) if id == "HexView action" => match item.as_str() {
                "Hex search helper" => {
                    return Ok(Some(Action::ShowPopup(Box::new(
                        HexSearchHelper::default().auto_select(true),
                    ))));
                }
                "Jump to offset" => {
                    return Ok(Some(Action::ShowPopup(Box::new(JumpToPopup::default()))));
                }
                _ => {}
            },
            Action::PopupReturn(id, Some(item)) if id == "JumpTo" => {
                let item = item.trim();

                if let Some(item) = item.strip_prefix("0x") {
                    match usize::from_str_radix(item, 16) {
                        Ok(i) => {
                            self.state.set_selected(Some(i));
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
                        self.state.set_selected(Some(i));
                    }
                    Err(e) => {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Parse index failed".to_string(),
                            body: format!("{e}"),
                        })));
                    }
                }
            }
            _ => {}
        }
        return Ok(None);
    }
}

impl TabState for HexViewTab {
    fn title(&self) -> String {
        self.file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(format!("Hex"))
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        StatefulWidget::render(
            HexView::new(&self.buf).set_hl_groups(Some(self.cached_hl.as_slice())),
            area,
            buf,
            &mut self.state,
        );
    }

    fn reload(&mut self) -> Result<Option<Box<dyn TabState>>, DiffTuiError> {
        Ok(Some(Box::new(HexViewTab::new(self.file.clone())?)))
    }
}

pub fn get_search_hl(buf: &[u8], re: &Regex) -> Vec<HighlightGroup> {
    let mut output: Vec<HighlightGroup> = vec![];
    let style = Style::default().on_yellow();
    for m in re.find_iter(buf) {
        output.push((m.start(), m.len(), style).into());
    }
    output
}

pub fn parse_c_format_guid(s: &str) -> Option<Uuid> {
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

pub fn parse_byte_string(s: &str) -> Option<Vec<u8>> {
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

#[derive(Debug, Default, Clone)]
pub struct HexViewState {
    offset: usize,
    selected: Option<usize>,
    bytes_in_line: usize,
    lines: usize,
}

impl HexViewState {
    pub fn set_offset(&mut self, offset: usize) {
        self.offset = offset;
    }

    pub fn move_offset_down_line(&mut self, line: usize) {
        self.offset = self.offset.saturating_add(line * self.bytes_in_line);
    }

    pub fn move_offset_up_line(&mut self, line: usize) {
        self.offset = self.offset.saturating_sub(line * self.bytes_in_line);
    }

    pub fn set_selected(&mut self, selected: Option<usize>) {
        self.selected = selected;
    }

    pub fn with_selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    pub fn move_sel_left(&mut self) {
        let sel = self.selected.unwrap_or(0).saturating_sub(1);
        self.selected = Some(sel);
    }

    pub fn move_sel_right(&mut self) {
        let sel = self.selected.unwrap_or(0).saturating_add(1);
        self.selected = Some(sel);
    }

    pub fn move_sel_down(&mut self) {
        let sel = self
            .selected
            .unwrap_or(0)
            .saturating_add(self.bytes_in_line);
        self.selected = Some(sel);
    }

    pub fn move_sel_up(&mut self) {
        let sel = self
            .selected
            .unwrap_or(0)
            .saturating_sub(self.bytes_in_line);
        self.selected = Some(sel);
    }

    pub fn move_sel_down_page(&mut self, factor: f32) {
        let sel = self
            .selected
            .unwrap_or(0)
            .saturating_add((((self.lines * self.bytes_in_line) as f32) * factor).floor() as usize);
        self.selected = Some(sel);
    }

    pub fn move_sel_up_page(&mut self, factor: f32) {
        let sel = self
            .selected
            .unwrap_or(0)
            .saturating_sub((((self.lines * self.bytes_in_line) as f32) * factor).floor() as usize);
        self.selected = Some(sel);
    }

    fn move_frame_to_include(&mut self, idx: usize) {
        while idx < self.offset {
            self.offset = self.offset.saturating_sub(self.bytes_in_line);
        }
        while idx >= self.offset + self.bytes_in_line * self.lines {
            self.offset = self.offset.saturating_add(self.bytes_in_line);
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HexViewStyle {
    pub default: Style,
    pub zero: Style,
    pub ff: Style,
}

impl Default for HexViewStyle {
    fn default() -> Self {
        Self {
            default: Style::default(),
            zero: Style::default().dim(),
            ff: Style::default().gray(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum HexViewMode {
    /// Displays less than 16 bytes a line
    Partial(usize),
    /// Displays all 16 bytes in a single line
    FullHex,
    /// Displays all 16 bytes plus ascii decoding
    FullHexWithAscii,
    /// Buffer to small to display the buffer
    BufferTooSmall,
}

impl HexViewMode {
    pub fn required_width(&self, max_line_number: Option<usize>) -> usize {
        match self {
            HexViewMode::Partial(bytes) => {
                // 00001234 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00
                // ^^^^^^^^ line number
                //         ^^ divider
                //           ^^^ a byte and its left padding space take 3 characters
                //                                   ^^ divider every 8 bytes
                max_line_number.map(|ln| line_number_width(ln)).unwrap_or(8)
                    + 2
                    + 3 * bytes
                    + 2 * self.mid_dividers()
            }
            HexViewMode::FullHex => {
                // 00001234 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00
                HexViewMode::Partial(16).required_width(max_line_number)
            }
            HexViewMode::FullHexWithAscii => {
                // 00001234 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00 │ abcdefgh ........
                //                                                             ^^^^^^^^^^^^^^^^^^^^
                HexViewMode::FullHex.required_width(max_line_number) + 3 + self.ascii_area_width()
            }
            HexViewMode::BufferTooSmall => 0,
        }
    }

    pub fn detect_mode(buf: Option<&[u8]>, area: Rect) -> Self {
        let max_ln = buf.map(|b| b.len());
        let available_width = area.width;

        if (Self::FullHexWithAscii.required_width(max_ln) as u16) < available_width {
            return Self::FullHexWithAscii;
        } else if (Self::FullHex.required_width(max_ln) as u16) < available_width {
            return Self::FullHex;
        } else {
            for i in 1..=16 {
                let len = 16 - i;
                if (Self::Partial(len).required_width(max_ln) as u16) < available_width {
                    return Self::Partial(len);
                }
            }
            return Self::BufferTooSmall;
        }
    }

    pub fn bytes_in_line(&self) -> usize {
        match self {
            HexViewMode::Partial(n) => *n,
            HexViewMode::FullHex => 16,
            HexViewMode::FullHexWithAscii => 16,
            HexViewMode::BufferTooSmall => 0,
        }
    }

    pub fn has_ascii_section(&self) -> bool {
        match self {
            HexViewMode::FullHexWithAscii => true,
            _ => false,
        }
    }

    pub fn mid_dividers(&self) -> usize {
        match self {
            HexViewMode::FullHexWithAscii => HexViewMode::Partial(16).mid_dividers(),
            HexViewMode::FullHex => HexViewMode::Partial(16).mid_dividers(),
            HexViewMode::Partial(bytes) => bytes.saturating_sub(1) / DIV_EVERY_BYTES,
            HexViewMode::BufferTooSmall => 0,
        }
    }

    pub fn ascii_area_width(&self) -> usize {
        match self {
            HexViewMode::FullHexWithAscii => 16 + self.mid_dividers(),
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HighlightGroup {
    pub start: usize,
    pub len: usize,
    pub style: Style,
}

impl From<(usize, usize, Style)> for HighlightGroup {
    fn from(value: (usize, usize, Style)) -> Self {
        Self {
            start: value.0,
            len: value.1,
            style: value.2,
        }
    }
}

impl HighlightGroup {
    pub fn is_highlighted(&self, idx: usize) -> Option<Style> {
        if idx >= self.start && idx < self.start + self.len {
            Some(self.style)
        } else {
            None
        }
    }

    pub fn extend_to_include(&mut self, idx: usize) {
        if self.start > idx {
            self.start = idx;
        }

        if self.end() <= idx {
            self.len = idx - self.start + 1;
        }
    }

    pub fn end(&self) -> usize {
        self.start + self.len
    }
}

#[derive(Debug, Clone)]
pub struct HexView<'buf, 'blk, 'hl> {
    buf: &'buf [u8],
    style: HexViewStyle,
    block: Block<'blk>,
    has_scroll_bar: bool,
    hl_groups: Option<&'hl [HighlightGroup]>,
}

impl<'buf, 'blk, 'hl> HexView<'buf, 'blk, 'hl> {
    pub fn new(buf: &'buf [u8]) -> Self {
        Self {
            buf,
            style: HexViewStyle::default(),
            block: Block::bordered(),
            has_scroll_bar: true,
            hl_groups: None,
        }
    }

    pub fn style(mut self, style: HexViewStyle) -> Self {
        self.style = style;
        self
    }

    pub fn block(mut self, block: Block<'blk>) -> Self {
        self.block = block;
        self
    }

    pub fn scroll_bar(mut self, has_scroll_bar: bool) -> Self {
        self.has_scroll_bar = has_scroll_bar;
        self
    }

    fn is_hl(&self, idx: usize) -> Option<Style> {
        if let Some(hl_groups) = self.hl_groups {
            for group in hl_groups.iter() {
                if let Some(sty) = group.is_highlighted(idx) {
                    return Some(sty);
                }
            }
        }
        None
    }

    fn render_hex_area(
        &self,
        area: Rect,
        buf: &mut Buffer,
        line_start: usize,
        bytes_in_line: usize,
        selected: Option<usize>,
    ) {
        let mut position = Position::new(area.x, area.y);
        for i in 0..bytes_in_line {
            if let Some(cell) = buf.cell_mut(position) {
                cell.set_style(Style::default());
                cell.set_char(' ');
            }
            position = position.offset(Offset::new(1, 0));

            let byte_to_print = self.buf[line_start + i];
            let (hh, lh) = num_to_hex_chars(byte_to_print);
            let byte_style = {
                let mut byte_style = if byte_to_print == 0xFF {
                    self.style.ff
                } else if byte_to_print == 0 {
                    self.style.zero
                } else {
                    self.style.default
                };

                if let Some(hl) = self.is_hl(line_start + i) {
                    byte_style = hl;
                }

                if selected.is_some_and(|sel| sel == line_start + i) {
                    byte_style = byte_style.reversed();
                }

                byte_style
            };

            if let Some(cell) = buf.cell_mut(position) {
                cell.set_style(byte_style);
                cell.set_char(hh);
            }
            position = position.offset(Offset::new(1, 0));

            if let Some(cell) = buf.cell_mut(position) {
                cell.set_style(byte_style);
                cell.set_char(lh);
            }
            position = position.offset(Offset::new(1, 0));

            if (i + 1) % DIV_EVERY_BYTES == 0 && i + 1 < bytes_in_line {
                if let Some(cell) = buf.cell_mut(position) {
                    cell.set_style(Style::default());
                    cell.set_char(' ');
                }
                position = position.offset(Offset::new(1, 0));

                if let Some(cell) = buf.cell_mut(position) {
                    cell.set_style(Style::default());
                    cell.set_char('|');
                }
                position = position.offset(Offset::new(1, 0));
            }
        }
    }

    fn render_ascii_area(
        &self,
        area: Rect,
        buf: &mut Buffer,
        line_start: usize,
        bytes_in_line: usize,
        selected: Option<usize>,
    ) {
        let mut position = Position::new(area.x, area.y);
        for i in 0..bytes_in_line {
            let byte_to_print = self.buf[line_start + i];
            let mut style = if let Some(hl) = self.is_hl(line_start + i) {
                hl
            } else {
                Style::default()
            };
            if selected.is_some_and(|sel| sel == line_start + i) {
                style = style.reversed();
            }
            if let Some(cell) = buf.cell_mut(position) {
                cell.set_style(style);
                cell.set_char(byte_to_display_char(byte_to_print));
            }
            position = position.offset(Offset::new(1, 0));

            if (i + 1) % DIV_EVERY_BYTES == 0 {
                if let Some(cell) = buf.cell_mut(position) {
                    cell.set_style(style);
                    cell.set_char(' ');
                }
                position = position.offset(Offset::new(1, 0));
            }
        }
    }

    pub fn set_hl_groups(mut self, hl_groups: Option<&'hl [HighlightGroup]>) -> Self {
        self.hl_groups = hl_groups;
        self
    }
}

impl<'buf, 'blk, 'hl> Widget for HexView<'buf, 'blk, 'hl> {
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        StatefulWidget::render(self, area, buf, &mut HexViewState::default());
    }
}

impl<'buf, 'blk, 'hl> StatefulWidget for HexView<'buf, 'blk, 'hl> {
    type State = HexViewState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.selected.is_some_and(|sel| sel >= self.buf.len()) {
            state.selected = Some(self.buf.len().saturating_sub(1));
        }
        let block_inner = self.block.inner(area);
        self.block.clone().render(area, buf);
        let [main_area, _scrollbar_area] = block_inner.layout(&Layout::new(
            Direction::Horizontal,
            [
                Constraint::Fill(1),
                Constraint::Length(if self.has_scroll_bar { 1 } else { 0 }),
            ],
        ));
        let mode = HexViewMode::detect_mode(Some(self.buf), main_area);
        let ln_width = line_number_width(self.buf.len());
        let mut line_start = state.offset;
        let layout = Layout::new(
            Direction::Horizontal,
            [
                Constraint::Length(line_number_width(self.buf.len()) as u16),
                Constraint::Length(2),
                Constraint::Length((mode.bytes_in_line() * 3 + mode.mid_dividers() * 2) as u16),
                Constraint::Length(if mode.has_ascii_section() { 3 } else { 0 }),
                Constraint::Length(mode.ascii_area_width() as u16),
            ],
        );

        state.bytes_in_line = mode.bytes_in_line();
        state.lines = main_area.height as usize;
        if let Some(sel) = state.selected {
            state.move_frame_to_include(sel);
        }

        for row in main_area.rows() {
            let [ln_area, ln_div_area, hex_area, ascii_div_area, ascii_area] = row.layout(&layout);

            let ln_str = format!("{line_start:00$X}", ln_width);
            Span::from(ln_str).render(ln_area, buf);
            Span::from(" │").render(ln_div_area, buf);

            let bytes_in_line = mode
                .bytes_in_line()
                .min(self.buf.len().saturating_sub(line_start));

            self.render_hex_area(hex_area, buf, line_start, bytes_in_line, state.selected);

            if mode.has_ascii_section() {
                Span::from(" │").render(ascii_div_area, buf);
                self.render_ascii_area(ascii_area, buf, line_start, bytes_in_line, state.selected);
            }

            line_start += mode.bytes_in_line();
        }

        if self.has_scroll_bar {
            let mut scrollbar_state = ScrollbarState::new(self.buf.len())
                .position(state.selected.unwrap_or(state.offset));
            Scrollbar::new(ScrollbarOrientation::VerticalRight).render(
                block_inner,
                buf,
                &mut scrollbar_state,
            );
        }
    }
}

#[derive(Debug, Clone, Copy, Default, strum::IntoStaticStr, strum::EnumIter)]
pub enum HexHelperMode {
    #[default]
    ByteString,
    Guid,
    Utf16le,
}

#[derive(Debug, Clone)]
enum HexHelperState<'a> {
    SelectMode(TextArea<'a>, Option<Regex>, ListState),
    InputContent(HexHelperMode, TextArea<'a>),
}

impl<'a> Default for HexHelperState<'a> {
    fn default() -> Self {
        Self::SelectMode(
            HexSearchHelper::create_focus_text_area(),
            None,
            ListState::default().with_selected(Some(0)),
        )
    }
}

#[derive(Debug, Clone, Default)]
pub struct HexSearchHelper<'a> {
    state: HexHelperState<'a>,
    should_exit: bool,
    auto_select: bool,
}

impl<'a> HexSearchHelper<'a> {
    const ID: &'static str = "HexHelper";

    pub fn auto_select(mut self, val: bool) -> Self {
        self.auto_select = val;
        self
    }

    fn current_displayed_items(&self) -> Vec<HexHelperMode> {
        match &self.state {
            HexHelperState::SelectMode(_, re, _) => {
                if let Some(re) = re {
                    HexHelperMode::iter()
                        .filter(|m| re.is_match(Into::<&str>::into(m).as_bytes()))
                        .collect()
                } else {
                    HexHelperMode::iter().collect()
                }
            }
            HexHelperState::InputContent(_, _) => vec![],
        }
    }

    fn selected_mode(&self) -> HexHelperMode {
        match &self.state {
            HexHelperState::SelectMode(_, _, list) => self
                .current_displayed_items()
                .into_iter()
                .nth(list.selected().unwrap_or(0))
                .unwrap_or_default(),
            HexHelperState::InputContent(_, _) => HexHelperMode::default(),
        }
    }

    fn create_focus_text_area() -> TextArea<'a> {
        let mut text_area = TextArea::default();
        text_area.set_block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::default().blue()),
        );
        text_area.set_wrap_mode(WrapMode::Glyph);
        text_area
    }
}

impl<'a> Popup for HexSearchHelper<'a> {
    fn handler(&mut self, event: &tui::Event) -> Option<Action> {
        if self.should_exit {
            return Some(Action::PopupReturn(Self::ID.to_string(), None));
        }
        match &mut self.state {
            HexHelperState::SelectMode(text_area, regex, list) => {
                if let tui::Event::Key(key) = event {
                    match key.code {
                        KeyCode::Down => list.select_next(),
                        KeyCode::Up => list.select_previous(),
                        KeyCode::Enter => {
                            self.state = HexHelperState::InputContent(
                                self.selected_mode(),
                                Self::create_focus_text_area(),
                            );
                            return None;
                        }
                        KeyCode::Esc => {
                            return Some(Action::PopupReturn(Self::ID.to_string(), None));
                        }
                        _ => {
                            text_area.input(*key);
                        }
                    }
                    let text = text_area.lines().join("\n");
                    let new_re = RegexBuilder::new(text.as_str())
                        .case_insensitive(!text.chars().any(|c| c.is_uppercase()))
                        .build()
                        .ok();
                    *regex = new_re;
                    if self.auto_select {
                        let current_items = self.current_displayed_items();
                        if current_items.len() == 1 {
                            self.state = HexHelperState::InputContent(
                                self.selected_mode(),
                                Self::create_focus_text_area(),
                            );
                            return None;
                        }
                    }
                }
            }
            HexHelperState::InputContent(hex_helper_mode, text_area) => {
                if let tui::Event::Key(key) = event {
                    match key.code {
                        KeyCode::Esc => {
                            return Some(Action::PopupReturn(Self::ID.to_string(), None));
                        }
                        KeyCode::Enter => {
                            let item = text_area.lines().join("\n");
                            match hex_helper_mode {
                                HexHelperMode::Guid => {
                                    let mut parsed: Option<Uuid> = None;
                                    if let Ok(uuid) = Uuid::try_parse(item.as_str()) {
                                        parsed = Some(uuid);
                                    } else if let Some(uuid) = parse_c_format_guid(item.as_str()) {
                                        parsed = Some(uuid);
                                    }
                                    if let Some(uuid) = parsed {
                                        let bytes = uuid.to_bytes_le();
                                        let mut search_str = String::new();

                                        for b in bytes {
                                            search_str.push_str(format!("\\x{b:02x}").as_str());
                                        }

                                        self.should_exit = true;
                                        return Some(Action::EditSearch(Some(search_str)));
                                    } else {
                                        return Some(Action::Notification(Notification {
                                            title: "Guid search".to_string(),
                                            body: "Not a valid GUID".to_string(),
                                        }));
                                    }
                                }
                                HexHelperMode::ByteString => {
                                    if let Some(bytes) = parse_byte_string(item.as_str()) {
                                        let mut search_str = String::new();
                                        for b in bytes {
                                            search_str.push_str(format!("\\x{b:02x}").as_str());
                                        }

                                        self.should_exit = true;
                                        return Some(Action::EditSearch(Some(search_str)));
                                    } else {
                                        return Some(Action::Notification(Notification {
                                            title: "Byte string search".to_string(),
                                            body: "Not a valid byte string".to_string(),
                                        }));
                                    }
                                }
                                HexHelperMode::Utf16le => {
                                    let utf16_bytes = item.encode_utf16();
                                    let mut search_str = String::new();
                                    for b in utf16_bytes.into_iter() {
                                        let [h, l] = b.to_le_bytes();
                                        search_str.push_str(format!("\\x{h:02x}").as_str());
                                        search_str.push_str(format!("\\x{l:02x}").as_str());
                                    }
                                    self.should_exit = true;
                                    return Some(Action::EditSearch(Some(search_str)));
                                }
                            }
                        }
                        _ => {
                            text_area.input(*key);
                        }
                    }
                }
            }
        }
        return None;
    }

    fn render(&mut self, frame: &mut ratatui::prelude::Frame) {
        let (area, buf) = self.prepare(
            frame,
            Constraint::Percentage(50),
            Constraint::Percentage(80),
        );
        let [selector_area, main_area] = area.layout(&Layout::new(
            Direction::Vertical,
            [Constraint::Length(3), Constraint::Fill(1)],
        ));

        let list_items = self
            .current_displayed_items()
            .into_iter()
            .map(|m| Into::<&str>::into(m));
        match &mut self.state {
            HexHelperState::SelectMode(text_area, _, list_state) => {
                text_area.render(selector_area, buf);
                StatefulWidget::render(
                    List::new(list_items)
                        .highlight_style(Style::default().on_blue())
                        .block(Block::bordered().border_type(BorderType::Rounded)),
                    main_area,
                    buf,
                    list_state,
                );
            }
            HexHelperState::InputContent(hex_helper_mode, text_area) => {
                let mode: &str = (*hex_helper_mode).into();
                Paragraph::new(mode)
                    .block(Block::bordered().border_type(BorderType::Rounded))
                    .render(selector_area, buf);
                text_area.render(main_area, buf);
            }
        }
    }
}

fn line_number_width(ln: usize) -> usize {
    if ln == 0 {
        return 1;
    }
    ln.ilog(16) as usize + 1
}

fn num_to_hex_chars(n: u8) -> (char, char) {
    let hh = n >> 4;
    let lh = n & 0xF;
    let hh_char = if hh >= 0xA {
        ('A' as u8) - 0xA + hh
    } else {
        ('0' as u8) + hh
    } as char;
    let lh_char = if lh >= 0xA {
        ('A' as u8) - 0xA + lh
    } else {
        ('0' as u8) + lh
    } as char;

    (hh_char, lh_char)
}

fn byte_to_display_char(n: u8) -> char {
    let ch = n as char;
    if ch.is_ascii_graphic() { ch } else { '.' }
}

#[cfg(test)]
mod test {
    use crate::ui::hex_view::{HexViewMode, line_number_width, num_to_hex_chars};

    #[test]
    fn test_linenumber_width() {
        assert_eq!(line_number_width(10), 1);
        assert_eq!(line_number_width(0xf), 1);
        assert_eq!(line_number_width(0xfa), 2);
        assert_eq!(line_number_width(0xfab), 3);
        assert_eq!(line_number_width(0xfab0), 4);
    }

    #[test]
    fn test_width() {
        assert_eq!(
            HexViewMode::Partial(8).required_width(None),
            "00001234 │ 00 00 00 00 00 00 00 00".chars().count()
        );
        assert_eq!(
            HexViewMode::Partial(10).required_width(None),
            "00001234 │ 00 00 00 00 00 00 00 00 | 00 00".chars().count()
        );
        assert_eq!(
            HexViewMode::Partial(10).required_width(Some(0x1234)),
            "1234 │ 00 00 00 00 00 00 00 00 | 00 00".chars().count()
        );
        assert_eq!(
            HexViewMode::FullHex.required_width(None),
            "00001234 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00"
                .chars()
                .count()
        );
        assert_eq!(
            HexViewMode::FullHex.required_width(Some(20)),
            "14 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00"
                .chars()
                .count()
        );
        assert_eq!(
            HexViewMode::FullHexWithAscii.required_width(None),
            "12345678 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00 │ abcdefgh ........"
                .chars()
                .count()
        );
        assert_eq!(
            HexViewMode::FullHexWithAscii.required_width(Some(1)),
            "1 │ 00 00 00 00 00 00 00 00 | 00 00 00 00 00 00 00 00 │ abcdefgh ........"
                .chars()
                .count()
        );
    }

    #[test]
    fn test_hex_to_chars() {
        assert_eq!(num_to_hex_chars(0xAB), ('A', 'B'));
        assert_eq!(num_to_hex_chars(0x00), ('0', '0'));
        assert_eq!(num_to_hex_chars(0x1b), ('1', 'B'));
        assert_eq!(num_to_hex_chars(0xc5), ('C', '5'));
    }
}
