mod folder_cmp_view;
mod folder_view;
mod loading_msg;
mod menu;
mod text_cmp_view;
mod tui;

use std::{io::stdout, path::PathBuf, time::Duration};

use crate::{
    DiffTuiError,
    ui::{folder_cmp_view::FolderCmpState, text_cmp_view::TextCmpView, tui::pause_event_stream},
};
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    buffer::Buffer,
    crossterm::{
        self, ExecutableCommand as _,
        event::{Event, KeyCode, KeyModifiers},
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::{Constraint, Direction, Layout, Rect},
    text::Span,
    widgets::{Block, Clear, Paragraph, Tabs, Widget},
};
use regex::{Regex, RegexBuilder};
use tracing::{error, trace};

pub type TuiTerminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>;

#[derive(Debug)]
pub enum Action {
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
    NavTop,
    NavBottom,
    ExpandSelected,
    CollapseSelected,
    ToggleSelected,
    ToggleCoupling,
    OpenSelectedInNewTab,
    ExitApp(Option<String>),
    Compare,
    PopupFilter,
    Tick,
    Open,
    ShowPopup(Box<dyn Popup>),
    PopupReturn(String, Option<String>),
    Notification(Notification),
    CreateTabAndSwitch(Box<dyn TabState>),
    RunExtApp(std::process::Command),
    ExtAppReturn(Option<i32>),
    NextTab,
    PrevTab,
    CloseTab,
    NextDiff,
    PrevDiff,
    ShowHelp,
    PageUp(f32),
    PageDown(f32),
    EditSearch,
    SearchNext(Regex),
    SearchPrev(Regex),
    RemoveHighlight,
}

impl TryFrom<&Event> for Action {
    type Error = ();
    fn try_from(value: &Event) -> Result<Self, Self::Error> {
        if let Some(kp_ev) = value.as_key_press_event() {
            kp_ev.try_into()
        } else {
            Err(())
        }
    }
}

