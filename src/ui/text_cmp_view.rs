use std::{cmp::max, fmt::Debug, path::PathBuf};

use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, List, ListState, StatefulWidget},
};
use similar::{ChangeTag, TextDiff};

use crate::{
    DiffTuiError,
    ui::{Action, EventHandler, Notification, TabState},
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

    fn diff_hunks(&self) -> Vec<(usize, usize)> {
        let mut list: Vec<(usize, usize)> = vec![];

        for change in self.diff.iter_all_changes() {
            if change.tag() == ChangeTag::Equal {
                continue;
            }
            let idx = max(
                change.old_index().unwrap_or(0),
                change.new_index().unwrap_or(0),
            );

            if let Some(last_hunk) = list.last_mut().filter(|h| h.1 == idx) {
                last_hunk.1 = idx + 1;
            } else {
                list.push((idx, idx + 1));
            }
        }

        return list;
    }
}

impl<'a> EventHandler for TextCmpView<'a> {
    fn handler(&mut self, event: &super::Action) -> Result<Option<super::Action>, DiffTuiError> {
        match event {
            Action::NavDown => self.sel.select_next(),
            Action::NavUp => self.sel.select_previous(),
            Action::NavTop => self.sel.select_first(),
            Action::NavBottom => self.sel.select_last(),
            Action::NextDiff => {
                if let Some(current_ln) = self.sel.selected() {
                    let diff_hunks = self.diff_hunks();

                    if let Some(next_hunk) = diff_hunks.iter().find(|h| h.0 > current_ln) {
                        self.sel.select(Some(next_hunk.0));
                    } else {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Next diff".to_string(),
                            body: "Reached last diff".to_string(),
                        })));
                    }
                }
            }
            Action::PrevDiff => {
                if let Some(current_ln) = self.sel.selected() {
                    let diff_hunks = self.diff_hunks();

                    if let Some(prev_hunk) = diff_hunks.iter().rev().find(|h| h.1 < current_ln) {
                        self.sel.select(Some(prev_hunk.0));
                    } else {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Previous diff".to_string(),
                            body: "Reached first diff".to_string(),
                        })));
                    }
                }
            }
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
        // TODO: Can we cache inline_diff so we don't need to build it for every frame?
        let inline_diff = self.diff.iter_all_inline_changes().collect::<Vec<_>>();
        let mut lhs_list = vec![];
        let mut rhs_list = vec![];
        let empty_line_len = (area.width + 1) / 2 - 2;
        let mut empty_line_str = String::new();

        for _ in 0..empty_line_len {
            empty_line_str.push('-');
        }

        let empty_line = Line::from(empty_line_str.as_str()).dim().red();

        let max_ln = inline_diff
            .iter()
            .map(|c| max(c.old_index().unwrap_or(0), c.new_index().unwrap_or(0)))
            .max()
            .unwrap_or(0);
        let ln_space = max_ln.to_string().len();

        for change in inline_diff.iter() {
            if change.tag() == ChangeTag::Equal {
                while lhs_list.len() < rhs_list.len() {
                    lhs_list.push(empty_line.clone());
                }
                while lhs_list.len() > rhs_list.len() {
                    rhs_list.push(empty_line.clone());
                }
            }
            let text = change
                .values()
                .iter()
                .map(|(hl, text)| {
                    let span = Span::from(*text);
                    if *hl { span.underlined() } else { span }
                })
                .collect::<Vec<_>>();

            if let Some(ln) = change.old_index() {
                let mut line = Line::from_iter([
                    Span::from(format!("{:1$}", ln, ln_space)).dim(),
                    Span::from(" │ ").dim(),
                ])
                .fg(if change.tag() != ChangeTag::Equal {
                    Color::Red
                } else {
                    Color::default()
                });
                line.extend(text.clone());

                lhs_list.push(line);
            }
            if let Some(ln) = change.new_index() {
                let mut line = Line::from_iter([
                    Span::from(format!("{:1$}", ln, ln_space)).dim(),
                    Span::from(" │ ").dim(),
                ])
                .fg(if change.tag() != ChangeTag::Equal {
                    Color::Red
                } else {
                    Color::default()
                });
                line.extend(text.clone());

                rhs_list.push(line);
            }
        }

        while lhs_list.len() < rhs_list.len() {
            lhs_list.push(empty_line.clone());
        }
        while lhs_list.len() > rhs_list.len() {
            rhs_list.push(empty_line.clone());
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
