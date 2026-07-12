use std::{
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
use similar::{DiffOp, DiffTag, TextDiff};
use tracing::error;

use crate::{
    DiffTuiConfig, DiffTuiError,
    diff::DiffSide,
    ui::{
        Action, EventHandler, GotoMenu, JumpToPopup, Notification, TabState,
        folder_cmp_view::FolderCmpState, hex_cmp_view::HexCmpView, menu::Menu,
    },
};

fn find_diff_op(ops: &[DiffOp], index: usize, side: DiffSide) -> Result<DiffOp, DiffTuiError> {
    match side {
        DiffSide::Left => ops.binary_search_by(|op| match *op {
            DiffOp::Equal {
                old_index,
                new_index: _,
                len,
            } => {
                if index < old_index {
                    std::cmp::Ordering::Greater
                } else if index < old_index + len {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index: _,
            } => {
                if index < old_index {
                    std::cmp::Ordering::Greater
                } else if index < old_index + old_len {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
            DiffOp::Insert {
                old_index,
                new_index: _,
                new_len: _,
            } => {
                if index < old_index {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index: _,
                new_len: _,
            } => {
                if index < old_index {
                    std::cmp::Ordering::Greater
                } else if index < old_index + old_len {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
        }),
        DiffSide::Right => ops.binary_search_by(|op| match *op {
            DiffOp::Equal {
                old_index: _,
                new_index,
                len,
            } => {
                if index < new_index {
                    std::cmp::Ordering::Greater
                } else if index < new_index + len {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
            DiffOp::Delete {
                old_index: _,
                old_len: _,
                new_index,
            } => {
                if index < new_index {
                    std::cmp::Ordering::Greater
                } else {
                    std::cmp::Ordering::Less
                }
            }
            DiffOp::Insert {
                old_index: _,
                new_index,
                new_len,
            } => {
                if index < new_index {
                    std::cmp::Ordering::Greater
                } else if index < new_index + new_len {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
            DiffOp::Replace {
                old_index: _,
                old_len: _,
                new_index,
                new_len,
            } => {
                if index < new_index {
                    std::cmp::Ordering::Greater
                } else if index < new_index + new_len {
                    std::cmp::Ordering::Equal
                } else {
                    std::cmp::Ordering::Less
                }
            }
        }),
    }
    .map(|i| ops[i])
    .map_err(|_| DiffTuiError::NodeNotFound)
}

pub struct TextCmpView {
    lhs_path: PathBuf,
    rhs_path: PathBuf,
    lhs_lines: Vec<String>,
    rhs_lines: Vec<String>,
    selected: usize,
    horzontal_scroll: usize,
    title: String,
    page_height: Option<u16>,
    highlight: Option<Regex>,
    line_number: bool,
    diffs: Vec<DiffOp>,
    view_start: usize,
    scroll_padding: usize,
    diff_hunks: Vec<(usize, usize)>,
    lhs_line_map: Vec<Option<usize>>,
    rhs_line_map: Vec<Option<usize>>,
    config: DiffTuiConfig,
}

impl Debug for TextCmpView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextCmpView")
            .field("lhs_path", &self.lhs_path)
            .field("rhs_path", &self.rhs_path)
            .finish()
    }
}

impl TextCmpView {
    pub fn new(lhs: PathBuf, rhs: PathBuf, config: &DiffTuiConfig) -> Result<Self, DiffTuiError> {
        let lhs_content = std::fs::read(lhs.as_path())?;
        let lhs_content = String::from_utf8_lossy(&lhs_content).to_string();
        let rhs_content = std::fs::read(rhs.as_path())?;
        let rhs_content = String::from_utf8_lossy(&rhs_content).to_string();
        Self::new_from_str(lhs_content, lhs, rhs_content, rhs, config)
    }

    pub fn new_from_str(
        lhs: String,
        lhs_path: PathBuf,
        rhs: String,
        rhs_path: PathBuf,
        config: &DiffTuiConfig,
    ) -> Result<Self, DiffTuiError> {
        let lhs_content = lhs.clone();
        let rhs_content = rhs.clone();
        let diff = TextDiff::from_lines(lhs, rhs);

        let ops = diff.ops().to_vec();

        let mut line_count = 0;
        let diff_hunks = ops
            .iter()
            .filter_map(|op| match op {
                DiffOp::Delete {
                    old_index: _,
                    old_len,
                    new_index: _,
                } => {
                    let start = line_count;
                    line_count += old_len;
                    Some((start, line_count))
                }
                DiffOp::Equal {
                    old_index: _,
                    new_index: _,
                    len,
                } => {
                    line_count += len;
                    None
                }
                DiffOp::Insert {
                    old_index: _,
                    new_index: _,
                    new_len,
                } => {
                    let start = line_count;
                    line_count += new_len;
                    Some((start, line_count))
                }
                DiffOp::Replace {
                    old_index: _,
                    old_len,
                    new_index: _,
                    new_len,
                } => {
                    let start = line_count;
                    line_count += old_len.max(new_len);
                    Some((start, line_count))
                }
            })
            .collect();

        let mut lhs_line_map = Vec::new();
        let mut rhs_line_map = Vec::new();

        for op in ops.iter() {
            match *op {
                DiffOp::Equal {
                    old_index,
                    new_index,
                    len,
                } => {
                    for line in old_index..(old_index + len) {
                        lhs_line_map.push(Some(line));
                    }
                    for line in new_index..(new_index + len) {
                        rhs_line_map.push(Some(line));
                    }
                }
                DiffOp::Delete {
                    old_index,
                    old_len,
                    new_index,
                } => {
                    for line in old_index..(old_index + old_len) {
                        lhs_line_map.push(Some(line));
                    }
                    for _ in new_index..(new_index + old_len) {
                        rhs_line_map.push(None);
                    }
                }
                DiffOp::Insert {
                    old_index,
                    new_index,
                    new_len,
                } => {
                    for _ in old_index..(old_index + new_len) {
                        lhs_line_map.push(None);
                    }
                    for line in new_index..(new_index + new_len) {
                        rhs_line_map.push(Some(line));
                    }
                }
                DiffOp::Replace {
                    old_index,
                    old_len,
                    new_index,
                    new_len,
                } => {
                    let max_len = old_len.max(new_len);
                    for line in old_index..(old_index + old_len) {
                        lhs_line_map.push(Some(line));
                    }
                    for _ in old_len..max_len {
                        lhs_line_map.push(None);
                    }
                    for line in new_index..(new_index + new_len) {
                        rhs_line_map.push(Some(line));
                    }
                    for _ in new_len..max_len {
                        rhs_line_map.push(None);
                    }
                }
            }
        }

        Ok(Self {
            title: Self::build_title(lhs_path.as_path(), rhs_path.as_path())
                .unwrap_or(String::from("Text")),
            lhs_path,
            rhs_path,
            selected: 0,
            horzontal_scroll: 0,
            page_height: None,
            highlight: None,
            line_number: true,
            diffs: ops,
            lhs_lines: lhs_content.lines().map(|s| s.to_string()).collect(),
            rhs_lines: rhs_content.lines().map(|s| s.to_string()).collect(),
            view_start: 0,
            scroll_padding: 5,
            diff_hunks,
            lhs_line_map,
            rhs_line_map,
            config: config.clone(),
        })
    }

    pub fn line_number(mut self, line_number: bool) -> Self {
        self.line_number = line_number;
        self
    }

    pub fn fit_window(&mut self) {
        let page_height = self.page_height.unwrap_or(0) as usize;

        if self.selected > (self.view_start + page_height).saturating_sub(self.scroll_padding) {
            self.view_start = (self.selected + self.scroll_padding)
                .min(self.lhs_line_map.len().max(self.rhs_line_map.len()))
                .saturating_sub(page_height);
        } else if self.selected < self.view_start + self.scroll_padding {
            self.view_start = self.selected.saturating_sub(self.scroll_padding);
        }
    }

    pub fn move_sel_down(&mut self, lines: usize) {
        self.selected = self
            .selected
            .saturating_add(lines)
            .min(self.lhs_lines.len().saturating_sub(1));
        self.fit_window();
    }
    pub fn move_sel_up(&mut self, lines: usize) {
        self.selected = self.selected.saturating_sub(lines);
        self.fit_window();
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
                        lhs_parent.and_then(|s| s.to_str()).unwrap_or("\"\""),
                        rhs_parent.and_then(|s| s.to_str()).unwrap_or("\"\""),
                        lhs_base
                    ));
                }
            } else {
                title = Some(format!("{}<=>{}", lhs_base, rhs_base));
            }
        }
        title
    }
}

impl EventHandler for TextCmpView {
    fn handler(&mut self, event: &super::Action) -> Result<Option<super::Action>, DiffTuiError> {
        match event {
            Action::NavDown => self.move_sel_down(1),
            Action::NavUp => self.move_sel_up(1),
            Action::NavRight => self.horzontal_scroll = self.horzontal_scroll.saturating_add(1),
            Action::NavLeft => self.horzontal_scroll = self.horzontal_scroll.saturating_sub(1),
            Action::Goto => {
                return Ok(Some(Action::ShowPopup(Box::new(GotoMenu::default()))));
            }
            Action::PopupReturn(id, Some(action)) if id == GotoMenu::ID => match action.as_str() {
                GotoMenu::TOP => {
                    self.selected = 0;
                    self.fit_window();
                }
                GotoMenu::BOTTOM => {
                    self.selected =
                        (self.lhs_line_map.len().max(self.rhs_line_map.len())).saturating_sub(1);
                    self.fit_window();
                }
                GotoMenu::JUMP => {
                    return Ok(Some(Action::ShowPopup(Box::new(JumpToPopup::default()))));
                }
                _ => {}
            },
            Action::PageDown(factor) => {
                if let Some(page_height) = self.page_height {
                    let line = (page_height as f32 * *factor).floor() as usize;
                    self.move_sel_down(line);
                }
            }
            Action::PageUp(factor) => {
                if let Some(page_height) = self.page_height {
                    let line = (page_height as f32 * *factor).floor() as usize;
                    self.move_sel_up(line);
                }
            }
            Action::NextDiff => {
                let diff_hunks = &self.diff_hunks;

                if let Some(next_hunk) = diff_hunks.iter().find(|h| h.0 > self.selected) {
                    self.selected = next_hunk.0;
                    self.fit_window();
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Next diff".to_string(),
                        body: "Reached last diff".to_string(),
                    })));
                }
            }
            Action::PrevDiff => {
                let diff_hunks = &self.diff_hunks;

                if let Some(prev_hunk) = diff_hunks.iter().rev().find(|h| h.1 < self.selected) {
                    self.selected = prev_hunk.0;
                    self.fit_window();
                } else {
                    return Ok(Some(Action::Notification(Notification {
                        title: "Previous diff".to_string(),
                        body: "Reached first diff".to_string(),
                    })));
                }
            }
            Action::SearchNext(r) => {
                self.highlight = Some(r.clone());
                let mut idx = self.selected + 1;
                while let Some((ll, rl)) =
                    self.lhs_line_map.get(idx).zip(self.rhs_line_map.get(idx))
                {
                    if ll
                        .and_then(|l| self.lhs_lines.get(l))
                        .is_some_and(|line| r.is_match(line.as_bytes()))
                        || rl
                            .and_then(|l| self.rhs_lines.get(l))
                            .is_some_and(|line| r.is_match(line.as_bytes()))
                    {
                        self.selected = idx;
                        self.fit_window();
                        return Ok(None);
                    }
                    idx += 1;
                }
                idx = 0;
                while let Some((ll, rl)) =
                    self.lhs_line_map.get(idx).zip(self.rhs_line_map.get(idx))
                {
                    if idx == self.selected {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Search".to_string(),
                            body: "No matches found".to_string(),
                        })));
                    }
                    if ll
                        .and_then(|l| self.lhs_lines.get(l))
                        .is_some_and(|line| r.is_match(line.as_bytes()))
                        || rl
                            .and_then(|l| self.rhs_lines.get(l))
                            .is_some_and(|line| r.is_match(line.as_bytes()))
                    {
                        self.selected = idx;
                        self.fit_window();
                        return Ok(None);
                    }
                    idx += 1;
                }
            }
            Action::SearchPrev(r) => {
                self.highlight = Some(r.clone());
                let mut idx = self.selected;
                if idx != 0 {
                    idx -= 1;
                    while let Some((ll, rl)) =
                        self.lhs_line_map.get(idx).zip(self.rhs_line_map.get(idx))
                    {
                        if ll
                            .and_then(|l| self.lhs_lines.get(l))
                            .is_some_and(|line| r.is_match(line.as_bytes()))
                            || rl
                                .and_then(|l| self.rhs_lines.get(l))
                                .is_some_and(|line| r.is_match(line.as_bytes()))
                        {
                            self.selected = idx;
                            return Ok(None);
                        }
                        idx -= 1;
                    }
                }
                idx = self.lhs_line_map.len() - 1;
                while let Some((ll, rl)) =
                    self.lhs_line_map.get(idx).zip(self.rhs_line_map.get(idx))
                {
                    if idx == self.selected {
                        return Ok(Some(Action::Notification(Notification {
                            title: "Search".to_string(),
                            body: "No matches found".to_string(),
                        })));
                    }
                    if ll
                        .and_then(|l| self.lhs_lines.get(l))
                        .is_some_and(|line| r.is_match(line.as_bytes()))
                        || rl
                            .and_then(|l| self.rhs_lines.get(l))
                            .is_some_and(|line| r.is_match(line.as_bytes()))
                    {
                        self.selected = idx;
                        return Ok(None);
                    }
                    idx -= 1;
                }
            }
            Action::RemoveHighlight => {
                self.highlight = None;
            }
            Action::TabCustomAction => {
                let mut opts = vec![
                    ("Reopen with hex cmp view".to_string(), Some('h')),
                    ("Jump to line".to_string(), Some(':')),
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
                    "TextCmpView action".to_string(),
                    opts,
                )))));
            }
            Action::PopupReturn(id, Some(item)) if id == "TextCmpView action" => {
                match item.as_str() {
                    "Reopen with hex cmp view" => {
                        return Ok(Some(Action::CreateTabAndSwitch(Box::new(HexCmpView::new(
                            self.lhs_path.clone(),
                            self.rhs_path.clone(),
                            &self.config,
                        )?))));
                    }
                    "Open parent folder in folder cmp view" => {
                        if let Some((lhs, rhs)) = self.lhs_path.parent().zip(self.rhs_path.parent())
                        {
                            let config = self.config.clone();
                            return Ok(Some(Action::CreateTabAndSwitch(Box::new(
                                FolderCmpState::new(lhs, rhs, &config, None, None)?,
                            ))));
                        }
                    }
                    "Jump to line" => {
                        return Ok(Some(Action::ShowPopup(Box::new(JumpToPopup::default()))));
                    }
                    _ => {}
                }
            }
            Action::PopupReturn(id, Some(item)) if id == "JumpTo" => {
                let item = item.trim();

                if let Some(item) = item.strip_prefix("0x") {
                    match usize::from_str_radix(item, 16) {
                        Ok(i) => {
                            self.selected = i;
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
                match item.parse() {
                    Ok(i) => {
                        self.selected = i;
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
                return Ok(Some(Action::Notification(Notification {
                    title: "Unimplemented".to_string(),
                    body: "Unimplemented".to_string(),
                })));
            }
            _ => {}
        }
        Ok(None)
    }
}

impl TabState for TextCmpView {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn render(&mut self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer) {
        let layout = Layout::new(
            Direction::Horizontal,
            [Constraint::Fill(1), Constraint::Fill(1)],
        )
        .spacing(Spacing::Overlap(1));
        let [lhs_area, rhs_area] = area.layout(&layout);
        let list_block = Block::bordered()
            .borders(Borders::all())
            .merge_borders(MergeStrategy::Exact);
        let page_height = list_block.inner(area).height;
        self.page_height = Some(page_height);

        let mut lhs_list = Vec::with_capacity(page_height as usize);
        let mut rhs_list = Vec::with_capacity(page_height as usize);
        let empty_line_len = list_block
            .inner(lhs_area)
            .width
            .max(list_block.inner(rhs_area).width);
        let mut empty_line_str = String::new();

        for _ in 0..empty_line_len {
            empty_line_str.push('-');
        }

        let empty_line = Line::from(empty_line_str.as_str()).dim().red();

        let max_ln = self.lhs_lines.len().max(self.rhs_lines.len());
        let ln_space = max_ln.ilog10() + 1;

        for index in self.view_start
            ..(self.view_start + (page_height as usize))
                .min(self.lhs_line_map.len().max(self.rhs_line_map.len()))
        {
            if let Some(ln) = self.lhs_line_map[index] {
                let op = match find_diff_op(&self.diffs, ln, DiffSide::Left).map(|op| op.tag()) {
                    Ok(t) => t,
                    Err(_) => {
                        error!("Missing diff op for line {ln} at lhs");
                        DiffTag::Equal
                    }
                };

                let mut line = if self.line_number {
                    Line::from_iter([
                        Span::from(format!("{:1$}", ln, ln_space as usize)).dim(),
                        Span::from(" │ ").dim(),
                    ])
                } else {
                    Line::default()
                }
                .fg(if op != DiffTag::Equal {
                    Color::Red
                } else {
                    Color::default()
                });
                line.push_span(
                    &self.lhs_lines[ln][(self.horzontal_scroll.min(self.lhs_lines[ln].len()))..],
                );

                lhs_list.push(line);
            } else {
                lhs_list.push(empty_line.clone());
            }

            if let Some(ln) = self.rhs_line_map[index] {
                let op = match find_diff_op(&self.diffs, ln, DiffSide::Right).map(|op| op.tag()) {
                    Ok(t) => t,
                    Err(_) => {
                        error!("Missing diff op for line {ln} at rhs");
                        DiffTag::Equal
                    }
                };

                let mut line = if self.line_number {
                    Line::from_iter([
                        Span::from(format!("{:1$}", ln, ln_space as usize)).dim(),
                        Span::from(" │ ").dim(),
                    ])
                } else {
                    Line::default()
                }
                .fg(if op != DiffTag::Equal {
                    Color::Red
                } else {
                    Color::default()
                });
                line.push_span(
                    &self.rhs_lines[ln][self.horzontal_scroll.min(self.rhs_lines[ln].len())..],
                );

                rhs_list.push(line);
            } else {
                rhs_list.push(empty_line.clone());
            }
        }

        List::new(lhs_list)
            .highlight_style(Style::default().reversed())
            .block(
                Block::bordered()
                    .borders(Borders::all())
                    .merge_borders(MergeStrategy::Exact),
            )
            .render(
                lhs_area,
                buf,
                &mut ListState::default()
                    .with_selected(Some(self.selected.saturating_sub(self.view_start))),
            );
        List::new(rhs_list)
            .highlight_style(Style::default().reversed())
            .block(
                Block::bordered()
                    .borders(Borders::all())
                    .merge_borders(MergeStrategy::Exact),
            )
            .render(
                rhs_area,
                buf,
                &mut ListState::default()
                    .with_selected(Some(self.selected.saturating_sub(self.view_start))),
            );
    }

    fn reload(&mut self) -> Result<Option<Box<dyn TabState>>, DiffTuiError> {
        Ok(Some(Box::new(TextCmpView::new(
            self.lhs_path.clone(),
            self.rhs_path.clone(),
            &self.config,
        )?)))
    }
}