impl TryFrom<KeyEvent> for Action {
    type Error = ();
    fn try_from(value: KeyEvent) -> Result<Self, Self::Error> {
        if value.modifiers == KeyModifiers::CONTROL {
            match value.code {
                KeyCode::Char('c') => Ok(Self::ExitApp(None)),
                KeyCode::Char('d') => Ok(Self::PageDown(0.5)),
                KeyCode::Char('u') => Ok(Self::PageUp(0.5)),
                KeyCode::Char('f') => Ok(Self::PageDown(1.0)),
                KeyCode::Char('b') => Ok(Self::PageUp(1.0)),
                _ => Err(()),
            }
        } else if value.modifiers == KeyModifiers::SHIFT {
            match value.code {
                KeyCode::Char('G') => Ok(Self::NavBottom),
                KeyCode::BackTab => Ok(Self::PrevTab),
                _ => Err(()),
            }
        } else if value.modifiers.is_empty() {
            match value.code {
                KeyCode::Char('j') | KeyCode::Down => Ok(Self::NavDown),
                KeyCode::Char('k') | KeyCode::Up => Ok(Self::NavUp),
                KeyCode::Char('h') | KeyCode::Left => Ok(Self::NavLeft),
                KeyCode::Char('l') | KeyCode::Right => Ok(Self::NavRight),
                KeyCode::Char('q') => Ok(Self::CloseTab),
                KeyCode::Char('c') => Ok(Self::Compare),
                KeyCode::Char('o') => Ok(Self::Open),
                KeyCode::Char('g') => Ok(Self::NavTop),
                KeyCode::Char('f') => Ok(Self::PopupFilter),
                KeyCode::Char('=') => Ok(Self::ToggleCoupling),
                KeyCode::Enter => Ok(Self::ToggleSelected),
                KeyCode::Tab => Ok(Self::NextTab),
                KeyCode::Char(']') => Ok(Self::NextDiff),
                KeyCode::Char('[') => Ok(Self::PrevDiff),
                KeyCode::Char('?') | KeyCode::F(1) => Ok(Self::ShowHelp),
                KeyCode::Char('/') => Ok(Self::EditSearch),
                KeyCode::Esc => Ok(Self::RemoveHighlight),
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}

pub trait EventHandler {
    fn handler(&mut self, event: &Action) -> Result<Option<Action>, DiffTuiError>;
}

pub trait TabState: EventHandler + std::fmt::Debug {
    fn title(&self) -> String;
    fn render(&mut self, area: Rect, buf: &mut Buffer);
}

pub trait Popup: std::fmt::Debug {
    fn handler(&mut self, event: &crate::ui::tui::Event) -> Option<Action>;
    fn render(&mut self, frame: &mut Frame);
    fn prepare<'a>(
        &self,
        frame: &'a mut Frame,
        hor: Constraint,
        ver: Constraint,
    ) -> (Rect, &'a mut Buffer) {
        let area = frame.area();
        let buf = frame.buffer_mut();
        let popup_area = area.centered(hor, ver);
        Clear::default().render(popup_area, buf);
        (popup_area, buf)
    }
}

#[derive(Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

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
        if let Self::None = self { true } else { false }
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
}

impl App {
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
        if let Some(_) = &self.showing_notify {
            if let tui::Event::Key(key) = evt {
                if key.code == KeyCode::Enter || key.code == KeyCode::Esc {
                    self.showing_notify = None;
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
                    match RegexBuilder::new(s)
                        .case_insensitive(s.chars().all(|c| c.is_lowercase()))
                        .build()
                    {
                        Ok(r) => {
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
                return Ok(None);
            }
            Action::Notification(notify) => {
                self.showing_notify = Some(notify);
                return Ok(None);
            }
            Action::PrevTab => {
                if self.current_tab == 0 {
                    if self.tabs.len() > 0 {
                        self.current_tab = self.tabs.len() - 1;
                    }
                } else {
                    self.current_tab -= 1;
                }
                return Ok(None);
            }
            Action::NextTab => {
                self.current_tab = self.current_tab.wrapping_add(1);
                if self.current_tab == self.tabs.len() {
                    self.current_tab = 0;
                }
                return Ok(None);
            }
            Action::CreateTabAndSwitch(new_tab) => {
                self.tabs.push(new_tab);
                self.current_tab = self.tabs.len() - 1;
                return Ok(None);
            }
            Action::CloseTab => {
                self.tabs.remove(self.current_tab);
                if self.tabs.is_empty() {
                    return Ok(Some(Action::ExitApp(Some("All tabs closed".to_string()))));
                }
                if self.current_tab >= self.tabs.len() {
                    self.current_tab = self.tabs.len() - 1;
                }
                return Ok(None);
            }
            Action::RunExtApp(mut cmd) => {
                let return_code = run_ext_tui_app(&mut cmd, terminal)?;
                return Ok(Some(Action::ExtAppReturn(return_code)));
            }
            Action::ShowHelp => {
                return Ok(Some(Action::Notification(Notification {
                    title: "Help".to_string(),
                    body: vec![
                        "Arrow keys / hjkl => Navigation",
                        "q        => Closet tab",
                        "c        => Compare selected file/folder",
                        "o        => Open selected filde / folder",
                        "g        => Move to top",
                        "G        => Move to bottom",
                        "/        => Filter files",
                        "][       => Next/Previous difference",
                        "=        => Decouple side-by-side view",
                        "Enter    => Expand/Collapse",
                        "Ctrl + c => Exit app",
                        "? / F1   => Show help",
                    ]
                    .join("\n"),
                })));
            }
            Action::EditSearch => {
                self.search_state = SearchState::Editing(String::new());
                return Ok(None);
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

    async fn run(&mut self) -> Result<(), DiffTuiError> {
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
        let area = frame.area();
        let buf = frame.buffer_mut();
        let notify_area = area.centered(Constraint::Percentage(50), Constraint::Percentage(50));

        Clear::default().render(notify_area, buf);
        Paragraph::new(notify.body.as_str())
            .block(Block::bordered().title(notify.title.as_str()))
            .render(notify_area, buf);
    }
}

pub fn start_tui(lhs: PathBuf, rhs: PathBuf) -> Result<(), DiffTuiError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async move {
        let first_tab: Box<dyn TabState> = if lhs.metadata()?.is_file() {
            Box::new(TextCmpView::new(lhs, rhs)?)
        } else {
            Box::new(FolderCmpState::new(lhs, rhs)?)
        };
        let mut app = App {
            tabs: vec![first_tab],
            current_tab: 0,
            should_quit: false,
            popup: None,
            showing_notify: None,
            search_state: SearchState::default(),
        };
        app.run().await.unwrap();
        Ok::<(), DiffTuiError>(())
    })?;
    Ok(())
}

pub fn run_ext_tui_app(
    cmd: &mut std::process::Command,
    terminal: &mut TuiTerminal,
) -> std::io::Result<Option<i32>> {
    let _event_blocker = pause_event_stream();
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    let status = cmd.status()?;
    if let Err(e) = stdout().execute(EnterAlternateScreen) {
        eprintln!("Run command {:?} failed: {e}", cmd);
        error!("Run command {:?} failed: {e}", cmd);
    }
    enable_raw_mode()?;
    terminal.clear()?;
    while crossterm::event::poll(Duration::from_millis(10))? {
        let _ = crossterm::event::read()?;
    }
    Ok(status.code())
}
