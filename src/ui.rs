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
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph};
use ratatui::Terminal;

use crate::config::Config;
use crate::downloads::{self, Job, StartOptions, Status};
use crate::library::{self, Song};
use crate::player::Player;
use crate::tools::Tools;
use crate::update::UPDATE_NOTICE;

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

#[derive(Clone, Copy, Debug)]
pub struct PaletteItem {
    pub cmd: &'static str,
    pub args: &'static str,
    pub desc: &'static str,
}

pub const COMMAND_PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        cmd: "/add",
        args: "<url> [--name <name>] [--start <s>] [--end <e>]",
        desc: "Download audio from YouTube",
    },
    PaletteItem {
        cmd: "/settings",
        args: "",
        desc: "Open settings (folder, browser, autoupdate, volume)",
    },
    PaletteItem {
        cmd: "/dir",
        args: "<path>",
        desc: "Set music library folder",
    },
    PaletteItem {
        cmd: "/update",
        args: "",
        desc: "Update yt-dlp and ytcut to the latest version",
    },
    PaletteItem {
        cmd: "/help",
        args: "",
        desc: "Open keybindings and command documentation",
    },
    PaletteItem {
        cmd: "/quit",
        args: "",
        desc: "Exit ytcut",
    },
];

pub fn matching_commands(input: &str) -> Vec<&'static PaletteItem> {
    let clean = input.trim().strip_prefix('/').unwrap_or(input.trim()).to_lowercase();
    let query_cmd = clean.split_whitespace().next().unwrap_or_default();

    if query_cmd.is_empty() {
        return COMMAND_PALETTE_ITEMS.iter().collect();
    }

    COMMAND_PALETTE_ITEMS
        .iter()
        .filter(|item| {
            let item_clean = item.cmd.strip_prefix('/').unwrap_or(item.cmd);
            item_clean.contains(query_cmd)
                || item.desc.to_lowercase().contains(query_cmd)
        })
        .collect()
}

