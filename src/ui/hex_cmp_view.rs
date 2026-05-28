use std::{cmp::min, path::PathBuf};

use crate::{
    DiffTuiError,
    ui::{EventHandler, TabState, text_cmp_view::TextCmpView},
};

#[derive(Debug)]
pub struct HexCmpView<'a> {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    text_view: TextCmpView<'a>,
}

impl<'a> HexCmpView<'a> {
    pub fn new(lhs: PathBuf, rhs: PathBuf) -> Result<Self, DiffTuiError> {
        let lhs_buf = std::fs::read(&lhs)?;
        let rhs_buf = std::fs::read(&rhs)?;
        let lhs_dump = Self::buf_to_hex_dump(&lhs_buf);
        let rhs_dump = Self::buf_to_hex_dump(&rhs_buf);

        let text_view = TextCmpView::new_from_str(lhs_dump, lhs.clone(), rhs_dump, rhs.clone())?
            .line_number(false);

        Ok(Self {
            lhs_path: lhs,
            rhs_path: rhs,
            text_view,
        })
    }

    fn buf_to_hex_dump(buf: &[u8]) -> String {
        let mut offset = 0;
        let mut output = String::new();
        let mut ascii_str = String::with_capacity(20);
        while offset < buf.len() {
            output.push_str(format!("{offset:08X} │").as_str());
            let mut printed_bytes = 0;

            for idx in offset..min(offset + 8, buf.len()) {
                output.push_str(format!(" {:02X}", buf[idx]).as_str());
                printed_bytes += 1;
                ascii_str.push(
                    char::from_u32(buf[idx] as u32)
                        .and_then(|c| if c.is_control() { None } else { Some(c) })
                        .unwrap_or('.'),
                );
            }
            output.push(' ');
            for idx in min(offset + 8, buf.len().saturating_sub(1))..min(offset + 16, buf.len()) {
                output.push_str(format!(" {:02X}", buf[idx]).as_str());
                printed_bytes += 1;
                ascii_str.push(
                    char::from_u32(buf[idx] as u32)
                        .and_then(|c| if c.is_control() { None } else { Some(c) })
                        .unwrap_or('.'),
                );
            }
            for _ in printed_bytes..16 {
                output.push_str("   ");
                ascii_str.push(' ');
            }
            output.push_str(" │ ");
            output.push_str(&ascii_str);
            output.push_str(" │\n");
            offset += 16;
            ascii_str.clear();
        }
        output
    }
}

impl<'a> EventHandler for HexCmpView<'a> {
    fn handler(&mut self, event: &super::Action) -> Result<Option<super::Action>, DiffTuiError> {
        self.text_view.handler(event)
    }
}

impl<'a> TabState for HexCmpView<'a> {
    fn title(&self) -> String {
        self.text_view.title()
    }

    fn render(&mut self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        self.text_view.render(area, buf);
    }
}
