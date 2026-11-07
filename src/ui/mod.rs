//! ratatui + crossterm TUI interface for ytcut.

use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::config::Config;
use crate::downloads::{self, Job, StartOptions, Status};
use crate::library::{self, Song};
use crate::player::Player;
use crate::tools::Tools;

mod palette;
mod render;
mod text;
mod theme;
mod widgets;

use palette::matching_commands;
use text::shlex_split;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Library,
    Playlist,
    Add,
    Command,
    Settings,
    Help,
    Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddField {
    Url,
    Name,
    Start,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsRow {
    Dir,
    Browser,
    Autoupdate,
    Volume,
}

impl SettingsRow {
    fn next(self) -> Self {
        match self {
            SettingsRow::Dir => SettingsRow::Browser,
            SettingsRow::Browser => SettingsRow::Autoupdate,
            SettingsRow::Autoupdate => SettingsRow::Volume,
            SettingsRow::Volume => SettingsRow::Dir,
        }
    }
    fn prev(self) -> Self {
        match self {
            SettingsRow::Dir => SettingsRow::Volume,
            SettingsRow::Browser => SettingsRow::Dir,
            SettingsRow::Autoupdate => SettingsRow::Browser,
            SettingsRow::Volume => SettingsRow::Autoupdate,
        }
    }
}

#[derive(Clone, Debug)]
struct SettingsState {
    selected: SettingsRow,
    editing_dir: bool,
    dir_buf: String,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            selected: SettingsRow::Dir,
            editing_dir: false,
            dir_buf: String::new(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum PromptType {
    Delete(String),
}

pub struct AppUi {
    pub library_dir: PathBuf,
    pub tools: Option<Tools>,
    pub player: Option<Player>,
    pub config: Config,
    pub songs: Vec<Song>,
    pub filtered_indices: Vec<usize>,
    pub lib_selected: usize,
    pub playlist_selected: usize,
    pub focus: Focus,
    pub jobs: Arc<Mutex<Vec<Job>>>,
    pub search_query: String,
    pub add_url: String,
    pub add_name: String,
    pub add_start: String,
    pub add_end: String,
    pub add_field: AddField,
    pub prompt: Option<PromptType>,
    pub status_message: Option<(String, Instant)>,
    pub command_input: String,
    pub command_palette_selected: usize,
    settings: SettingsState,
    pub last_completed_jobs: usize,
    pub should_quit: bool,
}

impl AppUi {
    pub fn new(library_dir: PathBuf, tools: Option<Tools>, config: Config) -> Self {
        let player = Player::try_new();
        if let Some(p) = &player {
            p.set_volume(config.volume());
        }
        let jobs = Arc::new(Mutex::new(Vec::new()));
        let mut app = Self {
            library_dir,
            tools,
            player,
            config,
            songs: Vec::new(),
            filtered_indices: Vec::new(),
            lib_selected: 0,
            playlist_selected: 0,
            focus: Focus::Library,
            jobs,
            search_query: String::new(),
            add_url: String::new(),
            add_name: String::new(),
            add_start: String::new(),
            add_end: String::new(),
            add_field: AddField::Url,
            prompt: None,
            status_message: None,
            command_input: String::new(),
            command_palette_selected: 0,
            settings: SettingsState::default(),
            last_completed_jobs: 0,
            should_quit: false,
        };
        app.refresh_songs();
        app
    }

    pub fn refresh_songs(&mut self) {
        if let Ok(songs) = library::list(&self.library_dir) {
            self.songs = songs;
        } else {
            self.songs.clear();
        }
        self.apply_search_filter();
    }

    pub fn apply_search_filter(&mut self) {
        let q = self.search_query.trim().to_lowercase();
        if q.is_empty() {
            self.filtered_indices = (0..self.songs.len()).collect();
        } else {
            self.filtered_indices = self
                .songs
                .iter()
                .enumerate()
                .filter(|(_, s)| s.name.to_lowercase().contains(&q))
                .map(|(i, _)| i)
                .collect();
        }
        if self.lib_selected >= self.filtered_indices.len() && !self.filtered_indices.is_empty() {
            self.lib_selected = self.filtered_indices.len() - 1;
        }
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_message = Some((msg.into(), Instant::now()));
    }

    pub fn run_loop<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        let tick_rate = Duration::from_millis(150);
        let mut last_tick = Instant::now();

        while !self.should_quit {
            // Periodic tick
            if let Some(player) = &mut self.player {
                player.tick();
            }

            // Auto-refresh library if any download completed
            let done_count = {
                let jobs = self.jobs.lock().unwrap();
                jobs.iter()
                    .filter(|j| j.status == Status::Done || j.status == Status::Exists)
                    .count()
            };
            if done_count != self.last_completed_jobs {
                self.last_completed_jobs = done_count;
                self.refresh_songs();
            }

            // Clear expired status messages (> 6s)
            if let Some((_, time)) = &self.status_message {
                if time.elapsed() > Duration::from_secs(6) {
                    self.status_message = None;
                }
            }

            terminal.draw(|f| self.render(f))?;

            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        self.handle_key(key.code, key.modifiers);
                    }
                }
            }

            if last_tick.elapsed() >= tick_rate {
                last_tick = Instant::now();
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        // Global quit check
        if code == KeyCode::Char('c') && modifiers.contains(KeyModifiers::CONTROL) {
            self.should_quit = true;
            return;
        }

        // Manual refresh check
        if code == KeyCode::F(5)
            || (code == KeyCode::Char('r') && modifiers.contains(KeyModifiers::CONTROL))
        {
            self.refresh_songs();
            self.set_status("Library refreshed.");
            return;
        }

        match self.focus {
            Focus::Prompt => self.handle_prompt_key(code),
            Focus::Add => self.handle_add_key(code),
            Focus::Command => self.handle_command_key(code),
            Focus::Settings => self.handle_settings_key(code),
            Focus::Help => self.handle_help_key(code),
            Focus::Library => self.handle_library_key(code),
            Focus::Playlist => self.handle_playlist_key(code),
        }
    }

    fn handle_prompt_key(&mut self, code: KeyCode) {
        match &self.prompt {
            Some(PromptType::Delete(name)) => match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let _ = library::delete(&self.library_dir, name);
                    self.set_status(format!("Deleted {name}"));
                    self.prompt = None;
                    self.focus = Focus::Library;
                    self.refresh_songs();
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.prompt = None;
                    self.focus = Focus::Library;
                }
                _ => {}
            },
            None => {
                self.focus = Focus::Library;
            }
        }
    }

    fn handle_command_key(&mut self, code: KeyCode) {
        let matching = matching_commands(&self.command_input);
        match code {
            KeyCode::Esc => {
                self.command_input.clear();
                self.command_palette_selected = 0;
                self.focus = Focus::Library;
            }
            KeyCode::Down | KeyCode::Tab => {
                if !matching.is_empty() {
                    self.command_palette_selected =
                        (self.command_palette_selected + 1) % matching.len();
                }
            }
            KeyCode::Up => {
                if !matching.is_empty() {
                    if self.command_palette_selected == 0 {
                        self.command_palette_selected = matching.len() - 1;
                    } else {
                        self.command_palette_selected -= 1;
                    }
                }
            }
            KeyCode::Enter => {
                let input = self.command_input.trim().to_string();
                if let Some(item) = matching.get(self.command_palette_selected) {
                    let clean = input.strip_prefix('/').unwrap_or(&input);
                    let has_args = clean.contains(char::is_whitespace);
                    let item_clean = item.cmd.strip_prefix('/').unwrap_or(item.cmd);
                    if !has_args
                        && (item.cmd == "/dir" || (item.cmd == "/search" && clean.is_empty()))
                        && (clean.is_empty() || clean == item_clean)
                    {
                        self.command_input = format!("{} ", item.cmd);
                        return;
                    }
                    if !clean.contains(char::is_whitespace) && clean == item_clean {
                        self.command_input.clear();
                        self.command_palette_selected = 0;
                        self.execute_command(item.cmd);
                        return;
                    }
                }
                self.command_input.clear();
                self.command_palette_selected = 0;
                self.execute_command(&input);
            }
            KeyCode::Backspace => {
                self.command_input.pop();
                self.command_palette_selected = 0;
            }
            KeyCode::Char(c) => {
                self.command_input.push(c);
                self.command_palette_selected = 0;
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, input: &str) {
        let trimmed = input.trim();
        let clean = trimmed.strip_prefix('/').unwrap_or(trimmed);
        let (cmd, rest) = match clean.split_once(char::is_whitespace) {
            Some((c, r)) => (c, r.trim()),
            None => (clean, ""),
        };
        match cmd {
            "settings" | "set" => {
                self.settings = SettingsState::default();
                self.focus = Focus::Settings;
            }
            "dir" | "folder" => {
                if rest.is_empty() {
                    self.set_status("Usage: /dir <path>");
                } else {
                    self.set_library_dir(PathBuf::from(rest));
                }
                self.focus = Focus::Library;
            }
            "add" => {
                if rest.is_empty() {
                    self.add_field = AddField::Url;
                    self.focus = Focus::Add;
                } else {
                    self.parse_and_start_download(rest);
                    self.focus = Focus::Library;
                }
            }
            "background" | "bg" => {
                self.play_in_background();
                self.focus = Focus::Library;
            }
            "stop" => {
                let stopped_bg = crate::background::stop().unwrap_or(false);
                let mut stopped_local = false;
                if let Some(player) = &mut self.player {
                    if !player.is_paused() {
                        player.pause();
                        stopped_local = true;
                    }
                }
                if stopped_bg && stopped_local {
                    self.set_status("Stopped background and paused local playback.");
                } else if stopped_bg {
                    self.set_status("Stopped background playback.");
                } else if stopped_local {
                    self.set_status("Paused local playback.");
                } else {
                    self.set_status("No playback is currently active.");
                }
                self.focus = Focus::Library;
            }
            "search" | "find" => {
                if rest.is_empty() {
                    self.search_query.clear();
                    self.apply_search_filter();
                    self.set_status("Search filter cleared.");
                } else {
                    self.search_query = rest.to_string();
                    self.apply_search_filter();
                    self.set_status(format!(
                        "Filter: \"{rest}\" ({} found, use /search to clear)",
                        self.filtered_indices.len()
                    ));
                }
                self.focus = Focus::Library;
            }
            "refresh" | "reload" => {
                self.refresh_songs();
                self.set_status(format!("Library refreshed ({} songs).", self.songs.len()));
                self.focus = Focus::Library;
            }
            "update" => {
                crate::update::update_now();
                self.set_status("Updating in the background…");
                self.focus = Focus::Library;
            }
            "help" | "?" => {
                self.focus = Focus::Help;
            }
            "quit" | "q" | "exit" => {
                self.should_quit = true;
            }
            _ => {
                if !clean.is_empty() {
                    self.set_status(format!(
                        "Unknown command: /{clean}. Type /help for commands."
                    ));
                }
                self.focus = Focus::Library;
            }
        }
    }

    fn handle_add_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc => {
                self.focus = Focus::Library;
            }
            KeyCode::Tab | KeyCode::Down => {
                self.add_field = match self.add_field {
                    AddField::Url => AddField::Name,
                    AddField::Name => AddField::Start,
                    AddField::Start => AddField::End,
                    AddField::End => AddField::Url,
                };
            }
            KeyCode::Up => {
                self.add_field = match self.add_field {
                    AddField::Url => AddField::End,
                    AddField::Name => AddField::Url,
                    AddField::Start => AddField::Name,
                    AddField::End => AddField::Start,
                };
            }
            KeyCode::Enter => {
                self.trigger_download();
            }
            KeyCode::Backspace => match self.add_field {
                AddField::Url => {
                    self.add_url.pop();
                }
                AddField::Name => {
                    self.add_name.pop();
                }
                AddField::Start => {
                    self.add_start.pop();
                }
                AddField::End => {
                    self.add_end.pop();
                }
            },
            KeyCode::Char(c) => match self.add_field {
                AddField::Url => {
                    self.add_url.push(c);
                }
                AddField::Name => {
                    self.add_name.push(c);
                }
                AddField::Start => {
                    self.add_start.push(c);
                }
                AddField::End => {
                    self.add_end.push(c);
                }
            },
            _ => {}
        }
    }

    fn parse_and_start_download(&mut self, rest: &str) {
        let parts = shlex_split(rest);
        if parts.is_empty() {
            self.set_status("Enter a URL first.");
            return;
        }
        let url = parts[0].clone();
        let mut name = String::new();
        let mut start = String::new();
        let mut end = String::new();

        let mut i = 1;
        while i < parts.len() {
            match parts[i].as_str() {
                "--name" | "-n" => {
                    if i + 1 < parts.len() {
                        name = parts[i + 1].clone();
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--start" | "-s" => {
                    if i + 1 < parts.len() {
                        start = parts[i + 1].clone();
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                "--end" | "-e" => {
                    if i + 1 < parts.len() {
                        end = parts[i + 1].clone();
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
                pos if !pos.starts_with('-') => {
                    if name.is_empty() {
                        name = pos.to_string();
                    } else if start.is_empty() {
                        start = pos.to_string();
                    } else if end.is_empty() {
                        end = pos.to_string();
                    }
                    i += 1;
                }
                _ => i += 1,
            }
        }
        self.add_url = url;
        self.add_name = name;
        self.add_start = start;
        self.add_end = end;
        let url = self.add_url.clone();
        self.start_download(url);
    }

    fn trigger_download(&mut self) {
        let url = self.add_url.clone();
        if url.trim().is_empty() {
            self.set_status("Enter a URL first.");
            return;
        }
        self.start_download(url);
        self.focus = Focus::Library;
    }

    fn start_download(&mut self, url: String) {
        let url = url.trim().to_string();
        if url.is_empty() {
            self.set_status("Enter a URL first.");
            return;
        }
        let Some(tools) = self.tools.clone() else {
            self.set_status("Tools are not ready yet.");
            return;
        };
        let browser = downloads::resolve_browser(self.config.browser.as_deref());
        let opts = StartOptions {
            url,
            name: self.add_name.trim().to_string(),
            start: self.add_start.trim().to_string(),
            end: self.add_end.trim().to_string(),
        };
        match downloads::start(
            &tools,
            &self.library_dir,
            Arc::clone(&self.jobs),
            opts,
            browser.as_deref(),
        ) {
            Ok(_) => {
                self.set_status("Download started.");
                self.add_url.clear();
                self.add_name.clear();
                self.add_start.clear();
                self.add_end.clear();
                self.add_field = AddField::Url;
            }
            Err(e) => {
                self.set_status(format!("Error: {e}"));
            }
        }
    }

    /// Playback controls and sign-in, shared by the library and queue panes.
    fn handle_common_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Char('o') => {
                let _ = open::that_detached("https://www.youtube.com/");
                self.set_status("Opened YouTube in browser for sign-in.");
                true
            }
            KeyCode::Char(' ') => {
                if let Some(player) = &mut self.player {
                    player.toggle_pause();
                }
                true
            }
            KeyCode::Char('n') => {
                if let Some(Err(e)) = self.player.as_mut().map(|p| p.next()) {
                    self.set_status(format!("Error: {e:#}"));
                }
                true
            }
            KeyCode::Char('p') => {
                if let Some(Err(e)) = self.player.as_mut().map(|p| p.prev()) {
                    self.set_status(format!("Error: {e:#}"));
                }
                true
            }
            KeyCode::Char('s') => {
                let msg = self.player.as_mut().map(|p| {
                    p.toggle_shuffle();
                    if p.shuffle {
                        "Shuffle: ON"
                    } else {
                        "Shuffle: OFF"
                    }
                });
                if let Some(msg) = msg {
                    self.set_status(msg);
                }
                true
            }
            KeyCode::Char('r') => {
                let msg = self.player.as_mut().map(|p| {
                    p.toggle_repeat();
                    if p.repeat_one {
                        "Repeat-one: ON"
                    } else {
                        "Repeat-one: OFF"
                    }
                });
                if let Some(msg) = msg {
                    self.set_status(msg);
                }
                true
            }
            _ => false,
        }
    }

    fn handle_library_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.focus = Focus::Playlist;
            }
            KeyCode::Char('/') => {
                self.command_input.clear();
                self.focus = Focus::Command;
            }
            KeyCode::Char('?') => {
                self.focus = Focus::Help;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if !self.filtered_indices.is_empty() {
                    self.lib_selected =
                        (self.lib_selected + 1).min(self.filtered_indices.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.lib_selected = self.lib_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                self.play_selected_library_song();
            }
            KeyCode::Char('d') => {
                if let Some(&idx) = self.filtered_indices.get(self.lib_selected) {
                    if let Some(song) = self.songs.get(idx) {
                        self.prompt = Some(PromptType::Delete(song.name.clone()));
                        self.focus = Focus::Prompt;
                    }
                }
            }
            _ => {
                self.handle_common_key(code);
            }
        }
    }

    fn handle_playlist_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab => {
                self.focus = Focus::Library;
            }
            KeyCode::Char('/') => {
                self.command_input.clear();
                self.focus = Focus::Command;
            }
            KeyCode::Char('?') => {
                self.focus = Focus::Help;
            }
            KeyCode::Char('j') | KeyCode::Down => {
                if let Some(player) = &self.player {
                    if !player.queue.is_empty() {
                        self.playlist_selected =
                            (self.playlist_selected + 1).min(player.queue.len() - 1);
                    }
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.playlist_selected = self.playlist_selected.saturating_sub(1);
            }
            KeyCode::Enter => {
                let result = self
                    .player
                    .as_mut()
                    .map(|p| p.play_index(self.playlist_selected));
                if let Some(Err(e)) = result {
                    self.set_status(format!("Error: {e:#}"));
                }
            }
            KeyCode::Char('d') => {
                if let Some(player) = &mut self.player {
                    player.remove_from_queue(self.playlist_selected);
                }
            }
            _ => {
                self.handle_common_key(code);
            }
        }
    }

    fn play_selected_library_song(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let song_idx = self.filtered_indices[self.lib_selected];
        let Some(song) = self.songs.get(song_idx) else {
            return;
        };
        let song_path = self.library_dir.join(&song.name);

        let result = if let Some(player) = &mut self.player {
            let target_idx = if let Some(pos) = player.queue.iter().position(|p| p == &song_path) {
                pos
            } else {
                player.add_to_queue(song_path);
                player.queue.len() - 1
            };
            player.play_index(target_idx)
        } else {
            Ok(())
        };
        if let Err(e) = result {
            self.set_status(format!("Error: {e:#}"));
        }
    }

    fn play_in_background(&mut self) {
        let (files, shuffle, repeat, start_secs) = if let Some(player) = &mut self.player {
            let cur_idx = player.current_song_index().unwrap_or(0);
            let pos_secs = player.position().as_secs();

            if player.queue.is_empty() {
                // If queue is empty, check if a song is selected in the library
                if let Some(&song_idx) = self.filtered_indices.get(self.lib_selected) {
                    if let Some(song) = self.songs.get(song_idx) {
                        (
                            vec![self.library_dir.join(&song.name)],
                            player.shuffle,
                            player.repeat_one,
                            None,
                        )
                    } else {
                        self.set_status("No music in library to play in background.");
                        return;
                    }
                } else if !self.songs.is_empty() {
                    let all_paths: Vec<PathBuf> = self
                        .songs
                        .iter()
                        .map(|s| self.library_dir.join(&s.name))
                        .collect();
                    (all_paths, player.shuffle, player.repeat_one, None)
                } else {
                    self.set_status("No music in library or queue to play in background.");
                    return;
                }
            } else {
                // Order files starting with current track, then subsequent tracks, then previous tracks
                let mut reordered = Vec::with_capacity(player.queue.len());
                if player.repeat_one {
                    if let Some(cur) = player.queue.get(cur_idx) {
                        reordered.push(cur.clone());
                    }
                } else {
                    reordered.extend_from_slice(&player.queue[cur_idx..]);
                    reordered.extend_from_slice(&player.queue[..cur_idx]);
                }
                let start = if pos_secs > 1 { Some(pos_secs) } else { None };
                (reordered, player.shuffle, player.repeat_one, start)
            }
        } else {
            self.set_status("Audio player is not available.");
            return;
        };

        if files.is_empty() {
            self.set_status("No tracks to play in background.");
            return;
        }

        let count = files.len();
        match crate::background::spawn(&self.library_dir, &files, shuffle, repeat, start_secs) {
            Ok(()) => {
                if let Some(player) = &mut self.player {
                    player.pause();
                }
                let song_title = files
                    .first()
                    .and_then(|p| p.file_name())
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "track".into());
                self.set_status(format!(
                    "Playing in background: {} ({} track{}). Stop with /stop or `ytcut stop`.",
                    song_title,
                    count,
                    if count == 1 { "" } else { "s" }
                ));
            }
            Err(e) => {
                self.set_status(format!("Error starting background playback: {e:#}"));
            }
        }
    }

    fn set_library_dir(&mut self, dir: PathBuf) {
        match std::fs::create_dir_all(&dir) {
            Ok(()) => {
                self.library_dir = dir.clone();
                self.config.library_dir = Some(dir.clone());
                if let Err(e) = self.config.save() {
                    self.set_status(format!("Could not save settings: {e}"));
                    return;
                }
                self.refresh_songs();
                self.set_status(format!("Library folder: {}", dir.display()));
            }
            Err(e) => self.set_status(format!("Could not create folder: {e}")),
        }
    }

    fn save_config(&mut self) {
        if let Err(e) = self.config.save() {
            self.set_status(format!("Could not save settings: {e}"));
        }
    }

    fn cycle_browser(&mut self) {
        let cur = self.config.browser.as_deref().unwrap_or("auto");
        let next = match cur {
            "auto" => "chrome",
            "chrome" => "chromium",
            "chromium" => "edge",
            "edge" => "brave",
            "brave" => "vivaldi",
            "vivaldi" => "none",
            _ => "auto",
        };
        self.config.browser = Some(next.to_string());
        self.save_config();
    }

    fn cycle_autoupdate(&mut self) {
        let cur = self.config.autoupdate.as_deref().unwrap_or("auto");
        let next = match cur {
            "auto" => "notify",
            "notify" => "off",
            _ => "auto",
        };
        self.config.autoupdate = Some(next.to_string());
        self.save_config();
    }

    fn adjust_volume(&mut self, delta: f32) {
        let v = (self.config.volume() + delta).clamp(0.0, 1.0);
        self.config.volume = Some(v);
        if let Some(p) = &self.player {
            p.set_volume(v);
        }
        self.save_config();
    }

    fn handle_settings_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.settings.editing_dir = false;
                self.focus = Focus::Library;
            }
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                self.settings.editing_dir = false;
                self.settings.selected = self.settings.selected.next();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.settings.editing_dir = false;
                self.settings.selected = self.settings.selected.prev();
            }
            KeyCode::Enter => match self.settings.selected {
                SettingsRow::Dir => {
                    if self.settings.editing_dir {
                        let dir = self.settings.dir_buf.trim().to_string();
                        if !dir.is_empty() {
                            self.set_library_dir(PathBuf::from(dir));
                        }
                        self.settings.editing_dir = false;
                    } else {
                        self.settings.editing_dir = true;
                        self.settings.dir_buf = self.library_dir.display().to_string();
                    }
                }
                SettingsRow::Browser => self.cycle_browser(),
                SettingsRow::Autoupdate => self.cycle_autoupdate(),
                SettingsRow::Volume => {}
            },
            KeyCode::Char('h') | KeyCode::Left => match self.settings.selected {
                SettingsRow::Browser => self.cycle_browser(),
                SettingsRow::Autoupdate => self.cycle_autoupdate(),
                SettingsRow::Volume => self.adjust_volume(-0.05),
                SettingsRow::Dir => {}
            },
            KeyCode::Char('l') | KeyCode::Right => match self.settings.selected {
                SettingsRow::Browser => self.cycle_browser(),
                SettingsRow::Autoupdate => self.cycle_autoupdate(),
                SettingsRow::Volume => self.adjust_volume(0.05),
                SettingsRow::Dir => {}
            },
            KeyCode::Backspace if self.settings.editing_dir => {
                self.settings.dir_buf.pop();
            }
            KeyCode::Char(c) if self.settings.editing_dir => {
                self.settings.dir_buf.push(c);
            }
            _ => {}
        }
    }

    fn handle_help_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') | KeyCode::Enter => {
                self.focus = Focus::Library;
            }
            _ => {}
        }
    }
}

