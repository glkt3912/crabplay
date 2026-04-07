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
use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{AppState, PlayerState};
use crate::audio::player::Player;
use crate::library::{metadata::read_metadata, scanner::scan_directory};
use crate::playlist::Playlist;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiMode {
    Normal,
    SourcePicker,
    NameInput,
}

#[derive(Debug, Clone)]
enum SourceEntry {
    Directory(PathBuf),
    Playlist { path: PathBuf, name: String },
}

impl SourceEntry {
    fn label(&self) -> String {
        match self {
            SourceEntry::Directory(p) => format!("[Dir] {}", p.display()),
            SourceEntry::Playlist { name, .. } => format!("[PL]  {}", name),
        }
    }
}

/// オーバーレイの描画用状態。`draw()` の引数を減らすためにまとめる。
struct PickerState<'a> {
    mode: UiMode,
    entries: &'a [SourceEntry],
    selected: usize,
    /// NameInput モード時の入力バッファ
    name_input: &'a str,
}

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

    // マーキー用: フレームカウンタとキャッシュ（offset も内包）
    let mut marquee_tick: u32 = 0;
    let mut last_selected = state.selected;
    let mut marquee_cache = MarqueeCache::new();

    // プレイリスト変更時のみ再計算するバッジキャッシュ
    let mut playlist_badge_map = state.playlist_badge_map();
    let mut playlist_dirty = false;

    // ソース選択オーバーレイの状態
    let mut ui_mode = UiMode::Normal;
    let mut picker_entries: Vec<SourceEntry> = Vec::new();
    let mut picker_selected: usize = 0;
    // プレイリスト名入力バッファ
    let mut name_input = String::new();

    loop {
        state.tick_timeouts();

        if playlist_dirty {
            playlist_badge_map = state.playlist_badge_map();
            playlist_dirty = false;
        }

        let picker = PickerState {
            mode: ui_mode,
            entries: &picker_entries,
            selected: picker_selected,
            name_input: &name_input,
        };
        terminal.draw(|f| {
            draw(
                f,
                state,
                player,
                &mut list_state,
                &playlist_badge_map,
                &picker,
                &mut marquee_cache,
            )
        })?;

        if event::poll(std::time::Duration::from_millis(200))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match ui_mode {
                UiMode::Normal => {
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
                            // Stopped 状態で toggle_pause() を呼ぶと sink.play() が走り
                            // playing_index=None のまま PlayerState::Playing になるため除外する
                            if state.player_state() != PlayerState::Stopped {
                                if player.toggle_pause() {
                                    state.set_paused();
                                } else {
                                    state.set_resumed();
                                }
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
                        // プレイリストに選択中のトラックを追加
                        KeyCode::Char('a') => {
                            if state.playlist_add_selected() {
                                playlist_dirty = true;
                                state.set_info(format!("Added  (PL:{})", state.playlist_len()));
                            } else {
                                state.set_info("Already in playlist".to_string());
                            }
                        }
                        // プレイリストをクリア
                        KeyCode::Char('c') => {
                            state.clear_playlist();
                            playlist_dirty = true;
                            state.set_info("Playlist cleared".to_string());
                        }
                        // リピートモードをサイクル
                        KeyCode::Char('r') => {
                            state.cycle_repeat();
                            state.set_info(format!("Repeat: {}", state.repeat.label()));
                        }
                        // プレイリスト名入力オーバーレイを開く
                        KeyCode::Char('s') => {
                            if state.playlist_is_empty() {
                                state.set_error(
                                    "Playlist is empty. Add tracks with [a] before saving."
                                        .to_string(),
                                );
                            } else {
                                name_input.clear();
                                ui_mode = UiMode::NameInput;
                            }
                        }
                        // シャッフルのトグル
                        KeyCode::Char('z') => {
                            state.toggle_shuffle();
                            let msg = if state.shuffle {
                                "Shuffle: On"
                            } else {
                                "Shuffle: Off"
                            };
                            state.set_info(msg.to_string());
                        }
                        // ソース選択オーバーレイを開く
                        KeyCode::Char('o') => {
                            picker_entries = build_source_entries(&state.source_dir);
                            picker_selected = 0;
                            ui_mode = UiMode::SourcePicker;
                        }
                        // シーク（±5秒）
                        KeyCode::Left | KeyCode::Right
                            if state.player_state() != PlayerState::Stopped =>
                        {
                            const SEEK_SECS: u64 = 5;
                            let current = player.get_pos();
                            let target = if key.code == KeyCode::Left {
                                current.saturating_sub(std::time::Duration::from_secs(SEEK_SECS))
                            } else {
                                current + std::time::Duration::from_secs(SEEK_SECS)
                            };
                            if let Err(e) = player.seek(target) {
                                state.set_error(format!("Seek failed: {e}"));
                            }
                        }
                        // 音量調整
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            state.volume_up();
                            player.set_volume(state.volume);
                            state.set_info(format!(
                                "Volume: {}%",
                                (state.volume * 100.0).round() as u32
                            ));
                        }
                        KeyCode::Char('-') => {
                            state.volume_down();
                            player.set_volume(state.volume);
                            state.set_info(format!(
                                "Volume: {}%",
                                (state.volume * 100.0).round() as u32
                            ));
                        }
                        _ => {}
                    }
                }
                UiMode::SourcePicker => match key.code {
                    KeyCode::Esc => {
                        ui_mode = UiMode::Normal;
                    }
                    KeyCode::Up => {
                        picker_selected = picker_selected.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if picker_selected + 1 < picker_entries.len() {
                            picker_selected += 1;
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(entry) = picker_entries.get(picker_selected).cloned() {
                            load_source(state, player, &entry);
                            playlist_dirty = true;
                            list_state.select(Some(0));
                            marquee_tick = 0;
                            last_selected = 0;
                            marquee_cache.clear();
                        }
                        ui_mode = UiMode::Normal;
                    }
                    KeyCode::Char('d') => match picker_entries.get(picker_selected) {
                        Some(SourceEntry::Playlist { path, name }) => {
                            let name = name.clone();
                            match std::fs::remove_file(path) {
                                Ok(_) => {
                                    state.set_info(format!("Deleted '{name}'"));
                                    picker_entries = build_source_entries(&state.source_dir);
                                    picker_selected =
                                        picker_selected.min(picker_entries.len().saturating_sub(1));
                                }
                                Err(e) => {
                                    state.set_error(format!("Delete failed: {e}"));
                                }
                            }
                        }
                        Some(SourceEntry::Directory(_)) => {
                            state.set_error("Cannot delete directory entry".to_string());
                        }
                        None => {}
                    },
                    _ => {} // Normal キーを誤処理しない
                },
                UiMode::NameInput => match key.code {
                    KeyCode::Esc => {
                        ui_mode = UiMode::Normal;
                    }
                    KeyCode::Enter => {
                        if name_input.is_empty() {
                            state.set_error("Name cannot be empty.".to_string());
                        } else {
                            save_playlist(state, &name_input);
                        }
                        ui_mode = UiMode::Normal;
                    }
                    KeyCode::Backspace => {
                        name_input.pop();
                    }
                    KeyCode::Char(c)
                        if !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') =>
                    {
                        name_input.push(c);
                    }
                    _ => {}
                },
            }
        }

        // 選択が変わったらマーキーをリセット
        if state.selected != last_selected {
            marquee_cache.reset_offset();
            marquee_tick = 0;
            last_selected = state.selected;
        }

        // 5フレーム（約1秒）ごとにマーキーを1文字進める
        marquee_tick += 1;
        if marquee_tick >= 5 {
            marquee_tick = 0;
            marquee_cache.offset += 1;
        }

        // rodio::Sink::empty() == true → 再生バッファが空 → トラック再生完了
        // is_playback_settled() で load_and_play 直後の一瞬 empty() が true になる誤検知を防ぐ
        if matches!(state.player_state(), PlayerState::Playing)
            && state.is_playback_settled()
            && player.is_empty()
        {
            state.clear_messages();
            if state.advance() {
                playlist_dirty = false; // advance() は playlist を変更しない
                list_state.select(Some(state.selected));
                marquee_cache.reset_offset();
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
    state.clear_messages();
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

fn save_playlist(state: &mut AppState, name: &str) {
    let paths = state.playlist_paths();
    let playlist = Playlist::new(name, paths);
    match playlist.save(&Playlist::default_dir()) {
        Ok(_) => state.set_info(format!("Saved as '{name}'")),
        Err(e) => state.set_error(e.to_string()),
    }
}

/// `o` キー押下時にソース一覧を構築する。毎回呼ぶことで常に最新の状態を反映する。
fn build_source_entries(source_dir: &std::path::Path) -> Vec<SourceEntry> {
    let mut entries = vec![SourceEntry::Directory(source_dir.to_path_buf())];

    let pl_dir = Playlist::default_dir();
    if let Ok(read_dir) = std::fs::read_dir(&pl_dir) {
        let mut playlist_entries: Vec<(std::time::SystemTime, SourceEntry)> = read_dir
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("json"))
                    .unwrap_or(false)
            })
            .filter_map(|e| {
                let path = e.path();
                let pl = Playlist::load(&path).ok()?;
                let mtime = e
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(std::time::UNIX_EPOCH);
                Some((
                    mtime,
                    SourceEntry::Playlist {
                        path,
                        name: pl.name,
                    },
                ))
            })
            .collect();
        // 更新日時の新しい順（全件）
        playlist_entries.sort_by(|a, b| b.0.cmp(&a.0));
        entries.extend(playlist_entries.into_iter().map(|(_, e)| e));
    }

    entries
}

