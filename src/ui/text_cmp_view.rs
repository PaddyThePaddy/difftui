use std::{
    cmp::max,
    fmt::Debug,
    path::{Path, PathBuf},
};

use ratatui::{
    layout::{Constraint, Direction, Layout, Spacing},
    style::{Color, Style, Stylize},
    symbols::merge::MergeStrategy,
    text::{Line, Span},
    widgets::{Block, Borders, List, ListState, StatefulWidget},
};
use regex::bytes::Regex;
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
    horzontal_scroll: usize,
    title: String,
    page_height: Option<u16>,
    highlight: Option<Regex>,
    line_number: bool,
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
        let lhs_content = std::fs::read(lhs.as_path())?;
        let lhs_content = String::from_utf8_lossy(&lhs_content).to_string();
        let rhs_content = std::fs::read(rhs.as_path())?;
        let rhs_content = String::from_utf8_lossy(&rhs_content).to_string();
        Self::new_from_str(lhs_content, lhs, rhs_content, rhs)
    }

    pub fn new_from_str(
        lhs: String,
        lhs_path: PathBuf,
        rhs: String,
        rhs_path: PathBuf,
    ) -> Result<Self, DiffTuiError> {
        let diff = TextDiff::from_lines(lhs, rhs);

        Ok(Self {
            title: Self::build_title(lhs_path.as_path(), rhs_path.as_path())
                .unwrap_or(String::from("Text")),
            lhs_path: lhs_path,
            rhs_path: rhs_path,
            diff,
            sel: ListState::default().with_selected(Some(0)),
            horzontal_scroll: 0,
            page_height: None,
            highlight: None,
            line_number: true,
        })
    }

    pub fn line_number(mut self, line_number: bool) -> Self {
        self.line_number = line_number;
        self
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
                            title = Some(format!("{}<=>{}/../{}", lhs_comp, rhs_comp, lhs_base));
                            break;
                        }
                    }
                    if title.is_none() {
                        title = Some(lhs_base.to_string());
                    }
                } else {
                    title = Some(format!(
                        "{}<=>{}/{}",
                        lhs_parent.map(|s| s.to_str()).flatten().unwrap_or("\"\""),
                        rhs_parent.map(|s| s.to_str()).flatten().unwrap_or("\"\""),
                        lhs_base
                    ));
                }
            } else {
                title = Some(format!("{}<=>{}", lhs_base, rhs_base));
            }
        }
        title
    }

    fn build_lines(&self) -> (Vec<Option<String>>, Vec<Option<String>>) {
        let inline_diff = self.diff.iter_all_changes().collect::<Vec<_>>();
        let mut lhs_list = vec![];
        let mut rhs_list = vec![];

        for change in inline_diff.iter() {
            if change.tag() == ChangeTag::Equal {
                while lhs_list.len() < rhs_list.len() {
                    lhs_list.push(None);
                }
                while lhs_list.len() > rhs_list.len() {
                    rhs_list.push(None);
                }
            }
            let text = change.value();

            if let Some(_) = change.old_index() {
                lhs_list.push(Some(text.to_string()));
            }
            if let Some(_) = change.new_index() {
                rhs_list.push(Some(text.to_string()));
            }
        }

        while lhs_list.len() < rhs_list.len() {
            lhs_list.push(None);
        }
        while lhs_list.len() > rhs_list.len() {
            rhs_list.push(None);
        }
        (lhs_list, rhs_list)
    }
}