#[derive(Clone, Debug)]
pub enum PromptType {
    Delete(String),
    Rename { old_name: String, input: String },
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
        match &mut self.prompt.clone() {
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
            Some(PromptType::Rename { old_name, input }) => match code {
                KeyCode::Esc => {
                    self.prompt = None;
                    self.focus = Focus::Library;
                }
                KeyCode::Enter => {
                    let new_input = input.trim();
                    if !new_input.is_empty() {
                        match library::rename(&self.library_dir, old_name, new_input) {
                            Ok(new_file) => {
                                self.set_status(format!("Renamed to {new_file}"));
                                self.refresh_songs();
                            }
                            Err(e) => {
                                self.set_status(format!("Error: {e}"));
                            }
                        }
                    }
                    self.prompt = None;
                    self.focus = Focus::Library;
                }
                KeyCode::Backspace => {
                    if let Some(PromptType::Rename { input, .. }) = &mut self.prompt {
                        input.pop();
                    }
                }
                KeyCode::Char(c) => {
                    if let Some(PromptType::Rename { input, .. }) = &mut self.prompt {
                        input.push(c);
                    }
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
            KeyCode::Up | KeyCode::BackTab => {
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
                        && (item.cmd == "/add" || item.cmd == "/dir")
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
                self.apply_command_filter();
            }
            KeyCode::Char(c) => {
                self.command_input.push(c);
                self.command_palette_selected = 0;
                self.apply_command_filter();
            }
            _ => {}
        }
    }

    /// A non-command input acts as a live search filter.
    fn apply_command_filter(&mut self) {
        if is_command(&self.command_input) {
            return;
        }
        let query = self
            .command_input
            .strip_prefix('/')
            .unwrap_or(&self.command_input);
        self.search_query = query.to_string();
        self.apply_search_filter();
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
                    self.set_status("Usage: /add <url> [--name <name>] [--start <s>] [--end <e>]");
                } else {
                    self.parse_and_start_download(rest);
                }
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
                self.search_query = clean.to_string();
                self.apply_search_filter();
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
            KeyCode::BackTab | KeyCode::Up => {
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

    fn handle_library_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::Playlist;
            }
            KeyCode::Char('a') => {
                self.focus = Focus::Add;
                self.add_field = AddField::Url;
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
            KeyCode::Char('+') => {
                self.add_selected_to_playlist();
            }
            KeyCode::Char('d') => {
                if let Some(&idx) = self.filtered_indices.get(self.lib_selected) {
                    if let Some(song) = self.songs.get(idx) {
                        self.prompt = Some(PromptType::Delete(song.name.clone()));
                        self.focus = Focus::Prompt;
                    }
                }
            }
            KeyCode::Char('R') => {
                if let Some(&idx) = self.filtered_indices.get(self.lib_selected) {
                    if let Some(song) = self.songs.get(idx) {
                        let stem = std::path::Path::new(&song.name)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| song.name.clone());
                        self.prompt = Some(PromptType::Rename {
                            old_name: song.name.clone(),
                            input: stem,
                        });
                        self.focus = Focus::Prompt;
                    }
                }
            }
            KeyCode::Char('o') => {
                let _ = open::that_detached("https://www.youtube.com/");
                self.set_status("Opened YouTube in browser for sign-in.");
            }
            // Player controls
            KeyCode::Char(' ') => {
                if let Some(player) = &mut self.player {
                    player.toggle_pause();
                }
            }
            KeyCode::Char('n') => {
                if let Some(Err(e)) = self.player.as_mut().map(|p| p.next()) {
                    self.set_status(format!("Error: {e:#}"));
                }
            }
            KeyCode::Char('p') => {
                if let Some(Err(e)) = self.player.as_mut().map(|p| p.prev()) {
                    self.set_status(format!("Error: {e:#}"));
                }
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
            }
            _ => {}
        }
    }

    fn handle_playlist_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
            }
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::Library;
            }
            KeyCode::Char('a') => {
                self.focus = Focus::Add;
                self.add_field = AddField::Url;
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
            KeyCode::Char('o') => {
                let _ = open::that_detached("https://www.youtube.com/");
                self.set_status("Opened YouTube in browser for sign-in.");
            }
            // Player controls
            KeyCode::Char(' ') => {
                if let Some(player) = &mut self.player {
                    player.toggle_pause();
                }
            }
            KeyCode::Char('n') => {
                if let Some(Err(e)) = self.player.as_mut().map(|p| p.next()) {
                    self.set_status(format!("Error: {e:#}"));
                }
            }
            KeyCode::Char('p') => {
                if let Some(Err(e)) = self.player.as_mut().map(|p| p.prev()) {
                    self.set_status(format!("Error: {e:#}"));
                }
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
            }
            _ => {}
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
            // Populate the queue with all songs if it is empty, else play in place.
            if player.queue.is_empty() {
                let all_paths: Vec<PathBuf> = self
                    .songs
                    .iter()
                    .map(|s| self.library_dir.join(&s.name))
                    .collect();
                player.set_queue(all_paths);
                player.play_index(song_idx)
            } else {
                player.play_file(&song_path)
            }
        } else {
            Ok(())
        };
        if let Err(e) = result {
            self.set_status(format!("Error: {e:#}"));
        }
    }

    fn add_selected_to_playlist(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        let song_idx = self.filtered_indices[self.lib_selected];
        if let Some(song) = self.songs.get(song_idx) {
            let path = self.library_dir.join(&song.name);
            if let Some(player) = &mut self.player {
                player.add_to_queue(path);
                self.set_status(format!("Added {} to queue", song.name));
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
            KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
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

    fn render(&self, f: &mut ratatui::Frame) {
        let size = f.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(4), // Add pane & downloads
                Constraint::Min(8),    // Library & Playlist
                Constraint::Length(3), // Player bar
                Constraint::Length(1), // Status / update / help bar
            ])
            .split(size);

        self.render_add_pane(f, chunks[0]);
        self.render_middle_columns(f, chunks[1]);
        self.render_player_bar(f, chunks[2]);
        self.render_bottom_status(f, chunks[3]);

        if let Some(prompt) = &self.prompt {
            self.render_prompt(f, prompt, size);
        }
        if self.focus == Focus::Command {
            self.render_command_palette(f, size);
        }
        if self.focus == Focus::Settings {
            self.render_settings(f, size);
        }
        if self.focus == Focus::Help {
            self.render_help(f, size);
        }
    }

    fn render_add_pane(&self, f: &mut ratatui::Frame, area: Rect) {
        let is_focused = self.focus == Focus::Add;
        let border_style = if is_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let title = if is_focused {
            " Add [Tab / Shift+Tab field · Esc cancel] "
        } else {
            " Add [a] "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        f.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // Inputs
                Constraint::Percentage(50), // Latest job status / progress
            ])
            .split(inner);

        // Inputs line: URL [name] [start-end]
        let spans = vec![
            Span::styled("URL: ", Style::default().fg(Color::Cyan)),
            Span::styled(
                if self.add_url.is_empty()
                    && self.focus == Focus::Add
                    && self.add_field == AddField::Url
                {
                    " "
                } else if self.add_url.is_empty() {
                    "<enter url>"
                } else {
                    &self.add_url
                },
                if is_focused && self.add_field == AddField::Url {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::UNDERLINED)
                } else if self.add_url.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
            Span::raw("  Name: "),
            Span::styled(
                if self.add_name.is_empty()
                    && self.focus == Focus::Add
                    && self.add_field == AddField::Name
                {
                    " "
                } else if self.add_name.is_empty() {
                    "<optional>"
                } else {
                    &self.add_name
                },
                if is_focused && self.add_field == AddField::Name {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::UNDERLINED)
                } else if self.add_name.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
            Span::raw("  Cut: "),
            Span::styled(
                if self.add_start.is_empty()
                    && self.focus == Focus::Add
                    && self.add_field == AddField::Start
                {
                    " "
                } else if self.add_start.is_empty() {
                    "0"
                } else {
                    &self.add_start
                },
                if is_focused && self.add_field == AddField::Start {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::UNDERLINED)
                } else if self.add_start.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
            Span::raw("-"),
            Span::styled(
                if self.add_end.is_empty()
                    && self.focus == Focus::Add
                    && self.add_field == AddField::End
                {
                    " "
                } else if self.add_end.is_empty() {
                    "end"
                } else {
                    &self.add_end
                },
                if is_focused && self.add_field == AddField::End {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::UNDERLINED)
                } else if self.add_end.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ];

        f.render_widget(Paragraph::new(Line::from(spans)), cols[0]);

        // Job status / progress
        let jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.last() {
            let label_span = Span::styled(
                truncate_str(&job.label, 20),
                Style::default().fg(Color::Cyan),
            );
            match job.status {
                Status::Running => {
                    let gauge = Gauge::default()
                        .block(Block::default())
                        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
                        .ratio((job.progress / 100.0).clamp(0.0, 1.0))
                        .label(format!("{}: {:.0}%", label_span.content, job.progress));
                    f.render_widget(gauge, cols[1]);
                }
                Status::Done => {
                    let text = format!("✓ Saved: {}", job.message);
                    f.render_widget(
                        Paragraph::new(text).style(Style::default().fg(Color::Green)),
                        cols[1],
                    );
                }
                Status::Exists => {
                    let text = format!("✓ Already exists: {}", job.message);
                    f.render_widget(
                        Paragraph::new(text).style(Style::default().fg(Color::Blue)),
                        cols[1],
                    );
                }
                Status::Error => {
                    let text = format!("✕ Error: {}", job.message);
                    f.render_widget(
                        Paragraph::new(text).style(Style::default().fg(Color::Red)),
                        cols[1],
                    );
                }
            }
        }
    }

    fn render_middle_columns(&self, f: &mut ratatui::Frame, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(60), // Library
                Constraint::Percentage(40), // Playlist/queue
            ])
            .split(area);

        // Library pane
        let lib_focused = self.focus == Focus::Library;
        let lib_border = if lib_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let lib_title = if !self.search_query.is_empty() {
            format!(" Library (Filter: {}) ", self.search_query)
        } else {
            " Library ".to_string()
        };

        let lib_block = Block::default()
            .borders(Borders::ALL)
            .title(lib_title)
            .border_style(lib_border);

        let items: Vec<ListItem> = self
            .filtered_indices
            .iter()
            .enumerate()
            .map(|(display_idx, &song_idx)| {
                let song = &self.songs[song_idx];
                let is_selected = display_idx == self.lib_selected && lib_focused;

                let marker = if is_selected { "▶ " } else { "  " };
                let size_str = song.formatted_size();

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                // Split name and size nicely
                let content = format!(
                    "{marker}{:<40} {:>8}",
                    truncate_str(&song.name, 40),
                    size_str
                );
                ListItem::new(content).style(style)
            })
            .collect();

        let list = List::new(items).block(lib_block);
        f.render_widget(list, cols[0]);

        // Playlist pane
        let pl_focused = self.focus == Focus::Playlist;
        let pl_border = if pl_focused {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let pl_block = Block::default()
            .borders(Borders::ALL)
            .title(" Queue ")
            .border_style(pl_border);

        let pl_items: Vec<ListItem> = match &self.player {
            Some(player) => player
                .queue
                .iter()
                .enumerate()
                .map(|(idx, path)| {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let is_current = player.current_song_index() == Some(idx);
                    let is_selected = idx == self.playlist_selected && pl_focused;

                    let prefix = if is_current { "▶ " } else { "  " };
                    let num = format!("{}. ", idx + 1);

                    let mut style = Style::default();
                    if is_selected {
                        style = style
                            .fg(Color::Black)
                            .bg(Color::Yellow)
                            .add_modifier(Modifier::BOLD);
                    } else if is_current {
                        style = style.fg(Color::Green).add_modifier(Modifier::BOLD);
                    } else {
                        style = style.fg(Color::White);
                    }

                    ListItem::new(format!("{prefix}{num}{name}")).style(style)
                })
                .collect(),
            None => vec![ListItem::new("No audio device available")],
        };

        let pl_list = List::new(pl_items).block(pl_block);
        f.render_widget(pl_list, cols[1]);
    }

    fn render_player_bar(&self, f: &mut ratatui::Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Now playing ")
            .border_style(Style::default().fg(Color::Cyan));

        f.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let Some(player) = &self.player else {
            f.render_widget(
                Paragraph::new("Audio output disabled (no sound card found)")
                    .style(Style::default().fg(Color::Red)),
                inner,
            );
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),      // Play/pause icon
                Constraint::Percentage(35), // Song name
                Constraint::Percentage(45), // Seek bar
                Constraint::Length(12),     // Time
                Constraint::Length(8),      // Indicators (shuffle/repeat)
            ])
            .split(inner);

        // Play/Pause icon
        let icon = if player.is_paused() {
            "⏸"
        } else if player.is_started {
            "▶"
        } else {
            "⏹"
        };
        f.render_widget(
            Paragraph::new(icon).style(
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            chunks[0],
        );

        // Song title
        let current_title = player
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "No track playing".to_string());
        f.render_widget(
            Paragraph::new(truncate_str(&current_title, 30)).style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            chunks[1],
        );

        // Seek gauge
        let ratio = player.progress_ratio();
        let gauge = Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .ratio(ratio);
        f.render_widget(gauge, chunks[2]);

        // Time
        let time_str = player.progress_str();
        f.render_widget(
            Paragraph::new(time_str)
                .alignment(Alignment::Right)
                .style(Style::default().fg(Color::DarkGray)),
            chunks[3],
        );

        // Shuffle / Repeat icons
        let rep_str = if player.repeat_one { "🔂" } else { "  " };
        let shf_str = if player.shuffle { "🔀" } else { "  " };
        f.render_widget(Paragraph::new(format!("{rep_str} {shf_str}")), chunks[4]);
    }

    fn render_bottom_status(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut spans = Vec::new();

        if let Some((msg, _)) = &self.status_message {
            spans.push(Span::styled(
                format!(" {msg} "),
                Style::default().fg(Color::Yellow),
            ));
        } else if let Ok(guard) = UPDATE_NOTICE.lock() {
            if let Some(notice) = guard.as_ref() {
                spans.push(Span::styled(
                    format!(" ★ {notice} "),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
                ));
            }
        }

        // Check if any job failed requiring sign in
        let jobs = self.jobs.lock().unwrap();
        if jobs
            .iter()
            .any(|j| j.status == Status::Error && j.message.contains("Sign in"))
        {
            spans.push(Span::styled(
                " [Sign-in needed: press 'o' to open YouTube in browser] ",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ));
        }

        if spans.is_empty() {
            spans.push(Span::styled(
                " Tab / Shift+Tab pane · space pause · a add · R rename · / commands · ? help · q quit ",
                Style::default().fg(Color::DarkGray),
            ));
        }

        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_command_palette(&self, f: &mut ratatui::Frame, area: Rect) {
        let matching = matching_commands(&self.command_input);
        let count = matching.len().max(1);
        let height = ((count as u16) + 4).min(12).min(area.height.saturating_sub(4));
        let width = 74.min(area.width.saturating_sub(4));

        let popup = Rect {
            x: area.x + (area.width.saturating_sub(width)) / 2,
            y: area.y + 2,
            width,
            height,
        };
        f.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Command Palette [Tab / Shift+Tab select · Esc close] ")
            .border_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
        f.render_widget(block, popup);

        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 1,
        });

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // input line
                Constraint::Length(1), // separator
                Constraint::Min(1),    // suggestions
            ])
            .split(inner);

        let clean = self
            .command_input
            .strip_prefix('/')
            .unwrap_or(&self.command_input);
        let input_text = format!("> /{clean}_");
        f.render_widget(
            Paragraph::new(input_text).style(
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            chunks[0],
        );

        let sep: String = "─".repeat(chunks[1].width as usize);
        f.render_widget(
            Paragraph::new(sep).style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );

        let items: Vec<ListItem> = if matching.is_empty() {
            vec![ListItem::new(Span::styled(
                format!("  Search library: \"{clean}\""),
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            matching
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let is_selected = idx == self.command_palette_selected;
                    let marker = if is_selected { "▶ " } else { "  " };
                    let cmd_text = if item.args.is_empty() {
                        format!("{marker}{:<10} ", item.cmd)
                    } else {
                        format!("{marker}{} {} ", item.cmd, item.args)
                    };

                    let (cmd_style, desc_style, bg_style) = if is_selected {
                        (
                            Style::default().fg(Color::Black).add_modifier(Modifier::BOLD),
                            Style::default().fg(Color::Black),
                            Style::default().bg(Color::Yellow).fg(Color::Black),
                        )
                    } else {
                        (
                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                            Style::default().fg(Color::DarkGray),
                            Style::default(),
                        )
                    };

                    let line = Line::from(vec![
                        Span::styled(cmd_text, cmd_style),
                        Span::styled(item.desc, desc_style),
                    ]);
                    ListItem::new(line).style(bg_style)
                })
                .collect()
        };

        f.render_widget(List::new(items), chunks[2]);
    }

    fn dir_display(&self) -> String {
        self.library_dir.display().to_string()
    }

    fn settings_line(&self, row: SettingsRow, label: &str, value: String) -> ListItem<'static> {
        let selected = self.settings.selected == row;
        let marker = if selected { "> " } else { "  " };
        let text = if row == SettingsRow::Dir && self.settings.editing_dir {
            format!("{marker}{label}: {}_", self.settings.dir_buf)
        } else {
            format!("{marker}{label}: {value}")
        };
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        ListItem::new(text).style(style)
    }

    fn render_settings(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(72, 55, area);
        f.render_widget(Clear, popup);

        let rows = vec![
            self.settings_line(SettingsRow::Dir, "Library folder", self.dir_display()),
            self.settings_line(
                SettingsRow::Browser,
                "Sign-in browser",
                self.config.browser.clone().unwrap_or_else(|| "auto".into()),
            ),
            self.settings_line(
                SettingsRow::Autoupdate,
                "Auto-update",
                self.config
                    .autoupdate
                    .clone()
                    .unwrap_or_else(|| "auto".into()),
            ),
            self.settings_line(
                SettingsRow::Volume,
                "Volume",
                volume_bar(self.config.volume()),
            ),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Settings [Tab / Shift+Tab field · Esc back] ")
            .border_style(Style::default().fg(Color::Yellow));
        f.render_widget(block, popup);

        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 2,
        });
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(4), Constraint::Length(1)])
            .split(inner);
        f.render_widget(List::new(rows), chunks[0]);
        f.render_widget(
            Paragraph::new(" Tab / Shift+Tab field · Esc back ")
                .style(Style::default().fg(Color::DarkGray)),
            chunks[1],
        );
    }

    fn render_help(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(72, 85, area);
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Help — Esc to close ")
            .border_style(Style::default().fg(Color::Cyan));
        let text = "\n\
        Navigation\n\
        \n\
          Tab           next pane / field\n\
          Shift+Tab     previous pane / field\n\
          j / k         move up / down\n\
          +             add to queue\n\
          a             add song (URL)\n\
          R             rename song\n\
          d             delete song\n\
          /             command palette & search\n\
          ?             this help\n\
          q / Esc       back / quit\n\
        \n\
        Playback\n\
        \n\
          space         play / pause\n\
          n / p         next / previous\n\
          s             toggle shuffle\n\
          r             toggle repeat one\n\
          o             open YouTube in browser (sign in)\n\
        \n\
        Commands (/)\n\
        \n\
          /add <args>   download audio (/add <url> [--name <name>] [--start <s>] [--end <e>])\n\
          /settings     open settings (folder, browser, auto-update, volume)\n\
          /dir <path>   switch or create library folder\n\
          /update       update yt-dlp and ytcut to the latest version\n\
          /help         show this help documentation\n\
          /quit         exit application\n\
        ";
        f.render_widget(Paragraph::new(text).block(block), popup);
    }

    fn render_prompt(&self, f: &mut ratatui::Frame, prompt: &PromptType, area: Rect) {
        let popup_area = centered_rect(60, 20, area);
        f.render_widget(Clear, popup_area);

        match prompt {
            PromptType::Delete(name) => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(" Confirm Delete ")
                    .border_style(Style::default().fg(Color::Red));
                let text = format!(
                    "\n  Delete '{name}'?\n\n  [y] confirm, [n] or [Esc] cancel"
                );
                let p = Paragraph::new(text).block(block);
                f.render_widget(p, popup_area);
            }
            PromptType::Rename { old_name, input } => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Rename '{old_name}' "))
                    .border_style(Style::default().fg(Color::Yellow));
                let text = format!("\n  New name: {input}_\n\n  [Esc] cancel");
                let p = Paragraph::new(text).block(block);
                f.render_widget(p, popup_area);
            }
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

