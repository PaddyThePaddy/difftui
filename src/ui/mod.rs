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
use globset::{Glob, GlobSetBuilder};
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
use ratatui_textarea::{CursorMove, TextArea};
use tracing::{error, trace};

pub type TuiTerminal = ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>;

#[derive(Debug, Clone, Default)]
enum PopupState {
    #[default]
    None,
    FilterEditor(Vec<String>),
    Alert(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

pub struct App<'a> {
    should_quit: bool,
    // tabs: Vec<Box<dyn WidgetWithEventHandler>>,
    // current_tab: Option<usize>,

    // for testing
    folder_cmp_state: FolderCmpState,
    popup: PopupState,
    filter_text: TextArea<'a>,
}

impl<'a> App<'a> {
    pub fn app_main(&mut self, term: &mut TuiTerminal) -> std::io::Result<()> {
        loop {
            term.draw(|frame: &mut Frame| {
                self.render(frame);
            })?;
            if crossterm::event::poll(Duration::new(0, 100))? {
                let event = crossterm::event::read()?;
                if let Ok(event) = Action::try_from(&event) {
                    if event == Action::ExitApp(None) {
                        break Ok(());
                    }
                    self.folder_cmp_state.handler(&event, term).unwrap();
                }
            } else {
                self.folder_cmp_state.handler(&Action::Tick, term).unwrap();
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        // frame.render_stateful_widget(FolderView::default(), frame.area(), &mut self.folder_state);
        FolderCmpView::default().render(frame, &mut self.folder_cmp_state);
        self.render_popup(frame);
    }

    fn handle_event(&mut self, evt: tui::Event) -> Option<Action> {
        match &mut self.popup {
            PopupState::None => {}
            PopupState::FilterEditor(old_text) => {
                if let tui::Event::Key(key_evt) = evt {
                    if key_evt.modifiers == KeyModifiers::CONTROL
                        && key_evt.code == KeyCode::Char('s')
                    {
                        let filter_text_lines = self.filter_text.lines();
                        let mut glob_builder = GlobSetBuilder::new();

                        for line in filter_text_lines {
                            let line = line.trim();
                            if line.is_empty() {
                                continue;
                            }
                            let glob = match Glob::new(line) {
                                Ok(g) => g,
                                Err(e) => {
                                    self.popup =
                                        PopupState::Alert(format!("Parsing glob failed: {e}"));
                                    return None;
                                }
                            };
                            glob_builder.add(glob);
                        }
                        let filters = match glob_builder.build() {
                            Ok(f) => f,
                            Err(e) => {
                                self.popup =
                                    PopupState::Alert(format!("Build glob set failed: {e}"));
                                return None;
                            }
                        };
                        self.folder_cmp_state.set_filters(filters);
                        self.popup = PopupState::None;
                    } else if key_evt.code == KeyCode::Esc {
                        self.filter_text = build_filter_text_area(Some(old_text.clone()));
                        self.popup = PopupState::None;
                    } else if key_evt.code == KeyCode::Char('d')
                        && key_evt.modifiers == KeyModifiers::CONTROL
                    {
                        let mut lines = self.filter_text.lines().to_vec();
                        let cursor = self.filter_text.cursor();
                        lines.remove(cursor.0);
                        let mut new_area = build_filter_text_area(Some(lines));
                        new_area.move_cursor(CursorMove::Jump(cursor.0 as u16, cursor.1 as u16));
                        self.filter_text = new_area;
                    } else {
                        self.filter_text.input(key_evt);
                    }
                    return None;
                }
            }
            PopupState::Alert(_) => {
                if let tui::Event::Key(key_evt) = evt {
                    if key_evt.code == KeyCode::Enter
                        || key_evt.code == KeyCode::Esc
                        || key_evt.code == KeyCode::Char('q')
                    {
                        self.popup = PopupState::None;
                    }
                }
                return None;
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
        match act {
            Action::PopupFilter => {
                self.popup = PopupState::FilterEditor(self.filter_text.lines().to_vec());
                Ok(None)
            }
            _ => self.folder_cmp_state.handler(&act, terminal),
        }
    }

    async fn run(&mut self) -> Result<(), DiffTuiError> {
        let mut tui = tui::Tui::new()?.tick_rate(4.0).frame_rate(30.0);

        tui.enter()?;

        let exit_msg = 'main: loop {
            tui.draw(|f| {
                trace!("Render cycle starts");
                self.render(f);
                trace!("Render cycle completed");
            })?;

            if let Some(evt) = tui.next().await {
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

    fn render_popup(&self, frame: &mut Frame) {
        let area = frame.area();
        let buf = frame.buffer_mut();
        let popup_area = area.centered(Constraint::Percentage(80), Constraint::Percentage(80));

        match &self.popup {
            PopupState::FilterEditor(_) => {
                Clear::default().render(popup_area, buf);
                self.filter_text.render(popup_area, buf);
            }
            PopupState::Alert(msg) => {
                Clear::default().render(popup_area, buf);
                Paragraph::new(msg.as_str())
                    .block(Block::bordered().title("Error"))
                    .render(popup_area, buf);
            }
            PopupState::None => {}
        }
    }
}

fn build_filter_text_area<'a>(text: Option<Vec<String>>) -> TextArea<'a> {
    let mut default_text_area = if let Some(text) = text {
        TextArea::new(text)
    } else {
        TextArea::default()
    };
    default_text_area.set_block(
        Block::bordered()
            .title("Filters")
            .title_bottom("<ESC> to cancel / <Ctrl-S> to confirm"),
    );
    default_text_area
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
            popup: PopupState::default(),
            filter_text: build_filter_text_area(None),
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
    Ok(())
}
