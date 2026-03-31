use anyhow::Result;
use crossterm::{
    cursor,
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
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
};
use std::io;
use unicode_width::UnicodeWidthStr;

use crate::app::{AppState, PlayerState};
use crate::audio::player::Player;
use crate::playlist::Playlist;

/// パニック時も含めてターミナルを必ず復元するガード型。
struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
    }
}

pub fn run(state: &mut AppState, player: &Player) -> Result<()> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen)?;

    // _guard がスコープを抜けると（正常終了・エラー・パニック問わず）
    // Drop が呼ばれてターミナルが復元される
    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    let result = event_loop(&mut terminal, state, player);

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

    // マーキー用: フレームカウンタとオフセット
    let mut marquee_tick: u32 = 0;
    let mut marquee_offset: usize = 0;
    let mut last_selected = state.selected;

    loop {
        terminal.draw(|f| draw(f, state, player, &mut list_state, marquee_offset))?;

        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            state.clear_messages();
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
                    play_current(state, player);
                }
                KeyCode::Char(' ') => {
                    player.toggle_pause();
                    if player.is_paused() {
                        state.set_paused();
                    } else {
                        state.set_resumed();
                    }
                }
                KeyCode::Char('n') => {
                    state.next();
                    list_state.select(Some(state.selected));
                    play_current(state, player);
                }
                KeyCode::Char('p') => {
                    state.prev();
                    list_state.select(Some(state.selected));
                    play_current(state, player);
                }
                // キューに選択中のトラックを追加
                KeyCode::Char('a') => {
                    state.enqueue_selected();
                    state.info_msg = Some(format!("Queued: {} track(s)", state.queue_len()));
                }
                // キューをクリア
                KeyCode::Char('c') => {
                    state.clear_queue();
                    state.info_msg = Some("Queue cleared".to_string());
                }
                // リピートモードをサイクル
                KeyCode::Char('r') => {
                    state.cycle_repeat();
                    state.info_msg = Some(format!("Repeat: {}", state.repeat.label()));
                }
                // キュー（空の場合は全トラック）をプレイリストとして保存
                KeyCode::Char('s') => {
                    save_playlist(state);
                }
                _ => {}
            }
        }

        // 選択が変わったらマーキーをリセット
        if state.selected != last_selected {
            marquee_offset = 0;
            marquee_tick = 0;
            last_selected = state.selected;
        }

        // 5フレーム（約1秒）ごとにマーキーを1文字進める
        marquee_tick += 1;
        if marquee_tick >= 5 {
            marquee_tick = 0;
            marquee_offset += 1;
        }

        // rodio::Sink::empty() == true → 再生バッファが空 → トラック再生完了
        if matches!(state.player_state(), PlayerState::Playing) && player.is_empty() {
            state.clear_messages();
            if state.advance() {
                list_state.select(Some(state.selected));
                marquee_offset = 0;
                marquee_tick = 0;
                last_selected = state.selected;
                play_current(state, player);
            } else {
                state.set_stopped();
            }
        }
    }

    Ok(())
}

fn play_current(state: &mut AppState, player: &Player) {
    state.last_error = None;
    if let Some(track) = state.tracks.get(state.selected) {
        let path = track.path.clone();
        match player.load_and_play(&path) {
            Ok(_) => state.set_playing(),
            Err(e) => {
                state.last_error = Some(e.to_string());
                state.set_stopped();
            }
        }
    }
}

fn save_playlist(state: &mut AppState) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_secs();
    let name = format!("playlist_{ts}");

    let paths = if state.queue_is_empty() {
        state.tracks.iter().map(|t| t.path.clone()).collect()
    } else {
        state.queue_paths()
    };

    let playlist = Playlist::new(&name, paths);
    match playlist.save(&Playlist::default_dir()) {
        Ok(dest) => state.info_msg = Some(format!("Saved: {}", dest.display())),
        Err(e) => state.last_error = Some(e.to_string()),
    }
}

const BADGE_WIDTH: usize = 6;

/// キューの位置番号リストをバッジ文字列（BADGE_WIDTH 文字固定）に変換する。
fn format_queue_badge(positions: &[usize]) -> String {
    let s = match positions {
        [] => return " ".repeat(BADGE_WIDTH),
        [p] => format!("[{}]", p),
        [p1, p2] if *p1 < 10 && *p2 < 10 => format!("[{},{}]", p1, p2),
        [p1, rest @ ..] => format!("[{}+{}]", p1, rest.len()),
    };
    format!("{:<width$}", s, width = BADGE_WIDTH)
}

/// 文字列を表示幅ベースでスクロールし、max_width 幅に収めて返す。
fn marquee_slice(s: &str, offset: usize, max_width: usize) -> String {
    // 文字の配列（char 単位）
    let chars: Vec<char> = s.chars().collect();
    let total = chars.len();
    if total == 0 {
        return String::new();
    }

    let mut result = String::new();
    let mut width = 0usize;
    let mut idx = offset % (total + 2); // 末尾に少し空白を挟んでループ

    while width < max_width {
        if idx >= total {
            // 末尾の空白パディング部分
            result.push(' ');
            width += 1;
            idx = (idx + 1) % (total + 2);
        } else {
            let ch = chars[idx];
            let mut buf = [0u8; 4];
            let ch_str: &str = ch.encode_utf8(&mut buf);
            let ch_width = UnicodeWidthStr::width(ch_str);
            if width + ch_width > max_width {
                break;
            }
            result.push(ch);
            width += ch_width;
            idx = (idx + 1) % (total + 2);
        }
    }

    // 全角文字が境界で収まらなかった場合など、max_width に満たない分を空白で埋める
    while UnicodeWidthStr::width(result.as_str()) < max_width {
        result.push(' ');
    }

    result
}