/// 選択されたソースをロードし、トラック一覧を差し替える。
/// player.stop() はこの関数内で呼ぶ。replace_tracks の後に set_info で通知する。
fn load_source(state: &mut AppState, player: &Player, entry: &SourceEntry) {
    player.stop();

    match entry {
        SourceEntry::Directory(dir) => match scan_directory(dir) {
            Err(e) => {
                state.set_error(format!("Scan failed: {e}"));
            }
            Ok(paths) => {
                let mut skip = 0usize;
                let tracks: Vec<_> = paths
                    .iter()
                    .filter_map(|p| match read_metadata(p) {
                        Ok(t) => Some(t),
                        Err(_) => {
                            skip += 1;
                            None
                        }
                    })
                    .collect();
                if tracks.is_empty() {
                    state.set_error(format!("No tracks found in '{}'", dir.display()));
                    return;
                }
                state.replace_tracks(tracks);
                if skip > 0 {
                    state.set_info(format!(
                        "Loaded ({} file(s) skipped, playlist cleared)",
                        skip
                    ));
                } else {
                    state.set_info("Source loaded (playlist cleared)".to_string());
                }
            }
        },
        SourceEntry::Playlist { path, .. } => match Playlist::load(path) {
            Err(e) => {
                state.set_error(format!("Failed to load playlist: {e}"));
            }
            Ok(pl) => {
                let mut skip = 0usize;
                let tracks: Vec<_> = pl
                    .paths
                    .iter()
                    .filter_map(|p| {
                        if !p.exists() {
                            skip += 1;
                            return None;
                        }
                        match read_metadata(p) {
                            Ok(t) => Some(t),
                            Err(_) => {
                                skip += 1;
                                None
                            }
                        }
                    })
                    .collect();
                if tracks.is_empty() {
                    state.set_error("Playlist is empty or all paths are missing".to_string());
                    return;
                }
                state.replace_tracks(tracks);
                if skip > 0 {
                    state.set_info(format!("Playlist loaded: {} missing file(s) skipped", skip));
                } else {
                    state.set_info("Playlist loaded".to_string());
                }
            }
        },
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
            let cw = UnicodeWidthChar::width(c).unwrap_or(1);
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

/// 各文字の (累積開始列, char, 表示幅) テーブルと文字列全体の表示幅を返す。
fn build_col_table(s: &str) -> (Vec<(usize, char, usize)>, usize) {
    let mut col_table: Vec<(usize, char, usize)> = Vec::new();
    let mut acc = 0usize;
    for ch in s.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
        col_table.push((acc, ch, w));
        acc += w;
    }
    (col_table, acc)
}

/// テスト用ラッパー: build_col_table + marquee_from_table を一括呼び出し。
#[cfg(test)]
fn marquee_slice(s: &str, offset: usize, max_width: usize) -> String {
    if s.is_empty() {
        return " ".repeat(max_width);
    }
    let (col_table, total_disp) = build_col_table(s);
    marquee_from_table(&col_table, total_disp, offset, max_width)
}

fn marquee_from_table(
    col_table: &[(usize, char, usize)],
    total_disp: usize,
    offset: usize,
    max_width: usize,
) -> String {
    if col_table.is_empty() {
        return " ".repeat(max_width);
    }
    let loop_disp = total_disp + 2; // 末尾 2 列の空白ギャップを含むループ幅
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
            let c_start = col_table[ci].0;
            if pos > c_start {
                // 全角文字の中間列から開始（例: 幅2の文字の2列目に offset が着地）
                // → 空白 1 列を出力して次の文字の先頭へ進める
                result.push(' ');
                out_width += 1;
                col = c_start + w;
            } else {
                if out_width + w > max_width {
                    break;
                }
                result.push(col_table[ci].1);
                out_width += w;
                col += w;
            }
        }
    }

    // 全角文字が境界に収まらなかった場合などを空白で補完
    while UnicodeWidthStr::width(result.as_str()) < max_width {
        result.push(' ');
    }

    result
}

