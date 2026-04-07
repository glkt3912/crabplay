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
    let mut error_count = 0usize;
    let tracks: Vec<_> = paths
        .iter()
        .filter_map(|p| match read_metadata(p) {
            Ok(track) => Some(track),
            Err(e) => {
                eprintln!("Warning: failed to read metadata for '{}': {e}", p.display());
                error_count += 1;
                None
            }
        })
        .collect();

    if tracks.is_empty() {
        let detail = if error_count > 0 {
            format!(" ({error_count} file(s) skipped due to errors)")
        } else {
            String::new()
        };
        eprintln!(
            "No MP3/FLAC files found in '{}'{detail}",
            args.dir.display()
        );
        return Ok(());
    }
    if error_count > 0 {
        eprintln!("Warning: {error_count} file(s) could not be read and were skipped");
    }

    if args.list {
        let formatter = make_formatter(&args.format);
        for track in &tracks {
            println!("{}", formatter.format_track(track)?);
        }
        return Ok(());
    }

    let player = Player::new().context("failed to initialize audio")?;
    let canonical_dir = args.dir.canonicalize().unwrap_or_else(|_| args.dir.clone());
    let mut state = AppState::new(tracks, canonical_dir);
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