fn is_command(input: &str) -> bool {
    let cmd = input.split_whitespace().next().unwrap_or("");
    matches!(
        cmd,
        "settings"
            | "set"
            | "dir"
            | "folder"
            | "add"
            | "update"
            | "help"
            | "quit"
            | "q"
            | "exit"
            | "?"
    )
}

fn volume_bar(volume: f32) -> String {
    let v = volume.clamp(0.0, 1.0);
    let filled = (v * 10.0).round() as usize;
    let bar: String = (0..10)
        .map(|i| if i < filled { '=' } else { '-' })
        .collect();
    format!("[{bar}] {}%", (v * 100.0).round() as i32)
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn shlex_split(input: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quote: Option<char> = None;
    let mut escape = false;

    for c in input.chars() {
        if escape {
            current.push(c);
            escape = false;
        } else if c == '\\' {
            escape = true;
        } else if let Some(q) = in_quote {
            if c == q {
                in_quote = None;
            } else {
                current.push(c);
            }
        } else if c == '"' || c == '\'' {
            in_quote = Some(c);
        } else if c.is_whitespace() {
            if !current.is_empty() {
                args.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        args.push(current);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shlex_split_handles_quotes_and_flags() {
        assert_eq!(
            shlex_split("https://youtu.be/xyz --name \"My Song\" --start 1:00 --end 2:00"),
            vec![
                "https://youtu.be/xyz",
                "--name",
                "My Song",
                "--start",
                "1:00",
                "--end",
                "2:00"
            ]
        );
        assert_eq!(
            shlex_split("https://youtu.be/xyz 'song name' 10 20"),
            vec!["https://youtu.be/xyz", "song name", "10", "20"]
        );
    }

    #[test]
    fn matching_commands_filters_correctly() {
        assert_eq!(matching_commands("").len(), COMMAND_PALETTE_ITEMS.len());
        assert_eq!(matching_commands("/").len(), COMMAND_PALETTE_ITEMS.len());
        assert_eq!(matching_commands("/set")[0].cmd, "/settings");
        assert_eq!(matching_commands("quit")[0].cmd, "/quit");
        assert_eq!(matching_commands("/add")[0].cmd, "/add");
    }
}
