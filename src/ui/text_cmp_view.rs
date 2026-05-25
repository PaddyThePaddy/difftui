use std::{fmt::Debug, path::PathBuf};

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListState, StatefulWidget},
};
use similar::{ChangeTag, TextDiff};

use crate::{
    DiffTuiError,
    ui::{Action, EventHandler, TabState},
};

pub struct TextCmpView<'a> {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    diff: TextDiff<'a, 'a, str>,
    sel: ListState,
}

impl<'a> Debug for TextCmpView<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextCmpView")
            .field("lhs_path", &self.lhs_path)
            .field("rhs_path", &self.rhs_path)
            .finish()
    }
}

impl<'a> TextCmpView<'a> {
    pub fn new(lhs: PathBuf, rhs: PathBuf) -> Result<Self, DiffTuiError> {
        let lhs_content = std::fs::read_to_string(lhs.as_path())?;
        let rhs_content = std::fs::read_to_string(rhs.as_path())?;
        let diff = TextDiff::from_lines(lhs_content, rhs_content);
        Ok(Self {
            lhs_path: lhs,
            rhs_path: rhs,
            diff,
            sel: ListState::default().with_selected(Some(0)),
        })
    }
}

impl<'a> EventHandler for TextCmpView<'a> {
    fn handler(&mut self, event: &super::Action) -> Result<Option<super::Action>, DiffTuiError> {
        match event {
            Action::NavDown => self.sel.select_next(),
            Action::NavUp => self.sel.select_previous(),
            _ => {}
        }
        Ok(None)
    }
}

impl<'a> TabState for TextCmpView<'a> {
    fn title(&self) -> String {
        "Text".to_string()
    }

    fn render(&mut self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let mut lhs_list = vec![];
        let mut rhs_list = vec![];
        for change in self.diff.iter_all_inline_changes() {
            if change.tag() == ChangeTag::Equal {
                while lhs_list.len() < rhs_list.len() {
                    lhs_list.push(Line::default().crossed_out());
                }
                while lhs_list.len() > rhs_list.len() {
                    rhs_list.push(Line::default().crossed_out());
                }
            }
            let line = Line::from_iter(change.values().iter().map(|(hl, text)| {
                let span = Span::from(*text);
                if *hl { span.underlined() } else { span }
            }))
            .fg(if change.tag() != ChangeTag::Equal {
                Color::Red
            } else {
                Color::default()
            });

            if change.old_index().is_some() {
                lhs_list.push(line.clone());
            }
            if change.new_index().is_some() {
                rhs_list.push(line.clone());
            }
        }
        while lhs_list.len() < rhs_list.len() {
            lhs_list.push(Line::default().crossed_out());
        }
        while lhs_list.len() > rhs_list.len() {
            rhs_list.push(Line::default().crossed_out());
        }

        let layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Fill(1), Constraint::Fill(1)],
        );
        let [lhs_area, rhs_area] = area.layout(&layout);
        List::new(lhs_list)
            .highlight_style(Style::default().on_dark_gray())
            .block(Block::bordered())
            .render(lhs_area, buf, &mut self.sel);
        List::new(rhs_list)
            .highlight_style(Style::default().on_dark_gray())
            .block(Block::bordered())
            .render(rhs_area, buf, &mut self.sel);
    }
}
