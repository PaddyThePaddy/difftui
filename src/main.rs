use clap::Parser;
use difftui::ui::start_tui;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::Level;

mod config;

#[derive(Debug, Clone, clap::Parser)]
struct Cli {
    lhs: PathBuf,
    rhs: PathBuf,
    #[arg(short, long)]
    verbose: bool,
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
    Ok(start_tui(args.lhs, args.rhs)?)
}