type ColTable = (Vec<(usize, char, usize)>, usize);

/// 文字列ごとの col_table をキャッシュし、同一文字列のフレームをまたいだ再計算を省く。
/// `offset` も保持することで draw() の引数を増やさずに済む。
struct MarqueeCache {
    offset: usize,
    entries: HashMap<String, ColTable>,
}

impl MarqueeCache {
    fn new() -> Self {
        Self {
            offset: 0,
            entries: HashMap::new(),
        }
    }

    fn render(&mut self, s: &str, max_width: usize) -> String {
        if s.is_empty() {
            return " ".repeat(max_width);
        }
        let (table, total_disp) = self
            .entries
            .entry(s.to_owned())
            .or_insert_with(|| build_col_table(s));
        marquee_from_table(table, *total_disp, self.offset, max_width)
    }

    fn reset_offset(&mut self) {
        self.offset = 0;
    }

    fn clear(&mut self) {
        self.offset = 0;
        self.entries.clear();
    }
}

fn draw(
    f: &mut ratatui::Frame,
    state: &AppState,
    player: &Player,
    list_state: &mut ListState,
    playlist_badge_map: &HashMap<usize, Vec<usize>>,
    picker: &PickerState,
    marquee_cache: &mut MarqueeCache,
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

    // タイトル列・アーティスト列の表示幅をターミナル幅に合わせて動的に計算する。
    // 固定オーバーヘッド: ボーダー2 + マーカー2 + スペース1 + 時間6 + バッジ7 = 18
    const FIXED_OVERHEAD: usize = 18;
    const TITLE_MIN: usize = 20;
    const ARTIST_MIN: usize = 12;
    let list_inner_width = chunks[0].width.saturating_sub(2) as usize;
    let available = list_inner_width.saturating_sub(FIXED_OVERHEAD);
    let title_width = (available * 62 / 100).max(TITLE_MIN);
    let artist_width = available.saturating_sub(title_width).max(ARTIST_MIN);

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
                let title_disp = if UnicodeWidthStr::width(t.title.as_str()) > title_width {
                    marquee_cache.render(&t.title, title_width)
                } else {
                    pad_display(&t.title, title_width)
                };
                let artist_disp = if UnicodeWidthStr::width(artist) > artist_width {
                    marquee_cache.render(artist, artist_width)
                } else {
                    pad_display(artist, artist_width)
                };
                (title_disp, artist_disp)
            } else {
                // pad_display で表示幅ベースのパディング（CJK 全角文字対応）
                (
                    pad_display(&t.title, title_width),
                    pad_display(artist, artist_width),
                )
            };

            let line = Line::from(vec![
                Span::raw(marker),
                Span::styled(title_str, Style::default().fg(Color::Green)),
                Span::styled(format!(" {}", artist_str), Style::default().fg(Color::Cyan)),
                Span::styled(
                    format!(" {:>2}:{:02}", mins, secs),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(
                    format!(
                        " {}",
                        format_queue_badge(
                            playlist_badge_map.get(&i).map(Vec::as_slice).unwrap_or(&[])
                        )
                    ),
                    Style::default().fg(Color::Magenta),
                ),
            ]);
            ListItem::new(line)
        })
        .collect();

    // プレイリスト件数をタイトルに表示
    let list_title = if state.playlist_is_empty() {
        " crabplay ".to_string()
    } else {
        format!(" crabplay  [PL: {}] ", state.playlist_len())
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
        let vol = format!("VOL {}%", (state.volume * 100.0).round() as u32);
        let shuf = if state.shuffle { "  SHUF" } else { "" };
        (
            format!(
                " {} {} — {}  [{} / {}]  {}{}",
                status, track.title, track.artist, elapsed, total, vol, shuf
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
    // テキストがターミナル幅を超える場合はマーキースクロール
    let keybinds_raw = format!(
        " [↑↓] select  [Enter] play  [Space] pause  [←/→] seek ±5s  [n/p] move+play  [a] add to playlist  [c] clear playlist  [r] repeat:{}  [z] shuffle:{}  [s] save playlist  [o] open source  [+/-] volume  [q] quit",
        state.repeat.label(),
        if state.shuffle { "On" } else { "Off" }
    );
    let keybinds_inner_width = chunks[2].width.saturating_sub(2) as usize;
    let keybinds_display = if UnicodeWidthStr::width(keybinds_raw.as_str()) > keybinds_inner_width {
        marquee_cache.render(&keybinds_raw, keybinds_inner_width)
    } else {
        keybinds_raw
    };
    let keybinds = Paragraph::new(keybinds_display)
        .block(Block::default().borders(Borders::ALL))
        .style(Style::default().fg(Color::LightCyan));

    f.render_widget(keybinds, chunks[2]);

    // ソース選択オーバーレイ（SourcePicker モード時のみ）
    if picker.mode == UiMode::SourcePicker {
        draw_source_picker(f, picker.entries, picker.selected);
    }
    // プレイリスト名入力オーバーレイ（NameInput モード時のみ）
    if picker.mode == UiMode::NameInput {
        draw_name_input(f, picker.name_input);
    }
}

/// 画面中央に percent_x × percent_y の矩形を返す。
fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

/// ソース選択オーバーレイを描画する。
fn draw_source_picker(f: &mut ratatui::Frame, entries: &[SourceEntry], selected: usize) {
    use ratatui::widgets::Clear;

    let area = centered_rect(70, 60, f.area());
    f.render_widget(Clear, area);

    let items: Vec<ListItem> = entries.iter().map(|e| ListItem::new(e.label())).collect();

    let mut picker_list_state = ListState::default();
    picker_list_state.select(Some(selected));

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Open Source  [↑↓] move  [Enter] load  [d] delete  [Esc] cancel ")
                .border_style(Style::default().fg(Color::Yellow)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut picker_list_state);
}

/// プレイリスト名入力オーバーレイを描画する。
fn draw_name_input(f: &mut ratatui::Frame, name_input: &str) {
    use ratatui::widgets::Clear;

    let area = centered_rect(60, 20, f.area());
    f.render_widget(Clear, area);

    let display = format!(" > {name_input}_");
    let widget = Paragraph::new(display)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Save Playlist  [Enter] save  [Esc] cancel ")
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .style(Style::default().fg(Color::White));

    f.render_widget(widget, area);
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
    fn marquee_ascii_scrolls_one_col_per_offset() {
        // "ABC"(3列) + 2列ギャップ = loop_disp 5
        // offset=0 → pos 0,1,2 = "ABC"
        // offset=1 → pos 1,2,3(gap) = "BC "
        // offset=2 → pos 2,3(gap),4(gap) = "C  "
        // offset=3 → pos 3(gap),4(gap),0 = "  A"（ギャップ2列 + 折り返しA）
        // offset=5 = offset=0 = "ABC"（ループ）
        assert_eq!(marquee_slice("ABC", 0, 3), "ABC");
        assert_eq!(marquee_slice("ABC", 1, 3), "BC ");
        assert_eq!(marquee_slice("ABC", 2, 3), "C  ");
        assert_eq!(marquee_slice("ABC", 3, 3), "  A");
        assert_eq!(marquee_slice("ABC", 5, 3), "ABC");
    }

    #[test]
    fn marquee_cjk_mid_column_becomes_space() {
        // "あ"（幅2）+ 2列ギャップ = loop 4
        // offset=0: 'あ'(2列) で max_width=2 を満たす → "あ"
        // offset=1: 'あ' の中間列 → 空白 1 列補完 + ギャップ 1 列 → "  "
        assert_eq!(marquee_slice("あ", 0, 2), "あ");
        assert_eq!(marquee_slice("あ", 1, 2), "  ");
        // offset=2,3: ギャップ → "  "
        assert_eq!(marquee_slice("あ", 2, 2), "  ");
        // offset=4: ループして再び "あ"
        assert_eq!(marquee_slice("あ", 4, 2), "あ");
    }

    #[test]
    fn marquee_always_max_width() {
        for s in ["Hello", "あいう", "Mix混合テスト"] {
            for offset in 0..30 {
                let result = marquee_slice(s, offset, 8);
                assert_eq!(
                    UnicodeWidthStr::width(result.as_str()),
                    8,
                    "s={s:?} offset={offset}"
                );
            }
        }
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
