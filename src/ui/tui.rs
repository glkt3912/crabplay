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
use std::collections::HashMap;
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

    // キュー変更時のみ再計算するバッジキャッシュ
    let mut queue_badge_map = state.queue_badge_map();
    let mut queue_dirty = false;

    loop {
        state.tick_error_timeout();

        if queue_dirty {
            queue_badge_map = state.queue_badge_map();
            queue_dirty = false;
        }

        terminal.draw(|f| {
            draw(
                f,
                state,
                player,
                &mut list_state,
                marquee_offset,
                &queue_badge_map,
            )
        })?;

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
                    if player.toggle_pause() {
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
                    queue_dirty = true;
                    state.info_msg = Some(format!("Queued: {} track(s)", state.queue_len()));
                }
                // キューをクリア
                KeyCode::Char('c') => {
                    state.clear_queue();
                    queue_dirty = true;
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
        // is_playback_settled() で load_and_play 直後の一瞬 empty() が true になる誤検知を防ぐ
        if matches!(state.player_state(), PlayerState::Playing)
            && state.is_playback_settled()
            && player.is_empty()
        {
            state.clear_messages();
            if state.advance() {
                queue_dirty = true;
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
    state.info_msg = None;
    if let Some(track) = state.tracks.get(state.selected) {
        let path = track.path.clone();
        match player.load_and_play(&path) {
            Ok(_) => state.set_playing(),
            Err(e) => {
                state.set_error(e.to_string());
                state.set_stopped();
            }
        }
    }
}

fn save_playlist(state: &mut AppState) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before UNIX epoch");
    // サブ秒精度（ミリ秒）を付加して同一秒内の上書きを防止
    let name = format!("playlist_{}_{:03}", ts.as_secs(), ts.subsec_millis());

    let paths = if state.queue_is_empty() {
        state.tracks.iter().map(|t| t.path.clone()).collect()
    } else {
        state.queue_paths()
    };

    let playlist = Playlist::new(&name, paths);
    match playlist.save(&Playlist::default_dir()) {
        Ok(dest) => state.info_msg = Some(format!("Saved: {}", dest.display())),
        Err(e) => state.set_error(e.to_string()),
    }
}

const BADGE_WIDTH: usize = 6;

/// 文字列を `width` 表示列に合わせてパディング（右揃えスペース）または切り詰めする。
///
/// Rust の `format!("{:<N}", s)` は char 単位でパディングするため CJK 全角文字（1 char = 2 列）
/// で実際の表示幅がずれる。この関数は `unicode-width` で表示幅を計算して正確に補正する。
fn pad_display(s: &str, width: usize) -> String {
    let disp = UnicodeWidthStr::width(s);
    if disp >= width {
        // 切り詰め: width 表示列に収まる文字だけ取る
        let mut out = String::new();
        let mut w = 0usize;
        for c in s.chars() {
            let mut buf = [0u8; 4];
            let cw = UnicodeWidthStr::width(c.encode_utf8(&mut buf));
            if w + cw > width {
                break;
            }
            out.push(c);
            w += cw;
        }
        // 全角文字が境界に収まらなかった場合の残りをスペースで埋める
        while UnicodeWidthStr::width(out.as_str()) < width {
            out.push(' ');
        }
        out
    } else {
        format!("{}{}", s, " ".repeat(width - disp))
    }
}

/// キューの位置番号リストをバッジ文字列（BADGE_WIDTH 文字固定）に変換する。
fn format_queue_badge(positions: &[usize]) -> String {
    let s = match positions {
        [] => return " ".repeat(BADGE_WIDTH),
        [p] => format!("[{}]", p),
        // "[x,y]" は5文字。BADGE_WIDTH(6) に収まるのは両方が1桁のときのみ
        [p1, p2] if *p1 < 10 && *p2 < 10 => format!("[{},{}]", p1, p2),
        [p1, rest @ ..] => format!("[{}+{}]", p1, rest.len()),
    };
    // p1 や rest.len() が多桁になり BADGE_WIDTH を超えた場合に切り詰める
    let truncated: String = s.chars().take(BADGE_WIDTH).collect();
    format!("{:<width$}", truncated, width = BADGE_WIDTH)
}

/// 文字列を表示幅ベースでスクロールし、max_width 幅に収めて返す。
///
/// `offset` は表示列単位（1 増加 = 1 列スクロール）。
/// 旧実装は `offset % (chars.len() + 2)` で文字数ベースのループを使っていたため、
/// CJK 全角文字（1 char = 2 列）を含む場合にスクロール速度が 2 倍になっていた。
/// 本実装は表示幅の合計 + 2 列の空白ギャップでループを計算する。
fn marquee_slice(s: &str, offset: usize, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.is_empty() {
        return " ".repeat(max_width);
    }

    // 各文字の (累積開始列, char, 表示幅) テーブルを構築
    let mut col_table: Vec<(usize, char, usize)> = Vec::with_capacity(chars.len());
    let mut acc = 0usize;
    for &ch in &chars {
        let mut buf = [0u8; 4];
        let w = UnicodeWidthStr::width(ch.encode_utf8(&mut buf));
        col_table.push((acc, ch, w));
        acc += w;
    }
    let total_disp = acc; // 文字列全体の表示幅
    let loop_disp = total_disp + 2; // 末尾 2 列の空白ギャップを含むループ幅

    // offset は表示列オフセット（1 増加 = 1 列スクロール）
    let start_col = offset % loop_disp;

    let mut result = String::new();
    let mut out_width = 0usize;
    let mut col = start_col;

    while out_width < max_width {
        let pos = col % loop_disp;
        if pos >= total_disp {
            // ギャップ領域（空白）
            result.push(' ');
            out_width += 1;
            col += 1;
        } else {
            // pos 列に対応する文字を線形探索（タイトル長は通常 100 char 未満）
            let (ci, w) = col_table
                .iter()
                .enumerate()
                .find(|(_, (c_start, _, cw))| c_start + cw > pos)
                .map(|(i, (_, _, cw))| (i, *cw))
                .unwrap_or((0, 1));
            if out_width + w > max_width {
                break;
            }
            result.push(col_table[ci].1);
            out_width += w;
            col += w;
        }
    }

    // 全角文字が境界に収まらなかった場合などを空白で補完
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
    queue_badge_map: &HashMap<usize, Vec<usize>>,
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
                    pad_display(&t.title, TITLE_WIDTH)
                };
                let artist_disp = if UnicodeWidthStr::width(artist) > ARTIST_WIDTH {
                    marquee_slice(artist, marquee_offset, ARTIST_WIDTH)
                } else {
                    pad_display(artist, ARTIST_WIDTH)
                };
                (title_disp, artist_disp)
            } else {
                // pad_display で表示幅ベースのパディング（CJK 全角文字対応）
                (
                    pad_display(&t.title, TITLE_WIDTH),
                    pad_display(artist, ARTIST_WIDTH),
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
                    format!(
                        " {}",
                        format_queue_badge(
                            queue_badge_map.get(&i).map(Vec::as_slice).unwrap_or(&[])
                        )
                    ),
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
        " [↑↓] select  [Enter] play  [Space] pause  [n/p] move+play  [a] queue  [c] clear  [r] repeat:{}  [s] save  [q] quit",
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
        assert_eq!(
            UnicodeWidthStr::width(format_queue_badge(&[]).as_str()),
            BADGE_WIDTH
        );
    }

    #[test]
    fn badge_single() {
        assert_eq!(format_queue_badge(&[1]), "[1]   ");
        assert_eq!(
            UnicodeWidthStr::width(format_queue_badge(&[1]).as_str()),
            BADGE_WIDTH
        );
    }

    #[test]
    fn badge_two_single_digits() {
        assert_eq!(format_queue_badge(&[1, 3]), "[1,3] ");
        assert_eq!(
            UnicodeWidthStr::width(format_queue_badge(&[1, 3]).as_str()),
            BADGE_WIDTH
        );
    }

    #[test]
    fn badge_many() {
        let result = format_queue_badge(&[2, 4, 6]);
        assert_eq!(UnicodeWidthStr::width(result.as_str()), BADGE_WIDTH);
        assert!(result.starts_with("[2+2]"));
    }

    #[test]
    fn badge_always_badge_width() {
        for positions in [vec![], vec![1], vec![1, 2], vec![1, 2, 3], vec![10, 20]] {
            assert_eq!(
                UnicodeWidthStr::width(format_queue_badge(&positions).as_str()),
                BADGE_WIDTH,
                "positions = {:?}",
                positions
            );
        }
    }
}