impl<'a> EventHandler for TextCmpView<'a> {
    fn handler(&mut self, event: &super::Action) -> Result<Option<super::Action>, DiffTuiError> {
        match event {
            Action::NavDown => self.sel.select_next(),
            Action::NavUp => self.sel.select_previous(),
            Action::NavRight => self.horzontal_scroll = self.horzontal_scroll.saturating_add(1),
            Action::NavLeft => self.horzontal_scroll = self.horzontal_scroll.saturating_sub(1),
            Action::NavTop => self.sel.select_first(),
            Action::NavBottom => self.sel.select_last(),
            Action::PageDown(factor) => {
                if let Some(page_height) = self.page_height {
                    let line = (page_height as f32 * *factor).floor() as u16;
                    for _ in 0..line {
                        self.sel.select_next();
                    }
                }
            }
            Action::PageUp(factor) => {
                if let Some(page_height) = self.page_height {
                    let line = (page_height as f32 * *factor).floor() as u16;
                    for _ in 0..line {
                        self.sel.select_previous();
                    }
                }
            }
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
            Action::SearchNext(r) => {
                self.highlight = Some(r.clone());
                if let Some(current) = self.sel.selected() {
                    let (ll, rl) = self.build_lines();
                    let mut idx = current + 1;
                    while let Some((ll, rl)) = ll.get(idx).zip(rl.get(idx)) {
                        if ll.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                            || rl.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                        {
                            self.sel.select(Some(idx));
                            return Ok(None);
                        }
                        idx += 1;
                    }
                    idx = 0;
                    while let Some((ll, rl)) = ll.get(idx).zip(rl.get(idx)) {
                        if idx == current {
                            return Ok(Some(Action::Notification(Notification {
                                title: "Search".to_string(),
                                body: "No matches found".to_string(),
                            })));
                        }
                        if ll.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                            || rl.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                        {
                            self.sel.select(Some(idx));
                            return Ok(None);
                        }
                        idx += 1;
                    }
                }
            }
            Action::SearchPrev(r) => {
                self.highlight = Some(r.clone());
                if let Some(current) = self.sel.selected() {
                    let (ll, rl) = self.build_lines();
                    let mut idx = current;
                    if idx != 0 {
                        idx -= 1;
                        while let Some((ll, rl)) = ll.get(idx).zip(rl.get(idx)) {
                            if ll.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                                || rl.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                            {
                                self.sel.select(Some(idx));
                                return Ok(None);
                            }
                            idx -= 1;
                        }
                    }
                    idx = ll.len() - 1;
                    while let Some((ll, rl)) = ll.get(idx).zip(rl.get(idx)) {
                        if idx == current {
                            return Ok(Some(Action::Notification(Notification {
                                title: "Search".to_string(),
                                body: "No matches found".to_string(),
                            })));
                        }
                        if ll.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                            || rl.as_ref().is_some_and(|l| r.is_match(l.as_bytes()))
                        {
                            self.sel.select(Some(idx));
                            return Ok(None);
                        }
                        idx -= 1;
                    }
                }
            }
            Action::RemoveHighlight => {
                self.highlight = None;
            }
            _ => {}
        }
        Ok(None)
    }
}

impl<'a> TabState for TextCmpView<'a> {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn render(&mut self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        self.page_height = Some(area.height - 2);
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
            let mut horizontal_scroll = self.horzontal_scroll;
            let text = change
                .values()
                .iter()
                .filter_map(|(hl, text)| {
                    if horizontal_scroll == 0 {
                        let span = Span::from(*text);
                        if *hl {
                            Some(span.underlined())
                        } else {
                            Some(span)
                        }
                    } else {
                        if let Some(scroll_point) =
                            text.char_indices().nth(horizontal_scroll).map(|(i, _)| i)
                        {
                            horizontal_scroll =
                                horizontal_scroll.saturating_sub(text.char_indices().count());
                            let span = Span::from(&text[scroll_point..]);
                            if *hl {
                                Some(span.underlined())
                            } else {
                                Some(span)
                            }
                        } else {
                            horizontal_scroll =
                                horizontal_scroll.saturating_sub(text.char_indices().count());
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            if let Some(ln) = change.old_index() {
                let mut line = if self.line_number {
                    Line::from_iter([
                        Span::from(format!("{:1$}", ln, ln_space)).dim(),
                        Span::from(" │ ").dim(),
                    ])
                } else {
                    Line::default()
                }
                .fg(if change.tag() != ChangeTag::Equal {
                    Color::Red
                } else {
                    Color::default()
                });
                line.extend(text.clone());

                lhs_list.push(line);
            }
            if let Some(ln) = change.new_index() {
                let mut line = if self.line_number {
                    Line::from_iter([
                        Span::from(format!("{:1$}", ln, ln_space)).dim(),
                        Span::from(" │ ").dim(),
                    ])
                } else {
                    Line::default()
                }
                .fg(if change.tag() != ChangeTag::Equal {
                    Color::Red
                } else {
                    Color::default()
                });
                line.extend(text);

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
        )
        .spacing(Spacing::Overlap(1));
        let [lhs_area, rhs_area] = area.layout(&layout);
        List::new(lhs_list)
            .scroll_padding(5)
            .highlight_style(Style::default().on_dark_gray())
            .block(
                Block::bordered()
                    .borders(Borders::all())
                    .merge_borders(MergeStrategy::Exact),
            )
            .render(lhs_area, buf, &mut self.sel);
        List::new(rhs_list)
            .scroll_padding(5)
            .highlight_style(Style::default().on_dark_gray())
            .block(
                Block::bordered()
                    .borders(Borders::all())
                    .merge_borders(MergeStrategy::Exact),
            )
            .render(rhs_area, buf, &mut self.sel);
    }
}
