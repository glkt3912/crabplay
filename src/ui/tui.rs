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
        Block, Borders, Gauge, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, ScrollbarState,
    },
};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::{AppState, PlayerState, SortKey};
use crate::audio::player::Player;
use crate::config::Config;
use crate::library::{metadata::read_metadata, scanner::scan_directory};
use crate::playlist::Playlist;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UiMode {
    Normal,
    SourcePicker,
    NameInput,
    Search,
    Help,
    QueueViewer,
}

#[derive(Debug, Clone)]
enum SourceEntry {
    Directory(PathBuf),
    RecentDir(PathBuf),
    Playlist { path: PathBuf, name: String },
}

impl SourceEntry {
    fn label(&self) -> String {
        match self {
            SourceEntry::Directory(p) => format!("[Dir]    {}", p.display()),
            SourceEntry::RecentDir(p) => format!("[Recent] {}", p.display()),
            SourceEntry::Playlist { name, .. } => format!("[PL]     {}", name),
        }
    }

    /// ディレクトリ系エントリならそのパスを返す（履歴記録に使用）。
    fn loaded_dir(&self) -> Option<PathBuf> {
        match self {
            SourceEntry::Directory(p) | SourceEntry::RecentDir(p) => Some(p.clone()),
            SourceEntry::Playlist { .. } => None,
        }
    }
}

