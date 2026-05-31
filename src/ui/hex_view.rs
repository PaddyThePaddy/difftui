use ratatui::{
    layout::{Constraint, Direction, Layout, Offset, Position},
    prelude::{Buffer, Rect},
    style::Style,
    text::Span,
    widgets::{Block, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget},
};

const DIV_EVERY_BYTES: usize = 8;

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

#[derive(Debug, Clone)]
pub struct HexView<'buf, 'blk, 'hl> {
    buf: &'buf [u8],
    style: HexViewStyle,
    block: Block<'blk>,
    has_scroll_bar: bool,
    hl_groups: Option<&'hl [(usize, usize)]>,
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

    fn is_hl(&self, idx: usize) -> bool {
        if let Some(hl_groups) = self.hl_groups {
            for group in hl_groups.iter() {
                if idx >= group.0 && idx <= group.0 + group.1 {
                    return true;
                }
            }
        }
        false
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

                if self.is_hl(line_start + i) {
                    byte_style = byte_style.red().not_dim();
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
            let mut style = if self.is_hl(line_start + i) {
                Style::default().red()
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

    pub fn set_hl_groups(mut self, hl_groups: Option<&'hl [(usize, usize)]>) -> Self {
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