fn draw(
    f: &mut ratatui::Frame,
    state: &AppState,
    player: &Player,
    list_state: &mut ListState,
    marquee_offset: usize,
) {
    // 3分割レイアウト: トラックリスト / 再生情報 / キーバインド
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
            Constraint::Length(3),
        ])
        .split(f.area());

    // タイトル列・アーティスト列・キューバッジの表示幅
    const TITLE_WIDTH: usize = 30;
    const ARTIST_WIDTH: usize = 20;

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
            let marker = if state.playing_index() == Some(i) {
                "▶ "
            } else {
                "  "
            };

            let (title_str, artist_str) = if i == state.selected {
                // 選択中: 表示幅を超える場合にマーキー
                let title_disp = if UnicodeWidthStr::width(t.title.as_str()) > TITLE_WIDTH {
                    marquee_slice(&t.title, marquee_offset, TITLE_WIDTH)
                } else {
                    format!("{:<width$}", t.title, width = TITLE_WIDTH)
                };
                let artist_disp = if UnicodeWidthStr::width(artist) > ARTIST_WIDTH {
                    marquee_slice(artist, marquee_offset, ARTIST_WIDTH)
                } else {
                    format!("{:<width$}", artist, width = ARTIST_WIDTH)
                };
                (title_disp, artist_disp)
            } else {
                (
                    format!("{:<width$}", t.title, width = TITLE_WIDTH),
                    format!("{:<width$}", artist, width = ARTIST_WIDTH),
                )
            };

            let line = Line::from(vec![
                Span::raw(marker),
                Span::styled(title_str, Style::default().fg(Color::White)),
                Span::styled(format!(" {}", artist_str), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!(" {:>2}:{:02}", mins, secs),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(" {}", format_queue_badge(&state.queue_positions_for(i))),
                    Style::default().fg(Color::Magenta),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    // キュー件数をタイトルに表示
    let list_title = if state.queue_is_empty() {
        " crabplay ".to_string()
    } else {
        format!(" crabplay  [queue: {}] ", state.queue_len())
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, chunks[0], list_state);

    // スクロールバー
    let total = state.tracks.len();
    if total > 0 {
        let mut scrollbar_state = ScrollbarState::new(total).position(state.selected);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        f.render_stateful_widget(scrollbar, chunks[0], &mut scrollbar_state);
    }

    // 再生情報ペイン
    let (now_playing, np_color) = if let Some(ref msg) = state.info_msg {
        (format!(" ✓  {msg}"), Color::Green)
    } else if let Some(ref err) = state.last_error {
        (format!(" ⚠  {err}"), Color::Red)
    } else if let Some(track) = state.current() {
        let status = match state.player_state() {
            PlayerState::Playing => "▶",
            PlayerState::Paused => "⏸",
            PlayerState::Stopped => "■",
        };
        let pos = player.get_pos();
        let elapsed = format!("{}:{:02}", pos.as_secs() / 60, pos.as_secs() % 60);
        let total = format!(
            "{}:{:02}",
            track.duration_secs / 60,
            track.duration_secs % 60
        );
        (
            format!(
                " {} {} — {}  [{} / {}]",
                status, track.title, track.artist, elapsed, total
            ),
            Color::Yellow,
        )
    } else {
        (" ■  No track selected".to_string(), Color::DarkGray)
    };

    let now_playing_widget = Paragraph::new(now_playing)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Now Playing "),
        )
        .style(Style::default().fg(np_color));

    f.render_widget(now_playing_widget, chunks[1]);

    // キーバインドペイン（リピートモード表示付き）
    let keybinds_text = format!(
        " [↑↓] select  [Enter] play  [Space] pause  [n/p] next/prev  [a] queue  [c] clear  [r] repeat:{}  [s] save  [q] quit",
        state.repeat.label()
    );
    let keybinds = Paragraph::new(keybinds_text)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::DarkGray));

    f.render_widget(keybinds, chunks[2]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_empty() {
        assert_eq!(format_queue_badge(&[]), "      ");
        assert_eq!(format_queue_badge(&[]).len(), BADGE_WIDTH);
    }

    #[test]
    fn badge_single() {
        assert_eq!(format_queue_badge(&[1]), "[1]   ");
        assert_eq!(format_queue_badge(&[1]).len(), BADGE_WIDTH);
    }

    #[test]
    fn badge_two_single_digits() {
        assert_eq!(format_queue_badge(&[1, 3]), "[1,3] ");
        assert_eq!(format_queue_badge(&[1, 3]).len(), BADGE_WIDTH);
    }

    #[test]
    fn badge_many() {
        let result = format_queue_badge(&[2, 4, 6]);
        assert_eq!(result.len(), BADGE_WIDTH);
        assert!(result.starts_with("[2+2]"));
    }

    #[test]
    fn badge_always_badge_width() {
        for positions in [vec![], vec![1], vec![1, 2], vec![1, 2, 3], vec![10, 20]] {
            assert_eq!(
                format_queue_badge(&positions).len(),
                BADGE_WIDTH,
                "positions = {:?}",
                positions
            );
        }
    }
}
