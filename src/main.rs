use anyhow::{Context, Result};
use clap::Parser;

use crabplay::audio::player::Player;
use crabplay::cli::Args;
use crabplay::library::{metadata::read_metadata, scanner::scan_directory};
use crabplay::output::make_formatter;
use crabplay::{app::AppState, ui::tui};

fn run(args: &Args) -> Result<()> {
    args.validate().context("argument validation failed")?;

    let paths = scan_directory(&args.dir).context("directory scan failed")?;
    let tracks: Vec<_> = paths
        .iter()
        .filter_map(|p| read_metadata(p).ok())
        .collect();

    if tracks.is_empty() {
        eprintln!("No MP3/FLAC files found in '{}'", args.dir.display());
        return Ok(());
    }

    if args.list {
        let formatter = make_formatter(&args.format);
        for track in &tracks {
            println!("{}", formatter.format_track(track)?);
        }
        return Ok(());
    }

    let player = Player::new().context("failed to initialize audio")?;
    let mut state = AppState::new(tracks);
    tui::run(&mut state, &player).context("TUI error")?;

    Ok(())
}

fn main() {
    let args = Args::parse();

    if let Err(err) = run(&args) {
        eprintln!("[error] {err:#}");
        std::process::exit(1);
    }
}