/// オーバーレイ・検索の描画用状態。`draw()` の引数を減らすためにまとめる。
struct PickerState<'a> {
    mode: UiMode,
    entries: &'a [SourceEntry],
    selected: usize,
    /// NameInput モード時の入力バッファ
    name_input: &'a str,
    /// Search モード時のクエリ文字列
    search_query: &'a str,
    /// Search モード時のフィルタ済みトラックインデックス列
    search_indices: &'a [usize],
    /// Search モード時のカーソル位置（フィルタ済みリスト内）
    search_cursor: usize,
    /// Help モード時のスクロールオフセット
    help_scroll: u16,
    /// QueueViewer モード時の選択カーソル位置
    queue_selected: usize,
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

    // config 読み込み・起動ディレクトリを履歴に記録し、前回の音量・リピートモード・シャッフルを復元する
    let mut config = Config::load();
    config.push_recent_dir(state.source_dir.clone());
    state.volume = config.volume;
    state.repeat = config.repeat;
    state.shuffle = config.shuffle;
    player.set_volume(state.volume);
    let _ = config.save();

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
    // インクリメンタル検索の状態
    let mut search_query = String::new();
    let mut search_indices: Vec<usize> = Vec::new();
    let mut search_cursor: usize = 0;
    // ヘルプオーバーレイのスクロールオフセット
    let mut help_scroll: u16 = 0;
    // キュービューアーの選択カーソル
    let mut queue_selected: usize = 0;

    loop {
        state.tick_timeouts();

        if playlist_dirty {
            playlist_badge_map = state.playlist_badge_map();
            playlist_dirty = false;
        }

        // Search モード中は list_state を検索カーソルに同期する（0件時は選択なし）
        if ui_mode == UiMode::Search {
            if search_indices.is_empty() {
                list_state.select(None);
            } else {
                list_state.select(Some(search_cursor));
            }
        } else {
            list_state.select(Some(state.selected));
        }

        let picker = PickerState {
            mode: ui_mode,
            entries: &picker_entries,
            selected: picker_selected,
            name_input: &name_input,
            search_query: &search_query,
            search_indices: &search_indices,
            search_cursor,
            help_scroll,
            queue_selected,
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
                            config.repeat = state.repeat;
                            let _ = config.save();
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
                            config.shuffle = state.shuffle;
                            let _ = config.save();
                        }
                        // インクリメンタル検索を開く
                        KeyCode::Char('/') => {
                            search_query.clear();
                            search_indices = (0..state.tracks.len()).collect();
                            search_cursor = 0;
                            marquee_cache.reset_offset();
                            marquee_tick = 0;
                            ui_mode = UiMode::Search;
                        }
                        // ヘルプオーバーレイを開く
                        KeyCode::Char('?') => {
                            help_scroll = 0;
                            ui_mode = UiMode::Help;
                        }
                        // ソース選択オーバーレイを開く
                        KeyCode::Char('o') => {
                            picker_entries = build_source_entries(&state.source_dir, &config);
                            picker_selected = 0;
                            ui_mode = UiMode::SourcePicker;
                        }
                        // キュービューアーを開く
                        KeyCode::Char('v') => {
                            queue_selected = 0;
                            ui_mode = UiMode::QueueViewer;
                        }
                        // ソートキーをサイクル
                        KeyCode::Char('S') => {
                            state.cycle_sort();
                            state.set_info(format!("Sort: {}", state.sort_key.label()));
                            playlist_dirty = true;
                        }
                        // シーク（±5秒）
                        KeyCode::Left | KeyCode::Right
                            if state.player_state() != PlayerState::Stopped =>
                        {
                            state.clear_messages();
                            let current = player.get_pos();
                            let target = if matches!(key.code, KeyCode::Left) {
                                current.saturating_sub(SEEK_OFFSET)
                            } else {
                                current + SEEK_OFFSET
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
                            config.volume = state.volume;
                            let _ = config.save();
                        }
                        KeyCode::Char('-') => {
                            state.volume_down();
                            player.set_volume(state.volume);
                            state.set_info(format!(
                                "Volume: {}%",
                                (state.volume * 100.0).round() as u32
                            ));
                            config.volume = state.volume;
                            let _ = config.save();
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
                        if let Some(entry) = picker_entries.get(picker_selected).cloned()
                            && load_source(state, player, &entry)
                        {
                            // ディレクトリロード成功時に履歴を更新する
                            if let Some(dir) = entry.loaded_dir() {
                                config.push_recent_dir(dir);
                                let _ = config.save();
                            }
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
                                    picker_entries =
                                        build_source_entries(&state.source_dir, &config);
                                    picker_selected =
                                        picker_selected.min(picker_entries.len().saturating_sub(1));
                                }
                                Err(e) => {
                                    state.set_error(format!("Delete failed: {e}"));
                                }
                            }
                        }
                        Some(SourceEntry::RecentDir(dir)) => {
                            let dir = dir.clone();
                            config.remove_recent_dir(&dir);
                            match config.save() {
                                Ok(_) => {
                                    state.set_info(format!(
                                        "Removed '{}' from recents",
                                        dir.display()
                                    ));
                                    picker_entries =
                                        build_source_entries(&state.source_dir, &config);
                                    picker_selected =
                                        picker_selected.min(picker_entries.len().saturating_sub(1));
                                }
                                Err(e) => {
                                    state.set_error(format!("Save failed: {e}"));
                                }
                            }
                        }
                        Some(SourceEntry::Directory(_)) => {
                            state.set_error("Cannot delete current directory entry".to_string());
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
                UiMode::Search => match key.code {
                    KeyCode::Esc => {
                        ui_mode = UiMode::Normal;
                        list_state.select(Some(state.selected));
                        marquee_cache.reset_offset();
                        marquee_tick = 0;
                    }
                    KeyCode::Enter => {
                        if let Some(&idx) = search_indices.get(search_cursor) {
                            state.selected = idx;
                        }
                        list_state.select(Some(state.selected));
                        last_selected = state.selected;
                        ui_mode = UiMode::Normal;
                        marquee_cache.reset_offset();
                        marquee_tick = 0;
                    }
                    KeyCode::Up => {
                        search_cursor = search_cursor.saturating_sub(1);
                        list_state.select(Some(search_cursor));
                    }
                    KeyCode::Down => {
                        if search_cursor + 1 < search_indices.len() {
                            search_cursor += 1;
                        }
                        list_state.select(Some(search_cursor));
                    }
                    KeyCode::Backspace => {
                        search_query.pop();
                        search_indices = filter_tracks(&state.tracks, &search_query);
                        if search_indices.is_empty() {
                            search_cursor = 0;
                        } else {
                            search_cursor = search_cursor.min(search_indices.len() - 1);
                        }
                    }
                    KeyCode::Char(c) => {
                        search_query.push(c);
                        search_indices = filter_tracks(&state.tracks, &search_query);
                        search_cursor = 0;
                    }
                    _ => {}
                },
                // ヘルプオーバーレイ: ↑/↓ でスクロール、他キーで閉じる
                UiMode::Help => match key.code {
                    KeyCode::Up => {
                        help_scroll = help_scroll.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        help_scroll += 1;
                    }
                    _ => {
                        ui_mode = UiMode::Normal;
                        help_scroll = 0;
                    }
                },
                // キュービューアー: ↑/↓ で選択、d で削除、Esc で閉じる
                UiMode::QueueViewer => match key.code {
                    KeyCode::Up => {
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::SHIFT)
                        {
                            if state.playlist_move_up(queue_selected) {
                                playlist_dirty = true;
                                queue_selected -= 1;
                            }
                        } else {
                            queue_selected = queue_selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Down => {
                        let len = state.playlist_len();
                        if key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::SHIFT)
                        {
                            if state.playlist_move_down(queue_selected) {
                                playlist_dirty = true;
                                queue_selected = (queue_selected + 1).min(len.saturating_sub(1));
                            }
                        } else if len > 0 {
                            queue_selected = (queue_selected + 1).min(len - 1);
                        }
                    }
                    KeyCode::Char('d') => {
                        let len = state.playlist_len();
                        if len > 0 {
                            state.playlist_remove_at(queue_selected);
                            playlist_dirty = true;
                            if queue_selected >= state.playlist_len() {
                                queue_selected = state.playlist_len().saturating_sub(1);
                            }
                        }
                    }
                    KeyCode::Esc => {
                        ui_mode = UiMode::Normal;
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
fn build_source_entries(source_dir: &std::path::Path, config: &Config) -> Vec<SourceEntry> {
    let mut entries = vec![SourceEntry::Directory(source_dir.to_path_buf())];

    // 最近使ったディレクトリ（現在の source_dir は除く）
    for dir in &config.recent_dirs {
        if dir.as_path() != source_dir {
            entries.push(SourceEntry::RecentDir(dir.clone()));
        }
    }

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

/// 選択されたソースをロードし、トラック一覧を差し替える。成功時に `true` を返す。
/// player.stop() はこの関数内で呼ぶ。replace_tracks の後に set_info で通知する。
fn load_source(state: &mut AppState, player: &Player, entry: &SourceEntry) -> bool {
    player.stop();

    match entry {
        SourceEntry::Directory(dir) | SourceEntry::RecentDir(dir) => match scan_directory(dir) {
            Err(e) => {
                state.set_error(format!("Scan failed: {e}"));
                false
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
                    return false;
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
                true
            }
        },
        SourceEntry::Playlist { path, .. } => match Playlist::load(path) {
            Err(e) => {
                state.set_error(format!("Failed to load playlist: {e}"));
                false
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
                    return false;
                }
                state.replace_tracks(tracks);
                if skip > 0 {
                    state.set_info(format!("Playlist loaded: {} missing file(s) skipped", skip));
                } else {
                    state.set_info("Playlist loaded".to_string());
                }
                true
            }
        },
    }
}

const BADGE_WIDTH: usize = 6;
const SEEK_OFFSET: std::time::Duration = std::time::Duration::from_secs(5);

/// `query` にマッチするトラックのインデックス列を返す。
/// タイトル・アーティスト・アルバムに対して大文字小文字を無視した部分一致で検索する。
/// `query` が空の場合は全インデックスを返す。
fn filter_tracks(tracks: &[crate::models::TrackInfo], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..tracks.len()).collect();
    }
    let q = query.to_lowercase();
    tracks
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            t.title.to_lowercase().contains(&q)
                || t.artist.to_lowercase().contains(&q)
                || t.album.to_lowercase().contains(&q)
        })
        .map(|(i, _)| i)
        .collect()
}

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
    // 3分割レイアウト: トラックリスト / 再生情報（プログレスバー含む）/ キーバインド
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(4),
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

    // Search モード中はフィルタ済みインデックスのみ表示、それ以外は全件
    let is_search = picker.mode == UiMode::Search;
    let owned_indices: Vec<usize>;
    let display_indices: &[usize] = if is_search {
        picker.search_indices
    } else {
        owned_indices = (0..state.tracks.len()).collect();
        &owned_indices
    };
    // マーキー対象の全体インデックス（Search 中はカーソル位置のトラック）
    let marquee_track_idx = if is_search {
        picker.search_indices.get(picker.search_cursor).copied()
    } else {
        Some(state.selected)
    };

    // トラックリスト
    let items: Vec<ListItem> = display_indices
        .iter()
        .map(|&i| {
            let t = &state.tracks[i];
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

            let (title_str, artist_str) = if marquee_track_idx == Some(i) {
                // カーソル位置: 表示幅を超える場合にマーキー
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
                    Style::default().fg(Color::Reset),
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

    // タイトルバー: Search 中はマッチ件数を表示
    let sort_badge = if state.sort_key != SortKey::Default {
        format!("  [Sort: {}]", state.sort_key.label())
    } else {
        String::new()
    };
    let list_title = if is_search {
        format!(
            " crabplay  [検索: {}/{}]{} ",
            picker.search_indices.len(),
            state.tracks.len(),
            sort_badge
        )
    } else if state.playlist_is_empty() {
        format!(" crabplay{} ", sort_badge)
    } else {
        format!(" crabplay  [PL: {}]{} ", state.playlist_len(), sort_badge)
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(list_title))
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, chunks[0], list_state);

    // スクロールバー（Search 中はフィルタ済み件数で計算）
    let scroll_total = display_indices.len();
    let scroll_pos = if is_search {
        picker.search_cursor
    } else {
        state.selected
    };
    if scroll_total > 0 {
        let mut scrollbar_state = ScrollbarState::new(scroll_total).position(scroll_pos);
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("↑"))
            .end_symbol(Some("↓"));
        f.render_stateful_widget(scrollbar, chunks[0], &mut scrollbar_state);
    }

    // 再生情報ペイン（テキスト行 + プログレスバー）
    let np_block = Block::default()
        .borders(Borders::ALL)
        .title(" Now Playing ");
    let np_inner = np_block.inner(chunks[1]);
    f.render_widget(np_block, chunks[1]);

    let np_rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(1)])
        .split(np_inner);

    let (now_playing_text, np_color, progress_ratio) = if let Some(ref msg) = state.info_msg {
        (format!(" ✓  {msg}"), Color::Green, None)
    } else if let Some(ref err) = state.last_error {
        (format!(" ⚠  {err}"), Color::Red, None)
    } else if let Some(track) = state.current() {
        let status = match state.player_state() {
            PlayerState::Playing => "▶",
            PlayerState::Paused => "⏸",
            PlayerState::Stopped => "■",
        };
        let pos = player.get_pos();
        let elapsed = format!("{}:{:02}", pos.as_secs() / 60, pos.as_secs() % 60);
        let total_str = format!(
            "{}:{:02}",
            track.duration_secs / 60,
            track.duration_secs % 60
        );
        let vol = format!("VOL {}%", (state.volume * 100.0).round() as u32);
        let shuf = if state.shuffle { "  SHUF" } else { "" };
        let ratio = if track.duration_secs > 0 {
            (pos.as_secs_f64() / track.duration_secs as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (
            format!(
                " {} {} — {}  [{} / {}]  {}{}",
                status, track.title, track.artist, elapsed, total_str, vol, shuf
            ),
            Color::Yellow,
            Some(ratio),
        )
    } else {
        (" ■  No track selected".to_string(), Color::Reset, None)
    };

    f.render_widget(
        Paragraph::new(now_playing_text).style(Style::default().fg(np_color)),
        np_rows[0],
    );

    if let Some(ratio) = progress_ratio {
        let gauge_color = match state.player_state() {
            PlayerState::Playing => Color::Yellow,
            PlayerState::Paused => Color::Reset,
            PlayerState::Stopped => Color::Reset,
        };
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(gauge_color).bg(Color::Reset))
            .ratio(ratio)
            .label("");
        f.render_widget(gauge, np_rows[1]);
    }

    // キーバインドペイン（Search モード中は検索バーとして流用）
    if is_search {
        let search_display = format!(
            " / {}█  [{}/{}]  [↑↓] 移動  [Enter] 確定  [Esc] キャンセル",
            picker.search_query,
            picker.search_indices.len(),
            state.tracks.len(),
        );
        let search_bar = Paragraph::new(search_display)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::Yellow));
        f.render_widget(search_bar, chunks[2]);
    } else {
        // 通常のキーバインド表示（リピートモード・シャッフル状態をリアルタイム表示）
        let keybinds_raw = format!(
            " [↑↓] select  [Enter] play  [Space] pause  [←/→] seek ±5s  [n/p] move+play  [/] search  [a] add to playlist  [c] clear playlist  [v] view queue  [S] sort  [r] repeat:{}  [z] shuffle:{}  [s] save playlist  [o] open source  [+/-] volume  [q] quit",
            state.repeat.label(),
            if state.shuffle { "On" } else { "Off" }
        );
        let keybinds_inner_width = chunks[2].width.saturating_sub(2) as usize;
        let keybinds_display =
            if UnicodeWidthStr::width(keybinds_raw.as_str()) > keybinds_inner_width {
                marquee_cache.render(&keybinds_raw, keybinds_inner_width)
            } else {
                keybinds_raw
            };
        let keybinds = Paragraph::new(keybinds_display)
            .block(Block::default().borders(Borders::ALL))
            .style(Style::default().fg(Color::LightCyan));
        f.render_widget(keybinds, chunks[2]);
    }

    // ソース選択オーバーレイ（SourcePicker モード時のみ）
    if picker.mode == UiMode::SourcePicker {
        draw_source_picker(f, picker.entries, picker.selected);
    }
    // プレイリスト名入力オーバーレイ（NameInput モード時のみ）
    if picker.mode == UiMode::NameInput {
        draw_name_input(f, picker.name_input);
    }
    // ヘルプオーバーレイ（Help モード時のみ）
    if picker.mode == UiMode::Help {
        draw_help_overlay(f, picker.help_scroll);
    }
    // キュービューアーオーバーレイ（QueueViewer モード時のみ）
    if picker.mode == UiMode::QueueViewer {
        draw_queue_viewer(f, state, picker.queue_selected);
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
                .add_modifier(Modifier::REVERSED)
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
        .style(Style::default().fg(Color::Reset));

    f.render_widget(widget, area);
}

