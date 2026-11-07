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
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::Terminal;

use crate::config::Config;
use crate::downloads::{self, Job, StartOptions, Status};
use crate::library::{self, Song};
use crate::player::Player;
use crate::tools::Tools;
use crate::update::UPDATE_NOTICE;

// ─── Theme ────────────────────────────────────────────────────────────────────

mod theme {
    use ratatui::style::Color;

    /// Near-black base with a cool blue tint.
    pub const BG: Color = Color::Rgb(15, 16, 25);
    /// Panel and popup background.
    pub const SURFACE: Color = Color::Rgb(22, 24, 38);
    /// Slightly raised surface, e.g. inactive button chips.
    pub const SURFACE_ALT: Color = Color::Rgb(31, 34, 54);
    /// Inactive panel borders.
    pub const BORDER: Color = Color::Rgb(52, 58, 88);
    /// Primary text.
    pub const FG: Color = Color::Rgb(201, 207, 233);
    /// Dim text, hints, file sizes.
    pub const MUTED: Color = Color::Rgb(96, 105, 150);
    /// Accent: focus, selection, progress.
    pub const ACCENT: Color = Color::Rgb(122, 162, 247);
    /// Playing / success.
    pub const GREEN: Color = Color::Rgb(158, 206, 106);
    /// Status messages.
    pub const YELLOW: Color = Color::Rgb(224, 175, 104);
    /// Errors and destructive actions.
    pub const RED: Color = Color::Rgb(247, 118, 142);
    /// Update notices.
    pub const MAGENTA: Color = Color::Rgb(187, 154, 247);
}

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
    pub alias: &'static str,
    pub desc: &'static str,
}

/// Fixed width of the command column in the palette so descriptions line up.
const COMMAND_COL_WIDTH: usize = 22;

pub const COMMAND_PALETTE_ITEMS: &[PaletteItem] = &[
    PaletteItem {
        cmd: "/add",
        alias: "",
        desc: "Download a song",
    },
    PaletteItem {
        cmd: "/background",
        alias: "/bg",
        desc: "Play in background",
    },
    PaletteItem {
        cmd: "/stop",
        alias: "",
        desc: "Stop background audio",
    },
    PaletteItem {
        cmd: "/search",
        alias: "",
        desc: "Filter the library",
    },
    PaletteItem {
        cmd: "/refresh",
        alias: "",
        desc: "Reload the library",
    },
    PaletteItem {
        cmd: "/settings",
        alias: "",
        desc: "Open settings",
    },
    PaletteItem {
        cmd: "/dir",
        alias: "",
        desc: "Change library folder",
    },
    PaletteItem {
        cmd: "/update",
        alias: "",
        desc: "Update ytcut",
    },
    PaletteItem {
        cmd: "/help",
        alias: "",
        desc: "Show help",
    },
    PaletteItem {
        cmd: "/quit",
        alias: "",
        desc: "Quit",
    },
];

