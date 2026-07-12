use clap::{ArgAction, Parser};
use difftui::DiffTuiConfig;
use difftui::ui::{OpenWith, start_tui};
use std::fs::File;
use std::path::PathBuf;
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
    #[arg(long)]
    no_ignore: bool,
    #[arg(long)]
    no_git_ignore: bool,
    #[arg(long)]
    hidden: bool,
    #[arg(long, action=ArgAction::Append)]
    additional_ignore: Vec<PathBuf>,
}

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
    Ok(start_tui(
        args.lhs,
        args.rhs,
        if args.hex == 2 {
            OpenWith::HexView
        } else if args.hex != 0 {
            OpenWith::HexCmpView
        } else {
            OpenWith::default()
        },
        &DiffTuiConfig {
            no_ignore: args.no_ignore,
            no_git_ignore: args.no_git_ignore,
            additional_ignore: args.additional_ignore,
            hidden: args.hidden,
        },
    )?)
}
