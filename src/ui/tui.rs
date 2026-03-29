use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};
use std::io;

use crate::app::{AppState, PlayerState};
use crate::audio::player::Player;

pub fn run(state: &mut AppState, player: &Player) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, state, player);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    state: &mut AppState,
    player: &Player,
) -> Result<()> {
    let mut list_state = ListState::default();
    list_state.select(Some(state.selected));

    loop {
        terminal.draw(|f| draw(f, state, &mut list_state))?;

        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') => {
                    player.stop();
                    break;
                }
                KeyCode::Up => {
                    state.prev();
                    list_state.select(Some(state.selected));
                }
                KeyCode::Down => {
                    state.next();
                    list_state.select(Some(state.selected));
                }
                KeyCode::Enter => {
                    state.last_error = None;
                    if let Some(track) = state.current() {
                        let path = track.path.clone();
                        match player.load_and_play(&path) {
                            Ok(_) => state.player_state = PlayerState::Playing,
                            Err(e) => state.last_error = Some(e.to_string()),
                        }
                    }
                }
                KeyCode::Char(' ') => {
                    state.last_error = None;
                    player.toggle_pause();
                    state.player_state = if player.is_paused() {
                        PlayerState::Paused
                    } else {
                        PlayerState::Playing
                    };
                }
                KeyCode::Char('n') => {
                    state.last_error = None;
                    state.next();
                    list_state.select(Some(state.selected));
                    if let Some(track) = state.current() {
                        let path = track.path.clone();
                        match player.load_and_play(&path) {
                            Ok(_) => state.player_state = PlayerState::Playing,
                            Err(e) => state.last_error = Some(e.to_string()),
                        }
                    }
                }
                KeyCode::Char('p') => {
                    state.last_error = None;
                    state.prev();
                    list_state.select(Some(state.selected));
                    if let Some(track) = state.current() {
                        let path = track.path.clone();
                        match player.load_and_play(&path) {
                            Ok(_) => state.player_state = PlayerState::Playing,
                            Err(e) => state.last_error = Some(e.to_string()),
                        }
                    }
                }
                _ => {}
            }
        }

        // 再生終了を検知して状態を更新
        if matches!(state.player_state, PlayerState::Playing) && player.is_empty() {
            state.player_state = PlayerState::Stopped;
        }
    }

    Ok(())
}

fn draw(f: &mut ratatui::Frame, state: &AppState, list_state: &mut ListState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    // トラックリスト
    let items: Vec<ListItem> = state
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mins = t.duration_secs / 60;
            let secs = t.duration_secs % 60;
            let artist = if t.artist.is_empty() {
                "Unknown"
            } else {
                &t.artist
            };
            let marker = if i == state.selected { "▶ " } else { "  " };
            let line = Line::from(vec![
                Span::raw(marker),
                Span::styled(
                    format!("{:<30}", &t.title),
                    Style::default().fg(Color::White),
                ),
                Span::styled(format!(" {:<20}", artist), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!(" {:>2}:{:02}", mins, secs),
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" crabplay "))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, chunks[0], list_state);

    // ステータスバー（エラーがある場合は赤色で表示）
    let (status_text, status_color) = if let Some(ref err) = state.last_error {
        (format!(" ⚠ {err}"), Color::Red)
    } else if let Some(track) = state.current() {
        let status = match state.player_state {
            PlayerState::Playing => "▶",
            PlayerState::Paused => "⏸",
            PlayerState::Stopped => "■",
        };
        (
            format!(
                " {} {} — {}  [↑↓] select  [Enter] play  [Space] pause  [n/p] skip  [q] quit",
                status, track.title, track.artist
            ),
            Color::Yellow,
        )
    } else {
        (
            " [↑↓] select  [Enter] play  [q] quit".to_string(),
            Color::Yellow,
        )
    };

    let status = Paragraph::new(status_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(status_color));

    f.render_widget(status, chunks[1]);
}
