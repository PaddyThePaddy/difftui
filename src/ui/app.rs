use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    text::Span,
    widgets::{Block, BorderType, Clear, Paragraph, Tabs, Widget as _},
};
use regex::bytes::{Regex, RegexBuilder};
use tracing::{error, trace};

use crate::{
    DiffTuiError,
    ui::{Action, Notification, Popup, TabState, TuiTerminal, run_ext_tui_app, tui},
};

#[derive(Debug, Default)]
pub enum SearchState {
    #[default]
    None,
    Editing(String),
    Finished(String, Regex),
}

impl SearchState {
    pub fn pattern(&self) -> Option<&Regex> {
        if let Self::Finished(_, pattern) = self {
            Some(pattern)
        } else {
            None
        }
    }

    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn text(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::Finished(s, _) => Some(s.as_str()),
            Self::Editing(s) => Some(s.as_str()),
        }
    }
}

#[derive(Debug)]
pub struct App {
    should_quit: bool,
    tabs: Vec<Box<dyn TabState>>,
    current_tab: usize,
    popup: Option<Box<dyn Popup>>,
    showing_notify: Option<Notification>,
    search_state: SearchState,
    notify_scroll: (u16, u16),
}

impl App {
    pub fn new(tab: Box<dyn TabState>) -> Self {
        Self {
            should_quit: false,
            tabs: vec![tab],
            current_tab: 0,
            popup: None,
            showing_notify: None,
            search_state: SearchState::None,
            notify_scroll: (0, 0),
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let has_tabline = self.tabs.len() > 1;
        let has_statusline = !self.search_state.is_none();
        let layout = Layout::new(
            Direction::Vertical,
            [
                Constraint::Length(if has_tabline { 1 } else { 0 }),
                Constraint::Fill(1),
                Constraint::Length(if has_statusline { 1 } else { 0 }),
            ],
        );
        let [tabline, content, statusline] = frame.area().layout(&layout);
        if has_tabline {
            Tabs::new(self.tabs.iter().map(|t| t.title()))
                .select(self.current_tab)
                .render(tabline, frame.buffer_mut());
        }
        if let Some(tab) = self.tabs.get_mut(self.current_tab) {
            tab.render(content, frame.buffer_mut());
        }

        if has_statusline {
            self.render_statusline(statusline, frame.buffer_mut());
        }

        if let Some(notify) = &self.showing_notify {
            self.render_notify(frame, notify);
        } else if let Some(popup) = &mut self.popup {
            popup.render(frame);
        }
    }

    fn render_statusline(&self, area: Rect, buf: &mut Buffer) {
        if let Some(s) = &self.search_state.text() {
            let search_prompt = format!("/{s}");
            Span::raw(search_prompt).render(area, buf);
        }
    }

    fn handle_event(&mut self, evt: tui::Event) -> Option<Action> {
        error!("debug 2 evt: {:?}", evt);
        if let tui::Event::Key(key) = evt
            && let KeyCode::Char('c') = key.code
            && key.modifiers == KeyModifiers::CONTROL
        {
            return Some(Action::ExitApp(None));
        }
        if self.showing_notify.is_some() {
            if let tui::Event::Key(key) = evt {
                match key.code {
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                        self.showing_notify = None
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        self.notify_scroll.0 = self.notify_scroll.0.saturating_add(1);
                    }
                    KeyCode::Up | KeyCode::Char('k') => {
                        self.notify_scroll.0 = self.notify_scroll.0.saturating_sub(1);
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        self.notify_scroll.1 = self.notify_scroll.1.saturating_sub(1);
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        self.notify_scroll.1 = self.notify_scroll.1.saturating_add(1);
                    }
                    _ => {}
                }
            }
            return None;
        } else if let Some(popup) = &mut self.popup {
            return popup.handler(&evt);
        } else if let (tui::Event::Key(key), SearchState::Editing(s)) =
            (&evt, &mut self.search_state)
        {
            match key.code {
                KeyCode::Esc => {
                    self.search_state = SearchState::None;
                }
                KeyCode::Enter => {
                    match RegexBuilder::new(&format!("(?-u){s}"))
                        .case_insensitive(s.chars().all(|c| c.is_lowercase()))
                        .build()
                    {
                        Ok(r) => {
                            trace!("Search pattern: {}", r.as_str());
                            self.search_state = SearchState::Finished(s.clone(), r.clone());
                            return Some(Action::SearchNext(r));
                        }
                        Err(e) => {
                            return Some(Action::Notification(Notification {
                                title: "Invalid pattern".to_string(),
                                body: e.to_string(),
                            }));
                        }
                    }
                }
                KeyCode::Backspace => {
                    s.pop();
                }
                KeyCode::Char(c) => {
                    s.push(c);
                }
                _ => {}
            }
            return None;
        } else if let (tui::Event::Key(key), SearchState::Finished(_, r)) =
            (&evt, &self.search_state)
        {
            match key.code {
                KeyCode::Char('n') => return Some(Action::SearchNext(r.clone())),
                KeyCode::Char('N') => return Some(Action::SearchPrev(r.clone())),
                _ => {}
            }
        }

        if let tui::Event::Key(k_evt) = evt {
            Action::try_from(k_evt).ok()
        } else if let tui::Event::Tick = evt {
            Some(Action::Tick)
        } else {
            None
        }
    }