/// キュービューアーオーバーレイを描画する。
fn draw_queue_viewer(f: &mut ratatui::Frame, state: &crate::app::AppState, selected: usize) {
    use ratatui::widgets::Clear;

    let area = centered_rect(60, 70, f.area());
    f.render_widget(Clear, area);

    let tracks = state.playlist_tracks();

    let items: Vec<ListItem> = if tracks.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            " キューは空です",
            Style::default().fg(Color::Reset),
        )))]
    } else {
        tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mins = t.duration_secs / 60;
                let secs = t.duration_secs % 60;
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!(" {:>2}. ", i + 1),
                        Style::default().fg(Color::Reset),
                    ),
                    Span::styled(t.title.clone(), Style::default().fg(Color::Green)),
                    Span::styled(format!(" — {}", t.artist), Style::default().fg(Color::Cyan)),
                    Span::styled(
                        format!(" {:>2}:{:02}", mins, secs),
                        Style::default().fg(Color::Reset),
                    ),
                ]))
            })
            .collect()
    };

    let mut list_state = ListState::default();
    if !tracks.is_empty() {
        list_state.select(Some(selected));
    }

    let title = format!(
        " Queue  {} tracks  [↑↓] select  [Shift+↑↓] reorder  [d] remove  [Esc] close ",
        tracks.len()
    );
    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Magenta)),
        )
        .highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .add_modifier(Modifier::BOLD),
        );

    f.render_stateful_widget(list, area, &mut list_state);
}

