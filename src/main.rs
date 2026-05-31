use clap::{ArgAction, Parser};
use crossterm::event::{KeyCode, KeyModifiers};
use difftui::ui::hex_view::HexViewState;
use difftui::ui::{OpenWith, start_tui};
use ratatui::widgets::StatefulWidget;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::Level;

mod config;

#[derive(Debug, Clone, clap::Parser)]
struct Cli {
    lhs: PathBuf,
    rhs: PathBuf,
    #[arg(short('x'), long, action=ArgAction::Count)]
    hex: u8,
    #[arg(short, long)]
    verbose: bool,
}

// #[derive(Debug, Clone, clap::Parser)]
// struct TestCli {
//     bin: PathBuf,
//     #[arg(short, long)]
//     verbose: bool,
// }

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();
    if args.verbose {
        let log_file = File::create("logs.txt")?;
        tracing_subscriber::fmt()
            .with_max_level(Level::TRACE)
            .with_ansi(false)
            .with_writer(Mutex::new(log_file))
            .init();
    }
    if args.hex > 1 {
        return hex_view(&args.lhs);
    }
    Ok(start_tui(
        args.lhs,
        args.rhs,
        if args.hex != 0 {
            OpenWith::HexView
        } else {
            OpenWith::default()
        },
    )?)
}

fn hex_view(path: &Path) -> anyhow::Result<()> {
    // let args = TestCli::parse();
    ratatui::run(|terminal| {
        let buf = std::fs::read(path)?;
        let mut state = HexViewState::default().with_selected(Some(0));
        loop {
            terminal.draw(|frame| {
                StatefulWidget::render(
                    difftui::ui::hex_view::HexView::new(&buf),
                    frame.area(),
                    frame.buffer_mut(),
                    &mut state,
                );
            })?;
            if let Some(evt) = crossterm::event::read()?.as_key_press_event() {
                match evt.code {
                    KeyCode::Up | KeyCode::Char('k') => state.move_sel_up(),
                    KeyCode::Down | KeyCode::Char('j') => state.move_sel_down(),
                    KeyCode::Left | KeyCode::Char('h') => state.move_sel_left(),
                    KeyCode::Right | KeyCode::Char('l') => state.move_sel_right(),
                    KeyCode::PageUp => state.move_sel_up_page(0.5),
                    KeyCode::Char('d') if evt.modifiers == KeyModifiers::CONTROL => {
                        state.move_sel_down_page(0.5)
                    }
                    KeyCode::Char('u') if evt.modifiers == KeyModifiers::CONTROL => {
                        state.move_sel_up_page(0.5)
                    }
                    KeyCode::Char('f') if evt.modifiers == KeyModifiers::CONTROL => {
                        state.move_sel_down_page(1.0)
                    }
                    KeyCode::Char('b') if evt.modifiers == KeyModifiers::CONTROL => {
                        state.move_sel_up_page(1.0)
                    }
                    KeyCode::PageDown => state.move_sel_down_page(1.0),
                    KeyCode::Char('q') => break,
                    _ => {}
                }
            }
        }
        Ok(())
    })
}