    fn update(
        &mut self,
        act: Action,
        terminal: &mut TuiTerminal,
    ) -> Result<Option<Action>, DiffTuiError> {
        if let Action::PopupReturn(_, _) = act {
            self.popup = None;
        }
        if let Action::RemoveHighlight = act {
            self.search_state = SearchState::None;
        }
        match act {
            Action::ShowPopup(popup) => {
                self.popup = Some(popup);
                Ok(None)
            }
            Action::Notification(notify) => {
                self.notify_scroll = (0, 0);
                self.showing_notify = Some(notify);
                Ok(None)
            }
            Action::PrevTab => {
                if self.current_tab == 0 {
                    if !self.tabs.is_empty() {
                        self.current_tab = self.tabs.len() - 1;
                    }
                } else {
                    self.current_tab -= 1;
                }
                Ok(None)
            }
            Action::NextTab => {
                self.current_tab = self.current_tab.wrapping_add(1);
                if self.current_tab == self.tabs.len() {
                    self.current_tab = 0;
                }
                Ok(None)
            }
            Action::CreateTabAndSwitch(new_tab) => {
                self.tabs.push(new_tab);
                self.current_tab = self.tabs.len() - 1;
                Ok(None)
            }
            Action::CloseTab => {
                self.tabs.remove(self.current_tab);
                if self.tabs.is_empty() {
                    return Ok(Some(Action::ExitApp(Some("All tabs closed".to_string()))));
                }
                if self.current_tab >= self.tabs.len() {
                    self.current_tab = self.tabs.len() - 1;
                }
                Ok(None)
            }
            Action::RunExtApp(mut cmd) => {
                let return_code = run_ext_tui_app(&mut cmd, terminal)?;
                Ok(Some(Action::ExtAppReturn(return_code)))
            }
            Action::ShowHelp => Ok(Some(Action::Notification(Notification {
                title: "Help".to_string(),
                body: vec![
                    "Arrow keys / hjkl => Navigation",
                    "q        => Closet tab",
                    "c        => Compare selected file/folder",
                    "o        => Open selected filde / folder",
                    "o        => Open selected filde / folder with option",
                    "g        => Move to top",
                    "G        => Move to bottom",
                    "f        => Filter files",
                    "z        => Tab specific actions",
                    "R        => Reload tab",
                    "x        => Swap sides",
                    "/        => Search",
                    "n / N    => Search next/previous",
                    "][       => Next/Previous difference",
                    "=        => Decouple side-by-side view",
                    "Enter    => Expand/Collapse",
                    "Ctrl + c => Exit app",
                    "? / F1   => Show help",
                ]
                .join("\n"),
            }))),
            Action::EditSearch(s) => {
                if let Some(s) = s {
                    self.search_state = SearchState::Editing(s);
                } else {
                    self.search_state = SearchState::Editing(String::new());
                }
                Ok(None)
            }
            Action::Reload => {
                if let Some(new_tab) = self.tabs[self.current_tab].reload()? {
                    self.tabs[self.current_tab] = new_tab;
                }
                Ok(None)
            }
            _ => {
                if let Some(tab) = self.tabs.get_mut(self.current_tab) {
                    tab.handler(&act)
                } else {
                    error!("Invalid tab index: {}", self.current_tab);
                    Ok(None)
                }
            }
        }
    }

    pub async fn run(&mut self) -> Result<(), DiffTuiError> {
        let mut tui = tui::Tui::new()?.tick_rate(4.0).frame_rate(30.0);

        tui.enter()?;

        let exit_msg = 'main: loop {
            if let Some(evt) = tui.next().await {
                if let tui::Event::Render = evt {
                    tui.draw(|f| {
                        trace!("Render cycle starts");
                        #[cfg(debug_assertions)]
                        let render_start = std::time::Instant::now();
                        self.render(f);
                        #[cfg(debug_assertions)]
                        trace!("Render took {:?}", std::time::Instant::now() - render_start);
                        trace!("Render cycle completed");
                        tui::render_complete();
                    })?;
                }
                let mut maybe_action = self.handle_event(evt);
                while let Some(action) = maybe_action {
                    trace!("action = {action:?}");
                    if let Action::ExitApp(msg) = action {
                        self.should_quit = true;
                        break 'main msg;
                    }
                    maybe_action = self.update(action, {
                        let this = &mut tui;
                        &mut this.terminal
                    })?;
                }
            }
        };

        tui.exit()?;

        if let Some(msg) = exit_msg {
            eprintln!("{msg}");
        }

        Ok(())
    }

    fn render_notify(&self, frame: &mut Frame, notify: &Notification) {
        let width = notify
            .title
            .len()
            .max(notify.body.lines().map(|s| s.len()).max().unwrap_or(0))
            .max("Esc / Enter / q to leave".len() + 2)
            + 4;
        let height = notify.body.lines().count() + 2;
        let area = frame.area();
        let buf = frame.buffer_mut();
        let notify_area = area.centered(
            Constraint::Max(width as u16),
            Constraint::Max(height as u16),
        );

        Clear.render(notify_area, buf);
        Paragraph::new(notify.body.as_str())
            .scroll(self.notify_scroll)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().magenta())
                    .title(notify.title.as_str())
                    .title_bottom("Esc / Enter / q to leave"),
            )
            .render(notify_area, buf);
    }
}
