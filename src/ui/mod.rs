mod folder_cmp_view;
mod folder_view;
mod tui;

use std::{
    io::stdout,
    iter::Once,
    ops::{Deref, DerefMut as _},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};

use crate::{
    DiffTuiError,
    ui::{
        folder_cmp_view::{FolderCmpState, FolderCmpView},
        tui::Tui,
    },
};
use crossterm::event::KeyEvent;
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        self, ExecutableCommand as _,
        event::{Event, KeyCode, KeyModifiers},
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    widgets::Widget,
};
use tracing::{error, info, trace};

pub type TuiTerminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stderr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    LauchExtCompare,
    UncoupleFolders,
    RecoupleFolders,
    OpenSelectedInNewTab,
    ExitApp,
    CompareSelected,
    CompareAll,
    NoOp,
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
                KeyCode::Char('c') => Ok(Self::ExitApp),
                _ => Err(()),
            }
        } else if value.modifiers == KeyModifiers::SHIFT {
            match value.code {
                KeyCode::Char('G') => Ok(Self::NavBottom),
                _ => Err(()),
            }
        } else if value.modifiers.is_empty() {
            match value.code {
                KeyCode::Char('j') | KeyCode::Down => Ok(Self::NavDown),
                KeyCode::Char('k') | KeyCode::Up => Ok(Self::NavUp),
                KeyCode::Char('h') | KeyCode::Left => Ok(Self::NavLeft),
                KeyCode::Char('l') | KeyCode::Right => Ok(Self::NavRight),
                KeyCode::Char('q') => Ok(Self::ExitApp),
                KeyCode::Char('c') => Ok(Self::CompareSelected),
                KeyCode::Char('a') => Ok(Self::CompareAll),
                KeyCode::Char('o') => Ok(Self::LauchExtCompare),
                KeyCode::Char('g') => Ok(Self::NavTop),
                KeyCode::Enter => Ok(Self::ToggleSelected),
                _ => Err(()),
            }
        } else {
            Err(())
        }
    }
}

pub trait EventHandler {
    fn handler(
        &mut self,
        event: &Action,
        terminal: &mut TuiTerminal,
    ) -> Result<(), DiffTuiError>;
}

pub trait WidgetWithEventHandler: Widget + EventHandler {}

impl<T: Widget + EventHandler> WidgetWithEventHandler for T {}

pub struct App {
    should_quit: bool,
    // tabs: Vec<Box<dyn WidgetWithEventHandler>>,
    // current_tab: Option<usize>,

    // for testing
    folder_cmp_state: FolderCmpState,
}

impl App {
    pub fn app_main(&mut self, term: &mut TuiTerminal) -> std::io::Result<()> {
        loop {
            term.draw(|frame: &mut Frame| {
                self.render(frame);
            })?;
            if crossterm::event::poll(Duration::new(0, 100))? {
                let event = crossterm::event::read()?;
                if let Ok(event) = Action::try_from(&event) {
                    if event == Action::ExitApp {
                        break Ok(());
                    }
                    self.folder_cmp_state.handler(&event, term).unwrap();
                }
            } else {
                self.folder_cmp_state
                    .handler(&Action::NoOp, term)
                    .unwrap();
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        // frame.render_stateful_widget(FolderView::default(), frame.area(), &mut self.folder_state);
        frame.render_stateful_widget(
            FolderCmpView::default(),
            frame.area(),
            &mut self.folder_cmp_state,
        );
    }

    fn handle_event(&mut self, evt: tui::Event) -> Option<Action> {
        if let tui::Event::Key(k_evt) = evt {
            Action::try_from(k_evt).ok()
        } else if let tui::Event::Tick = evt {
            Some(Action::NoOp)
        } else {
            None
        }
    }

    fn update(
        &mut self,
        act: Action,
        terminal: &mut TuiTerminal,
    ) -> Result<Option<Action>, DiffTuiError> {
        self.folder_cmp_state.handler(&act, terminal)?;
        Ok(None)
    }

    async fn run(&mut self) -> Result<(), DiffTuiError> {
        let mut tui = tui::Tui::new()?.tick_rate(4.0).frame_rate(30.0);

        tui.enter()?;

        loop {
            tui.draw(|f| {
                self.render(f);
            })?;

            if let Some(evt) = tui.next().await {
                let mut maybe_action = self.handle_event(evt);
                while let Some(action) = maybe_action {
                    if action == Action::ExitApp {
                        self.should_quit = true;
                    }
                    maybe_action = self.update(action, {
                        let this = &mut tui;
                        &mut this.terminal
                    })?;
                }
            }

            if self.should_quit {
                break;
            }
        }

        tui.exit()?;

        Ok(())
    }
}

pub fn start_tui(lhs: PathBuf, rhs: PathBuf) -> Result<(), DiffTuiError> {
    let rt = tokio::runtime::Builder::new_multi_thread().enable_time().build().unwrap();
    rt.block_on(async move {
        let mut app = App {
            folder_cmp_state: FolderCmpState::new(lhs, rhs).unwrap(),
            should_quit: false,
        };
        app.run().await.unwrap();
    });
    Ok(())
}

pub fn run_ext_tui_app(
    cmd: &mut std::process::Command,
    terminal: &mut TuiTerminal,
) -> std::io::Result<()> {
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    cmd.status()?;
    if let Err(e) = stdout().execute(EnterAlternateScreen) {
        eprintln!("Run command {:?} failed: {e}", cmd);
        error!("Run command {:?} failed: {e}", cmd);
    }
    enable_raw_mode()?;
    terminal.clear()?;
    Ok(())
}