pub fn matching_commands(input: &str) -> Vec<&'static PaletteItem> {
    let clean = input
        .trim()
        .strip_prefix('/')
        .unwrap_or(input.trim())
        .to_lowercase();
    let query_cmd = clean.split_whitespace().next().unwrap_or_default();

    if query_cmd.is_empty() {
        return COMMAND_PALETTE_ITEMS.iter().collect();
    }

    COMMAND_PALETTE_ITEMS
        .iter()
        .filter(|item| {
            let item_clean = item.cmd.strip_prefix('/').unwrap_or(item.cmd);
            let alias_clean = item.alias.strip_prefix('/').unwrap_or(item.alias);
            item_clean.contains(query_cmd)
                || (!alias_clean.is_empty() && alias_clean.contains(query_cmd))
                || item.desc.to_lowercase().contains(query_cmd)
        })
        .collect()
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

    fn render(&self, f: &mut ratatui::Frame) {
        let size = f.area();

        // A single base surface so the whole screen reads as one app.
        f.render_widget(Block::default().style(Style::default().bg(theme::BG)), size);

        let has_running_job = {
            let jobs = self.jobs.lock().unwrap();
            jobs.iter().any(|j| j.status == Status::Running)
        };

        let mut constraints: Vec<Constraint> = Vec::new();
        if has_running_job {
            constraints.push(Constraint::Length(1)); // download banner
        }
        constraints.push(Constraint::Length(1)); // header
        constraints.push(Constraint::Min(8)); // library & queue
        constraints.push(Constraint::Length(3)); // player bar
        constraints.push(Constraint::Length(1)); // status bar

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .spacing(1)
            .split(size);

        let mut idx = 0;
        let banner = if has_running_job {
            let area = chunks[idx];
            idx += 1;
            Some(area)
        } else {
            None
        };
        let header = chunks[idx];
        idx += 1;
        let middle = chunks[idx];
        idx += 1;
        let player = chunks[idx];
        idx += 1;
        let status = chunks[idx];

        if let Some(banner) = banner {
            self.render_download_banner(f, banner);
        }
        self.render_header(f, header);
        self.render_middle_columns(f, middle);
        self.render_player_bar(f, player);
        self.render_bottom_status(f, status);

        if let Some(prompt) = &self.prompt {
            self.render_prompt(f, prompt, size);
        }
        if self.focus == Focus::Add {
            self.render_add_dialog(f, size);
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

    fn render_header(&self, f: &mut ratatui::Frame, area: Rect) {
        let version = env!("CARGO_PKG_VERSION");
        let count = self.songs.len();
        let songs = if count == 1 { "song" } else { "songs" };
        let dir = truncate_str(&self.library_dir.display().to_string(), 40);

        let wordmark = " ytcut";
        let version = format!(" v{version} ");
        let right = format!("{count} {songs} · {dir}  ");

        let width = area.width as usize;
        let left_w = wordmark.chars().count() + version.chars().count();
        let right_w = right.chars().count();
        let pad = width.saturating_sub(left_w + right_w);

        let line = Line::from(vec![
            Span::styled(
                wordmark,
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(version, Style::default().fg(theme::MUTED)),
            Span::raw(" ".repeat(pad)),
            Span::styled(right, Style::default().fg(theme::MUTED)),
        ]);

        f.render_widget(
            Paragraph::new(line).style(Style::default().bg(theme::BG)),
            area,
        );
    }

    fn render_download_banner(&self, f: &mut ratatui::Frame, area: Rect) {
        let jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter().rev().find(|j| j.status == Status::Running) {
            let label = truncate_str(&job.label, 26);
            let bar_w = area.width.saturating_sub(22).max(8);

            let mut spans: Vec<Span> = vec![
                Span::styled(
                    " ↓ ",
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("{label} "), Style::default().fg(theme::FG)),
            ];
            spans.extend(progress_bar(job.progress / 100.0, bar_w));
            spans.push(Span::styled(
                format!(" {:.0}%", job.progress),
                Style::default().fg(theme::MUTED),
            ));

            f.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::BG)),
                area,
            );
        }
    }

    fn render_add_dialog(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(68, 52, area);
        f.render_widget(Clear, popup);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Add from YouTube ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(block, popup);

        let inner = popup.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 2,
        });

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // URL
                Constraint::Length(2), // Name
                Constraint::Length(2), // Cut
                Constraint::Min(1),    // spacer
                Constraint::Length(1), // footer
            ])
            .split(inner);

        let url_focused = self.add_field == AddField::Url;
        let url_line = Line::from(vec![
            field_label("URL", url_focused),
            field_value(&self.add_url, "<paste or type a YouTube URL>", url_focused),
        ]);
        f.render_widget(Paragraph::new(url_line), rows[0]);

        let name_focused = self.add_field == AddField::Name;
        let name_line = Line::from(vec![
            field_label("Name", name_focused),
            field_value(
                &self.add_name,
                "<optional — defaults to the video title>",
                name_focused,
            ),
        ]);
        f.render_widget(Paragraph::new(name_line), rows[1]);

        let start_focused = self.add_field == AddField::Start;
        let end_focused = self.add_field == AddField::End;
        let cut_active = start_focused || end_focused;
        let cut_line = Line::from(vec![
            field_label("Cut", cut_active),
            Span::styled("Start ", Style::default().fg(theme::MUTED)),
            field_value(&self.add_start, "0:00", start_focused),
            Span::styled("  End ", Style::default().fg(theme::MUTED)),
            field_value(&self.add_end, "end", end_focused),
        ]);
        f.render_widget(Paragraph::new(cut_line), rows[2]);

        f.render_widget(
            Paragraph::new(Span::styled(
                "Tab · next field      Enter · download      Esc · cancel",
                Style::default().fg(theme::MUTED),
            )),
            rows[4],
        );
    }

    fn render_middle_columns(&self, f: &mut ratatui::Frame, area: Rect) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .spacing(1)
            .split(area);

        self.render_library_pane(f, cols[0]);
        self.render_queue_pane(f, cols[1]);
    }

    fn render_library_pane(&self, f: &mut ratatui::Frame, area: Rect) {
        let focused = self.focus == Focus::Library;
        let title = if self.search_query.is_empty() {
            " Library ".to_string()
        } else {
            format!(" Library · “{}” ", truncate_str(&self.search_query, 20))
        };
        let block = panel(title, focused);

        if self.filtered_indices.is_empty() {
            let hint = if self.songs.is_empty() {
                "No songs yet — run /add <url> to download one"
            } else {
                "No songs match your search"
            };
            f.render_widget(
                Paragraph::new(hint)
                    .style(Style::default().fg(theme::MUTED))
                    .block(block),
                area,
            );
            return;
        }

        let name_max = area.width.saturating_sub(8) as usize;
        let mut items: Vec<ListItem> = Vec::with_capacity(self.filtered_indices.len() * 2);
        let mut starts: Vec<usize> = Vec::with_capacity(self.filtered_indices.len());
        let mut heights: Vec<usize> = Vec::with_capacity(self.filtered_indices.len());

        for (display_idx, &song_idx) in self.filtered_indices.iter().enumerate() {
            let song = &self.songs[song_idx];
            let selected = display_idx == self.lib_selected && focused;

            starts.push(items.len());
            let wrapped = wrap_word(&song.name, name_max);
            heights.push(wrapped.len());

            let marker = if selected { "▶ " } else { "  " };
            let text_style = if selected {
                Style::default().fg(theme::BG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::FG)
            };
            let item_style = if selected {
                Style::default().bg(theme::ACCENT)
            } else {
                Style::default()
            };

            for (i, line) in wrapped.iter().enumerate() {
                let prefix = if i == 0 {
                    format!("  {marker}")
                } else {
                    "    ".to_string()
                };
                items.push(
                    ListItem::new(Line::from(vec![
                        Span::styled(prefix, text_style),
                        Span::styled(line.clone(), text_style),
                    ]))
                    .style(item_style),
                );
            }
            items.push(ListItem::new(""));
        }

        let list = List::new(items).block(block);
        let mut state = ListState::default();
        if let (Some(&start), Some(&height)) = (
            starts.get(self.lib_selected),
            heights.get(self.lib_selected),
        ) {
            let visible = area.height.saturating_sub(2) as usize;
            let offset = if height >= visible {
                start
            } else {
                start.saturating_sub(visible.saturating_sub(height))
            };
            *state.offset_mut() = offset;
        }
        f.render_stateful_widget(list, area, &mut state);
    }

    fn render_queue_pane(&self, f: &mut ratatui::Frame, area: Rect) {
        let focused = self.focus == Focus::Playlist;
        let count = self.player.as_ref().map(|p| p.queue.len()).unwrap_or(0);
        let block = panel(format!(" Queue ({count}) "), focused);

        let Some(player) = &self.player else {
            f.render_widget(
                Paragraph::new("No audio device available")
                    .style(Style::default().fg(theme::RED))
                    .block(block),
                area,
            );
            return;
        };

        if player.queue.is_empty() {
            f.render_widget(
                Paragraph::new("Queue is empty — play a song to fill it")
                    .style(Style::default().fg(theme::MUTED))
                    .block(block),
                area,
            );
            return;
        }

        let current_idx = player.current_song_index();
        let items: Vec<ListItem> = player
            .queue
            .iter()
            .enumerate()
            .map(|(idx, path)| {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let is_current = current_idx == Some(idx);
                let is_selected = idx == self.playlist_selected && focused;

                let marker = if is_current { "▶ " } else { "  " };
                let num = format!("{:>2} ", idx + 1);
                let name = crop_word(&name, area.width.saturating_sub(8) as usize);

                let text_style = if is_selected {
                    Style::default().fg(theme::BG).add_modifier(Modifier::BOLD)
                } else if is_current {
                    Style::default()
                        .fg(theme::GREEN)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::FG)
                };
                let item_style = if is_selected {
                    Style::default().bg(theme::ACCENT)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {marker}{num}"), text_style),
                    Span::styled(name, text_style),
                ]))
                .style(item_style)
            })
            .collect();

        let list = List::new(with_gaps(items)).block(block);
        let mut state = ListState::default();
        *state.offset_mut() = spaced_offset(self.playlist_selected, area.height);
        f.render_stateful_widget(list, area, &mut state);
    }

    fn render_player_bar(&self, f: &mut ratatui::Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme::BORDER))
            .style(Style::default().bg(theme::SURFACE));
        f.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin {
            vertical: 1,
            horizontal: 2,
        });

        let Some(player) = &self.player else {
            f.render_widget(
                Paragraph::new("No audio device found — playback unavailable")
                    .style(Style::default().fg(theme::RED)),
                inner,
            );
            return;
        };

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(3),      // play button
                Constraint::Length(1),      // gap
                Constraint::Percentage(24), // title
                Constraint::Min(6),         // progress bar
                Constraint::Length(1),      // gap
                Constraint::Length(13),     // time
                Constraint::Length(1),      // gap
                Constraint::Length(3),      // shuffle button
                Constraint::Length(1),      // gap
                Constraint::Length(3),      // repeat button
            ])
            .split(inner);

        let icon = if player.is_paused() {
            "‖"
        } else if player.is_started {
            "▶"
        } else {
            "■"
        };
        f.render_widget(
            Paragraph::new(Line::from(vec![Span::styled(
                format!(" {icon} "),
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )])),
            chunks[0],
        );

        let title = player
            .current_file
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Nothing playing".to_string());
        f.render_widget(
            Paragraph::new(truncate_str(&title, chunks[2].width as usize))
                .style(Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)),
            chunks[2],
        );

        f.render_widget(
            Paragraph::new(Line::from(progress_bar(
                player.progress_ratio(),
                chunks[3].width,
            ))),
            chunks[3],
        );

        f.render_widget(
            Paragraph::new(player.progress_str())
                .alignment(Alignment::Right)
                .style(Style::default().fg(theme::MUTED)),
            chunks[5],
        );

        f.render_widget(
            Paragraph::new(Line::from(vec![toggle_button("⇆", player.shuffle)])),
            chunks[7],
        );
        f.render_widget(
            Paragraph::new(Line::from(vec![toggle_button("↻", player.repeat_one)])),
            chunks[9],
        );
    }

    fn render_bottom_status(&self, f: &mut ratatui::Frame, area: Rect) {
        let mut spans: Vec<Span> = Vec::new();

        if let Some((msg, _)) = &self.status_message {
            spans.push(Span::styled(
                format!(" {msg} "),
                Style::default().fg(theme::YELLOW),
            ));
        } else if let Ok(guard) = UPDATE_NOTICE.lock() {
            if let Some(notice) = guard.as_ref() {
                spans.push(Span::styled(
                    format!(" ★ {notice} "),
                    Style::default()
                        .fg(theme::MAGENTA)
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
                " [Sign in needed: press o to open YouTube] ",
                Style::default().fg(theme::RED).add_modifier(Modifier::BOLD),
            ));
        }

        if spans.is_empty() {
            spans.push(Span::styled(
                " Tab pane · space pause · / commands · ? help ",
                Style::default().fg(theme::MUTED),
            ));
        }

        f.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::SURFACE)),
            area,
        );
    }

    fn render_command_palette(&self, f: &mut ratatui::Frame, area: Rect) {
        let matching = matching_commands(&self.command_input);
        let count = matching.len().max(1);
        let height = ((count as u16) + 4)
            .min(12)
            .min(area.height.saturating_sub(4));
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
            .border_type(BorderType::Rounded)
            .title(" Command ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));
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
        f.render_widget(
            Paragraph::new(format!(" /{clean}"))
                .style(Style::default().fg(theme::FG).add_modifier(Modifier::BOLD)),
            chunks[0],
        );

        let sep = "─".repeat(chunks[1].width as usize);
        f.render_widget(
            Paragraph::new(sep).style(Style::default().fg(theme::BORDER)),
            chunks[1],
        );

        let items: Vec<ListItem> = if matching.is_empty() {
            vec![ListItem::new(Span::styled(
                format!("  No matching commands for \"/{clean}\""),
                Style::default().fg(theme::MUTED),
            ))]
        } else {
            matching
                .iter()
                .enumerate()
                .map(|(idx, item)| {
                    let is_selected = idx == self.command_palette_selected;
                    let marker = if is_selected { "▶ " } else { "  " };

                    let (cmd_style, alias_style, desc_style) = if is_selected {
                        (
                            Style::default().fg(theme::BG).add_modifier(Modifier::BOLD),
                            Style::default().fg(theme::BG),
                            Style::default().fg(theme::BG),
                        )
                    } else {
                        (
                            Style::default()
                                .fg(theme::ACCENT)
                                .add_modifier(Modifier::BOLD),
                            Style::default().fg(theme::MUTED),
                            Style::default().fg(theme::MUTED),
                        )
                    };
                    let item_style = if is_selected {
                        Style::default().bg(theme::ACCENT)
                    } else {
                        Style::default()
                    };

                    let alias = if item.alias.is_empty() {
                        String::new()
                    } else {
                        format!("  {}", item.alias)
                    };
                    let left_w =
                        marker.chars().count() + item.cmd.chars().count() + alias.chars().count();
                    let pad = " ".repeat(COMMAND_COL_WIDTH.saturating_sub(left_w));

                    let line = Line::from(vec![
                        Span::styled(format!("{marker}{}", item.cmd), cmd_style),
                        Span::styled(alias, alias_style),
                        Span::raw(pad),
                        Span::styled(item.desc, desc_style),
                    ]);
                    ListItem::new(line).style(item_style)
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
        let marker = if selected { "▶ " } else { "  " };
        let text = if row == SettingsRow::Dir && self.settings.editing_dir {
            format!("{marker}{label}: {}", self.settings.dir_buf)
        } else {
            format!("{marker}{label}: {value}")
        };
        let fg = if selected {
            Style::default().fg(theme::BG).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::FG)
        };
        let bg = if selected {
            Style::default().bg(theme::ACCENT)
        } else {
            Style::default()
        };
        ListItem::new(Span::styled(text, fg)).style(bg)
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
            .border_type(BorderType::Rounded)
            .title(" Settings ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));
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
            Paragraph::new(Span::styled(
                "Enter · edit / cycle      ← → · change value      Esc · close",
                Style::default().fg(theme::MUTED),
            )),
            chunks[1],
        );
    }

    fn render_help(&self, f: &mut ratatui::Frame, area: Rect) {
        let popup = centered_rect(76, 90, area);
        f.render_widget(Clear, popup);
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(" Help ")
            .title_style(
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::SURFACE));

        let lines = vec![
            Line::from(""),
            help_header("Navigation"),
            Line::from(""),
            help_row("Tab", "switch pane / field"),
            help_row("j / ↓", "move down"),
            help_row("k / ↑", "move up"),
            help_row("Enter", "play selection"),
            help_row("d", "delete song"),
            help_row("/", "command palette"),
            help_row("?", "this help"),
            Line::from(""),
            help_header("Playback"),
            Line::from(""),
            help_row("space", "play / pause"),
            help_row("n", "next track"),
            help_row("p", "previous track"),
            help_row("s", "toggle shuffle"),
            help_row("r", "toggle repeat one"),
            help_row("o", "open YouTube (sign in)"),
            Line::from(""),
            help_header("Commands"),
            Line::from(""),
            help_row("/add", "download a song"),
            help_row("/background", "play in background (/bg)"),
            help_row("/stop", "stop background audio"),
            help_row("/search", "filter the library"),
            help_row("/refresh", "reload the library"),
            help_row("/settings", "open settings"),
            help_row("/dir", "change library folder"),
            help_row("/update", "update ytcut"),
            help_row("/quit", "exit"),
            Line::from(""),
        ];

        f.render_widget(List::new(lines).block(block), popup);
    }

    fn render_prompt(&self, f: &mut ratatui::Frame, prompt: &PromptType, area: Rect) {
        let popup_area = centered_rect(60, 20, area);
        f.render_widget(Clear, popup_area);

        match prompt {
            PromptType::Delete(name) => {
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .title(" Delete ")
                    .title_style(Style::default().fg(theme::RED).add_modifier(Modifier::BOLD))
                    .border_style(Style::default().fg(theme::RED))
                    .style(Style::default().bg(theme::SURFACE));
                let text = format!("\n  Delete “{name}”?\n\n  [y] confirm    [n] cancel");
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

fn volume_bar(volume: f32) -> String {
    let v = volume.clamp(0.0, 1.0);
    let filled = (v * 10.0).round() as usize;
    let bar: String = (0..10)
        .map(|i| if i < filled { '█' } else { '░' })
        .collect();
    format!("{bar} {:>3}%", (v * 100.0).round() as i32)
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() > max_len {
        let truncated: String = s.chars().take(max_len.saturating_sub(1)).collect();
        format!("{truncated}…")
    } else {
        s.to_string()
    }
}

/// A rounded, themed panel with an accent border when focused.
fn panel(title: String, focused: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .title_style(
            Style::default()
                .fg(if focused { theme::ACCENT } else { theme::MUTED })
                .add_modifier(Modifier::BOLD),
        )
        .border_style(Style::default().fg(if focused {
            theme::ACCENT
        } else {
            theme::BORDER
        }))
        .style(Style::default().bg(theme::SURFACE))
}

/// A filled/unfilled progress bar with a knob, as styled spans.
fn progress_bar(ratio: f64, width: u16) -> Vec<Span<'static>> {
    let ratio = ratio.clamp(0.0, 1.0);
    let width = width as usize;
    if width == 0 {
        return Vec::new();
    }
    let cells = width.saturating_sub(1);
    let filled = ((ratio * cells as f64).round() as usize).min(cells);

    let mut spans: Vec<Span> = Vec::new();
    if filled > 0 {
        spans.push(Span::styled(
            "█".repeat(filled),
            Style::default().fg(theme::ACCENT),
        ));
    }
    spans.push(Span::styled("●", Style::default().fg(theme::FG)));
    let empty = cells.saturating_sub(filled);
    if empty > 0 {
        spans.push(Span::styled(
            "░".repeat(empty),
            Style::default().fg(theme::MUTED),
        ));
    }
    spans
}

/// Keep the selected row visible: offset so `selected` sits within `visible` rows.
fn scroll_offset(selected: usize, visible: usize) -> usize {
    let visible = visible.saturating_sub(1).max(1);
    selected.saturating_sub(visible)
}

/// A form-field label, accent and arrowed when focused.
fn field_label(name: &str, focused: bool) -> Span<'static> {
    let text = if focused {
        format!("▶ {name:<5}")
    } else {
        format!("  {name:<5}")
    };
    Span::styled(
        text,
        if focused {
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::MUTED)
        },
    )
}

/// A form-field value: a dim placeholder when empty, underlined while focused.
fn field_value(value: &str, placeholder: &str, focused: bool) -> Span<'static> {
    if value.is_empty() {
        Span::styled(
            placeholder.to_string(),
            Style::default()
                .fg(theme::MUTED)
                .add_modifier(Modifier::DIM),
        )
    } else if focused {
        Span::styled(
            value.to_string(),
            Style::default()
                .fg(theme::FG)
                .add_modifier(Modifier::UNDERLINED),
        )
    } else {
        Span::styled(value.to_string(), Style::default().fg(theme::FG))
    }
}

/// Wrap text into lines no wider than `max` characters, breaking at word
/// boundaries (and hard-breaking any single word longer than the limit).
fn wrap_word(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        if !current.is_empty() && current.chars().count() + 1 + word.chars().count() > max {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        for ch in word.chars() {
            if current.chars().count() >= max {
                lines.push(std::mem::take(&mut current));
            }
            current.push(ch);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Truncate to fit `max` characters, cutting at a word boundary with an ellipsis.
fn crop_word(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut truncated: String = text.chars().take(max - 1).collect();
    if let Some(space) = truncated.rfind(' ') {
        truncated.truncate(space);
    }
    truncated.push('…');
    truncated
}

/// Insert a blank line after each item so rows are taller and easier to scan.
fn with_gaps(items: Vec<ListItem<'static>>) -> Vec<ListItem<'static>> {
    let mut out = Vec::with_capacity(items.len().saturating_mul(2));
    for item in items {
        out.push(item);
        out.push(ListItem::new(""));
    }
    out
}

/// A bold, accent-colored section heading for the help page.
fn help_header(title: &str) -> Line<'static> {
    Line::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    ))
}

/// A help row: a bold accent key column followed by its description.
fn help_row(key: &str, desc: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {:<14}", key),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(desc.to_string(), Style::default().fg(theme::FG)),
    ])
}

