pub mod app;
pub mod folder_cmp_view;
pub mod folder_view;
pub mod hex_cmp_view;
pub mod hex_view;
pub mod loading_msg;
pub mod menu;
pub mod text_cmp_view;
pub mod tui;

use std::{io::stdout, path::PathBuf, time::Duration};

use crate::{
    DiffTuiError,
    ui::{
        app::App, folder_cmp_view::FolderCmpState, hex_cmp_view::HexCmpView, hex_view::HexViewTab,
        text_cmp_view::TextCmpView, tui::pause_event_stream,
    },
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
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, BorderType, Clear, Widget},
};
use ratatui_textarea::TextArea;
use regex::bytes::Regex;
use tracing::error;

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
    OpenMenu,
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
    EditSearch(Option<String>),
    SearchNext(Regex),
    SearchPrev(Regex),
    RemoveHighlight,
    SwapSide,
    Reload,
    TabCustomAction,
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
                KeyCode::Char('R') => Ok(Self::Reload),
                KeyCode::Char('O') => Ok(Self::OpenMenu),
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
                KeyCode::Char('/') => Ok(Self::EditSearch(None)),
                KeyCode::Esc => Ok(Self::RemoveHighlight),
                KeyCode::Char('x') => Ok(Self::SwapSide),
                KeyCode::Char('z') => Ok(Self::TabCustomAction),
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
    fn reload(&mut self) -> Result<Option<Box<dyn TabState>>, DiffTuiError>;
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
pub struct JumpToPopup<'a> {
    ta: TextArea<'a>,
}

impl<'a> Default for JumpToPopup<'a> {
    fn default() -> Self {
        let mut ta = TextArea::default();
        ta.set_block(
            Block::bordered()
                .title("Jump to")
                .border_style(Style::default().magenta())
                .border_type(BorderType::Rounded),
        );
        Self { ta }
    }
}

impl<'a> Popup for JumpToPopup<'a> {
    fn handler(&mut self, event: &tui::Event) -> Option<Action> {
        if let tui::Event::Key(key_evt) = event {
            if key_evt.code == KeyCode::Enter {
                return Some(Action::PopupReturn(
                    "JumpTo".to_string(),
                    Some(self.ta.lines()[0].clone()),
                ));
            } else if key_evt.code == KeyCode::Esc {
                return Some(Action::PopupReturn("JumpTo".to_string(), None));
            } else {
                self.ta.input(*key_evt);
            }
        }
        None
    }

    fn render(&mut self, frame: &mut ratatui::prelude::Frame) {
        let (area, buf) = self.prepare(frame, Constraint::Max(20), Constraint::Length(3));
        self.ta.render(area, buf);
    }
}

#[derive(Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

#[derive(Debug, Clone, Copy, Default)]
pub enum OpenWith {
    #[default]
    Auto,
    HexCmpView,
    HexView,
    TextView,
}

pub fn start_tui(lhs: PathBuf, rhs: PathBuf, open_with: OpenWith) -> Result<(), DiffTuiError> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_time()
        .build()
        .unwrap();
    rt.block_on(async move {
        let first_tab: Box<dyn TabState> = if lhs.metadata()?.is_file() {
            match open_with {
                OpenWith::HexCmpView => Box::new(HexCmpView::new(lhs, rhs)?),
                OpenWith::HexView => Box::new(HexViewTab::new(lhs)?),
                _ => Box::new(TextCmpView::new(lhs, rhs)?),
            }
        } else {
            Box::new(FolderCmpState::new(lhs, rhs)?)
        };
        let mut app = App::new(first_tab);

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