#[cfg(unix)]
struct StderrSilencer {
    saved_fd: i32,
}

#[cfg(unix)]
impl StderrSilencer {
    fn new() -> Self {
        unsafe {
            use std::os::fd::AsRawFd;
            extern "C" {
                fn dup(fd: std::ffi::c_int) -> std::ffi::c_int;
                fn dup2(oldfd: std::ffi::c_int, newfd: std::ffi::c_int) -> std::ffi::c_int;
            }
            let saved_fd = dup(2);
            if let Ok(dev_null) = std::fs::File::open("/dev/null") {
                let _ = dup2(dev_null.as_raw_fd(), 2);
            }
            Self { saved_fd }
        }
    }
}

#[cfg(unix)]
impl Drop for StderrSilencer {
    fn drop(&mut self) {
        if self.saved_fd >= 0 {
            unsafe {
                extern "C" {
                    fn dup2(oldfd: std::ffi::c_int, newfd: std::ffi::c_int) -> std::ffi::c_int;
                    fn close(fd: std::ffi::c_int) -> std::ffi::c_int;
                }
                let _ = dup2(self.saved_fd, 2);
                let _ = close(self.saved_fd);
            }
        }
    }
}

pub fn run(library_dir: PathBuf, tools: Option<Tools>, config: Config) -> Result<()> {
    #[cfg(unix)]
    let _silencer = StderrSilencer::new();

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    crossterm::execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = AppUi::new(library_dir, tools, config);
    let res = app.run_loop(&mut terminal);

    disable_raw_mode()?;
    crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    res
}