/// キーバインドヘルプオーバーレイを描画する。
fn draw_help_overlay(f: &mut ratatui::Frame, scroll: u16) {
    use ratatui::widgets::Clear;

    let area = centered_rect(60, 80, f.area());
    f.render_widget(Clear, area);

    let text = vec![
        Line::from(Span::styled(
            " 通常操作 ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑ / ↓       ", Style::default().fg(Color::Cyan)),
            Span::raw("トラック選択"),
        ]),
        Line::from(vec![
            Span::styled("  Enter       ", Style::default().fg(Color::Cyan)),
            Span::raw("選択曲を再生"),
        ]),
        Line::from(vec![
            Span::styled("  Space       ", Style::default().fg(Color::Cyan)),
            Span::raw("再生 / 一時停止"),
        ]),
        Line::from(vec![
            Span::styled("  ← / →       ", Style::default().fg(Color::Cyan)),
            Span::raw("±5 秒シーク"),
        ]),
        Line::from(vec![
            Span::styled("  n / p       ", Style::default().fg(Color::Cyan)),
            Span::raw("次 / 前の曲へスキップして再生"),
        ]),
        Line::from(vec![
            Span::styled("  r           ", Style::default().fg(Color::Cyan)),
            Span::raw("リピートモード切り替え (Off → All → One)"),
        ]),
        Line::from(vec![
            Span::styled("  z           ", Style::default().fg(Color::Cyan)),
            Span::raw("シャッフル On / Off"),
        ]),
        Line::from(vec![
            Span::styled("  + / -       ", Style::default().fg(Color::Cyan)),
            Span::raw("音量 +5% / -5%"),
        ]),
        Line::from(vec![
            Span::styled("  /           ", Style::default().fg(Color::Cyan)),
            Span::raw("インクリメンタル検索"),
        ]),
        Line::from(vec![
            Span::styled("  a           ", Style::default().fg(Color::Cyan)),
            Span::raw("選択曲をプレイリストに追加"),
        ]),
        Line::from(vec![
            Span::styled("  c           ", Style::default().fg(Color::Cyan)),
            Span::raw("プレイリストをクリア"),
        ]),
        Line::from(vec![
            Span::styled("  v           ", Style::default().fg(Color::Cyan)),
            Span::raw("キュービューアーを開く"),
        ]),
        Line::from(vec![
            Span::styled("  S           ", Style::default().fg(Color::Cyan)),
            Span::raw("ソートキー切り替え (Default → Title → Artist → Album → Duration)"),
        ]),
        Line::from(vec![
            Span::styled("  s           ", Style::default().fg(Color::Cyan)),
            Span::raw("プレイリストを名前をつけて保存"),
        ]),
        Line::from(vec![
            Span::styled("  o           ", Style::default().fg(Color::Cyan)),
            Span::raw("ソースピッカーを開く"),
        ]),
        Line::from(vec![
            Span::styled("  ?           ", Style::default().fg(Color::Cyan)),
            Span::raw("このヘルプを表示"),
        ]),
        Line::from(vec![
            Span::styled("  q           ", Style::default().fg(Color::Cyan)),
            Span::raw("終了"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " 検索モード ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  文字入力    ", Style::default().fg(Color::Cyan)),
            Span::raw("クエリを追加してリアルタイム絞り込み"),
        ]),
        Line::from(vec![
            Span::styled("  Backspace   ", Style::default().fg(Color::Cyan)),
            Span::raw("クエリを 1 文字削除"),
        ]),
        Line::from(vec![
            Span::styled("  ↑ / ↓       ", Style::default().fg(Color::Cyan)),
            Span::raw("絞り込み結果内を移動"),
        ]),
        Line::from(vec![
            Span::styled("  Enter       ", Style::default().fg(Color::Cyan)),
            Span::raw("選択を確定して通常モードに戻る"),
        ]),
        Line::from(vec![
            Span::styled("  Esc         ", Style::default().fg(Color::Cyan)),
            Span::raw("検索をキャンセル"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " ソースピッカー内 ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  ↑ / ↓       ", Style::default().fg(Color::Cyan)),
            Span::raw("項目選択"),
        ]),
        Line::from(vec![
            Span::styled("  Enter       ", Style::default().fg(Color::Cyan)),
            Span::raw("選択したソースを読み込む"),
        ]),
        Line::from(vec![
            Span::styled("  d           ", Style::default().fg(Color::Cyan)),
            Span::raw("選択中のプレイリストを削除"),
        ]),
        Line::from(vec![
            Span::styled("  Esc         ", Style::default().fg(Color::Cyan)),
            Span::raw("ピッカーを閉じる"),
        ]),
    ];

    let widget = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" キーバインド一覧  [↑↓] スクロール  [任意のキー] 閉じる ")
                .border_style(Style::default().fg(Color::Green)),
        )
        .style(Style::default().fg(Color::Reset))
        .scroll((scroll, 0));

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

    // ── filter_tracks ──────────────────────────────────────────────

    fn make_track(title: &str, artist: &str) -> crate::models::TrackInfo {
        make_track_full(title, artist, "")
    }

    fn make_track_full(title: &str, artist: &str, album: &str) -> crate::models::TrackInfo {
        crate::models::TrackInfo {
            path: std::path::PathBuf::from("/dummy"),
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            duration_secs: 0,
        }
    }

    #[test]
    fn filter_tracks_empty_query_returns_all() {
        let tracks = vec![
            make_track("Song A", "Artist 1"),
            make_track("Song B", "Artist 2"),
        ];
        assert_eq!(filter_tracks(&tracks, ""), vec![0, 1]);
    }

    #[test]
    fn filter_tracks_matches_title_case_insensitive() {
        let tracks = vec![
            make_track("Rock Anthem", "Band"),
            make_track("Jazz Night", "Trio"),
        ];
        assert_eq!(filter_tracks(&tracks, "rock"), vec![0]);
        assert_eq!(filter_tracks(&tracks, "JAZZ"), vec![1]);
    }

    #[test]
    fn filter_tracks_matches_artist() {
        let tracks = vec![
            make_track("Title", "The Beatles"),
            make_track("Title", "Rolling Stones"),
        ];
        assert_eq!(filter_tracks(&tracks, "beatles"), vec![0]);
        assert_eq!(filter_tracks(&tracks, "stones"), vec![1]);
    }

    #[test]
    fn filter_tracks_no_match_returns_empty() {
        let tracks = vec![make_track("Song", "Artist")];
        assert!(filter_tracks(&tracks, "zzz").is_empty());
    }

    #[test]
    fn filter_tracks_matches_both_title_and_artist() {
        let tracks = vec![
            make_track("Love Song", "Artist"),
            make_track("Title", "Love Band"),
            make_track("Other", "Other"),
        ];
        assert_eq!(filter_tracks(&tracks, "love"), vec![0, 1]);
    }

    #[test]
    fn filter_tracks_matches_album() {
        let tracks = vec![
            make_track_full("Song A", "Artist A", "Blue Album"),
            make_track_full("Song B", "Artist B", "Red Album"),
            make_track_full("Song C", "Artist C", "Other"),
        ];
        assert_eq!(filter_tracks(&tracks, "blue"), vec![0]);
        assert_eq!(filter_tracks(&tracks, "album"), vec![0, 1]);
        assert_eq!(filter_tracks(&tracks, "BLUE"), vec![0]);
    }

    // ── TestBackend 描画テスト ──────────────────────────────────────

    fn make_terminal(width: u16, height: u16) -> ratatui::Terminal<ratatui::backend::TestBackend> {
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap()
    }

    #[test]
    fn draw_source_picker_no_panic() {
        let mut terminal = make_terminal(80, 24);
        let entries = vec![
            SourceEntry::Directory(std::path::PathBuf::from("/music")),
            SourceEntry::RecentDir(std::path::PathBuf::from("/recent")),
            SourceEntry::Playlist {
                path: std::path::PathBuf::from("/playlists/test.json"),
                name: "test".to_string(),
            },
        ];
        terminal
            .draw(|f| draw_source_picker(f, &entries, 0))
            .unwrap();
    }

    #[test]
    fn draw_source_picker_shows_entry_labels() {
        let mut terminal = make_terminal(80, 24);
        let entries = vec![
            SourceEntry::Directory(std::path::PathBuf::from("/music")),
            SourceEntry::RecentDir(std::path::PathBuf::from("/recent")),
            SourceEntry::Playlist {
                path: std::path::PathBuf::from("/p.json"),
                name: "MyList".to_string(),
            },
        ];
        terminal
            .draw(|f| draw_source_picker(f, &entries, 0))
            .unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("[Dir]"), "content should contain [Dir]");
        assert!(
            content.contains("[Recent]"),
            "content should contain [Recent]"
        );
        assert!(content.contains("[PL]"), "content should contain [PL]");
        assert!(
            content.contains("MyList"),
            "content should contain playlist name"
        );
    }

    #[test]
    fn draw_source_picker_empty_entries_no_panic() {
        let mut terminal = make_terminal(80, 24);
        terminal.draw(|f| draw_source_picker(f, &[], 0)).unwrap();
    }

    #[test]
    fn draw_name_input_no_panic() {
        let mut terminal = make_terminal(80, 24);
        terminal
            .draw(|f| draw_name_input(f, "my playlist"))
            .unwrap();
    }

    #[test]
    fn draw_name_input_shows_prompt() {
        let mut terminal = make_terminal(80, 24);
        terminal.draw(|f| draw_name_input(f, "rock")).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("rock"), "input buffer should be visible");
    }

    #[test]
    fn draw_help_overlay_shows_keybinds() {
        let mut terminal = make_terminal(80, 24);
        terminal.draw(|f| draw_help_overlay(f, 0)).unwrap();
        let content: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(content.contains("Enter"), "help should show Enter key");
        assert!(content.contains("Space"), "help should show Space key");
        // CJK 文字はバッファ上で1セルずつ分割されるため個別に確認する
        assert!(
            content.contains("通"),
            "help should show Normal section header"
        );
    }

    #[test]
    fn draw_help_overlay_scrolled_no_panic() {
        let mut terminal = make_terminal(80, 24);
        // 末尾を超えるオフセットを渡してもパニックしないこと
        terminal.draw(|f| draw_help_overlay(f, 999)).unwrap();
    }

    #[test]
    fn draw_help_overlay_small_terminal_no_panic() {
        // 極端に小さいターミナルでもパニックしないこと
        let mut terminal = make_terminal(20, 8);
        terminal.draw(|f| draw_help_overlay(f, 0)).unwrap();
    }
}
