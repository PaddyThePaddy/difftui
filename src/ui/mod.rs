mod folder_cmp_view;
mod folder_view;

use std::{io::stdout, iter::Once, path::{Path, PathBuf}, sync::OnceLock, time::Duration};

use crate::{
    DiffTuiError,
    ui::folder_cmp_view::{FolderCmpState, FolderCmpView},
};
use ratatui::{
    DefaultTerminal, Frame,
    crossterm::{
        self, ExecutableCommand as _, event::{Event, KeyCode, KeyModifiers}, terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode}
    },
    widgets::Widget,
};
use tracing::{error, info};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlEvent {
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
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

impl TryFrom<&Event> for ControlEvent {
    type Error = ();
    fn try_from(value: &Event) -> Result<Self, Self::Error> {
        if let Some(kp_ev) = value.as_key_press_event() {
            if kp_ev.modifiers == KeyModifiers::CONTROL {
                match kp_ev.code {
                    KeyCode::Char('c') => Ok(Self::ExitApp),
                    _ => Err(()),
                }
            } else if kp_ev.modifiers.is_empty() {
                match kp_ev.code {
                    KeyCode::Char('j') | KeyCode::Down => Ok(Self::NavDown),
                    KeyCode::Char('k') | KeyCode::Up => Ok(Self::NavUp),
                    KeyCode::Char('h') | KeyCode::Left => Ok(Self::NavLeft),
                    KeyCode::Char('l') | KeyCode::Right => Ok(Self::NavRight),
                    KeyCode::Char('q') => Ok(Self::ExitApp),
                    KeyCode::Char('c') => Ok(Self::CompareSelected),
                    KeyCode::Char('a') => Ok(Self::CompareAll),
                    KeyCode::Char('o') => Ok(Self::LauchExtCompare),
                    KeyCode::Enter => Ok(Self::ToggleSelected),
                    _ => Err(()),
                }
            } else {
                Err(())
            }
        } else {
            Err(())
        }
    }
}

pub trait EventHandler {
    fn handler(&mut self, event: &ControlEvent, terminal: &mut DefaultTerminal) -> Result<(), DiffTuiError>;
}

pub trait WidgetWithEventHandler: Widget + EventHandler {}

impl<T: Widget + EventHandler> WidgetWithEventHandler for T {}

pub struct App {
    tabs: Vec<Box<dyn WidgetWithEventHandler>>,
    current_tab: Option<usize>,

    // for testing
    folder_cmp_state: FolderCmpState,
}

impl App {
    pub fn app_main(&mut self, term: &mut DefaultTerminal) -> std::io::Result<()> {
        loop {
            term.draw(|frame: &mut Frame| {
                self.render(frame);
            })?;
            if crossterm::event::poll(Duration::new(0, 100))? {
                let event = crossterm::event::read()?;
                if let Ok(event) = ControlEvent::try_from(&event) {
                    if event == ControlEvent::ExitApp {
                        break Ok(());
                    }
                    self.folder_cmp_state.handler(&event, term).unwrap();
                }
            } else {
                self.folder_cmp_state.handler(&ControlEvent::NoOp, term).unwrap();
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
}

static LHS: OnceLock<PathBuf> = OnceLock::new();
static RHS: OnceLock<PathBuf> = OnceLock::new();
fn app_wrapper(term: &mut DefaultTerminal) -> std::io::Result<()> {
    let mut app = App {
        tabs: vec![],
        current_tab: None,
        folder_cmp_state: FolderCmpState::new(
            LHS.get().unwrap(),
            RHS.get().unwrap(),
        )
        .unwrap(),
    };

    app.app_main(term)
}

pub fn start_tui(lhs: PathBuf, rhs:PathBuf) -> Result<(), DiffTuiError> {
    LHS.set(lhs).unwrap();
    RHS.set(rhs).unwrap();
    ratatui::run(app_wrapper)?;
    Ok(())
}

pub fn run_ext_tui_app(cmd: &mut std::process::Command, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
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
