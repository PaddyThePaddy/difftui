mod folder_cmp_view;
mod folder_view;
mod loading_msg;
mod tui;

use std::{io::stdout, path::PathBuf, time::Duration};

use crate::{
    DiffTuiError,
    ui::{
        folder_cmp_view::{FolderCmpState, FolderCmpView},
        tui::pause_event_stream,
    },
};
use crossterm::event::KeyEvent;
use ratatui::{
    Frame,
    crossterm::{
        self, ExecutableCommand as _,
        event::{Event, KeyCode, KeyModifiers},
        terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
    },
    layout::Constraint,
    widgets::{Block, Clear, Paragraph, Widget},
};
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
    LauchExtCompare,
    UncoupleFolders,
    RecoupleFolders,
    OpenSelectedInNewTab,
    ExitApp(Option<String>),
    CompareSelected,
    CompareAll,
    PopupFilter,
    Tick,
    ShowPopup(Box<dyn Popup>),
    PopupReturn(String, String),
    Notification(Notification),
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
                KeyCode::Char('q') => Ok(Self::ExitApp(None)),
                KeyCode::Char('c') => Ok(Self::CompareSelected),
                KeyCode::Char('a') => Ok(Self::CompareAll),
                KeyCode::Char('o') => Ok(Self::LauchExtCompare),
                KeyCode::Char('g') => Ok(Self::NavTop),
                KeyCode::Char('/') => Ok(Self::PopupFilter),
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
    ) -> Result<Option<Action>, DiffTuiError>;
}

pub trait WidgetWithEventHandler: Widget + EventHandler {}

impl<T: Widget + EventHandler> WidgetWithEventHandler for T {}

pub trait Popup: std::fmt::Debug {
    fn handler(&mut self, event: &crate::ui::tui::Event) -> Option<Action>;
    fn render(&self, frame: &mut Frame);
}

#[derive(Debug)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

pub struct App {
    should_quit: bool,
    // tabs: Vec<Box<dyn WidgetWithEventHandler>>,
    // current_tab: Option<usize>,

    // for testing
    folder_cmp_state: FolderCmpState,
    popup: Option<Box<dyn Popup>>,
    showing_notify: Option<Notification>,
}

impl App {
    fn render(&mut self, frame: &mut Frame) {
        // frame.render_stateful_widget(FolderView::default(), frame.area(), &mut self.folder_state);
        FolderCmpView::default().render(frame, &mut self.folder_cmp_state);

        if let Some(notify) = &self.showing_notify {
            self.render_notify(frame, notify);
        } else if let Some(popup) = &self.popup {
            popup.render(frame);
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
        match act {
            Action::ShowPopup(popup) => {
                self.popup = Some(popup);
                return Ok(None);
            }
            Action::Notification(notify) => {
                self.showing_notify = Some(notify);
                return Ok(None);
            }
            _ => self.folder_cmp_state.handler(&act, terminal),
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
                        self.render(f);
                        trace!("Render cycle completed");
                        tui::render_complete();
                    })?;
                }
                let mut maybe_action = self.handle_event(evt);
                while let Some(action) = maybe_action {
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
        let mut app = App {
            folder_cmp_state: FolderCmpState::new(lhs, rhs).unwrap(),
            should_quit: false,
            popup: None,
            showing_notify: None,
        };
        app.run().await.unwrap();
    });
    Ok(())
}

pub fn run_ext_tui_app(
    cmd: &mut std::process::Command,
    terminal: &mut TuiTerminal,
) -> std::io::Result<()> {
    let _event_blocker = pause_event_stream();
    stdout().execute(LeaveAlternateScreen)?;
    disable_raw_mode()?;
    cmd.status()?;
    if let Err(e) = stdout().execute(EnterAlternateScreen) {
        eprintln!("Run command {:?} failed: {e}", cmd);
        error!("Run command {:?} failed: {e}", cmd);
    }
    enable_raw_mode()?;
    terminal.clear()?;
    while crossterm::event::poll(Duration::from_millis(10))? {
        let _ = crossterm::event::read()?;
    }
    Ok(())
}