/// A padded "button" chip for the player bar, lit green when active.
fn toggle_button(symbol: &str, on: bool) -> Span<'static> {
    let style = if on {
        Style::default()
            .fg(theme::BG)
            .bg(theme::GREEN)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::MUTED).bg(theme::SURFACE_ALT)
    };
    Span::styled(format!(" {symbol} "), style)
}

/// Scroll offset for a gap-spaced list (two buffer rows per song).
fn spaced_offset(selected: usize, area_height: u16) -> usize {
    let visible = ((area_height.saturating_sub(2)) as usize).div_ceil(2);
    scroll_offset(selected, visible) * 2
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
        assert_eq!(matching_commands("/back")[0].cmd, "/background");
        assert_eq!(matching_commands("/bg")[0].cmd, "/background");
        assert_eq!(matching_commands("/stop")[0].cmd, "/stop");
        assert_eq!(matching_commands("/search")[0].cmd, "/search");
        assert_eq!(matching_commands("/refresh")[0].cmd, "/refresh");
    }

    #[test]
    fn wrap_word_wraps_at_boundaries_and_breaks_long_words() {
        assert_eq!(wrap_word("hello world", 20), vec!["hello world"]);
        assert_eq!(wrap_word("hello world", 8), vec!["hello", "world"]);
        assert_eq!(
            wrap_word("a very long title here", 10),
            vec!["a very", "long title", "here"]
        );
        assert_eq!(
            wrap_word("supercalifragilistic", 5),
            vec!["super", "calif", "ragil", "istic"]
        );
        assert_eq!(wrap_word("", 10), vec![""]);
    }

    #[test]
    fn crop_word_cuts_at_word_boundary() {
        assert_eq!(crop_word("short", 10), "short");
        assert_eq!(crop_word("hello wonderful world", 12), "hello…");
        assert_eq!(crop_word("abcdefghij", 5), "abcd…");
        assert_eq!(crop_word("oneword", 1), "…");
    }
}
