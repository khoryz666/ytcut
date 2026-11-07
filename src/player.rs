//! rodio queue: play/pause/next/prev/seek/shuffle/repeat, state.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::event::KeyCode;
use rodio::Player as AudioPlayer;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Source};

/// Outcome of handling a playback control key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlOutcome {
    /// The key performed a playback action with no status message.
    Handled,
    /// The key performed an action; show this status message.
    Message(String),
    /// The key was not a playback control.
    Unhandled,
}

/// A short `"Name: ON"` / `"Name: OFF"` label for a toggled setting.
fn toggle_label(on: bool, name: &str) -> String {
    format!("{name}: {}", if on { "ON" } else { "OFF" })
}

pub struct Player {
    _sink: MixerDeviceSink,
    pub player: AudioPlayer,
    pub queue: Vec<PathBuf>,
    pub pos: usize,
    pub shuffle: bool,
    pub repeat_one: bool,
    pub current_duration: Option<Duration>,
    pub current_file: Option<PathBuf>,
    pub is_started: bool,
    shuffled_order: Vec<usize>,
}

#[cfg(target_os = "linux")]
fn silence_alsa() {
    unsafe {
        type Handler = Option<
            unsafe extern "C" fn(
                *const std::ffi::c_char,
                std::ffi::c_int,
                *const std::ffi::c_char,
                std::ffi::c_int,
                *const std::ffi::c_char,
            ),
        >;
        extern "C" {
            fn snd_lib_error_set_handler(handler: Handler) -> std::ffi::c_int;
        }
        unsafe extern "C" fn noop(
            _file: *const std::ffi::c_char,
            _line: std::ffi::c_int,
            _function: *const std::ffi::c_char,
            _err: std::ffi::c_int,
            _fmt: *const std::ffi::c_char,
        ) {
        }
        let _ = snd_lib_error_set_handler(Some(noop));
    }
}

#[cfg(not(target_os = "linux"))]
fn silence_alsa() {}

/// Open an output device, preferring the PipeWire/PulseAudio server (which routes to the
/// user's default sink — e.g. a Bluetooth speaker) and falling back to analog speakers.
/// Open the system's default output (PulseAudio/PipeWire), which follows the user's
/// chosen sink — a Bluetooth earphone, the speakers, whatever the desktop uses.
fn open_sink() -> Result<MixerDeviceSink> {
    DeviceSinkBuilder::from_default_device()
        .and_then(|b| b.with_error_callback(|_| {}).open_sink_or_fallback())
        .context("no usable output device")
}

impl Player {
    pub fn new() -> Result<Self> {
        silence_alsa();
        let sink = open_sink().context("failed to open audio output device")?;
        let player = AudioPlayer::connect_new(sink.mixer());
        Ok(Self {
            _sink: sink,
            player,
            queue: Vec::new(),
            pos: 0,
            shuffle: false,
            repeat_one: false,
            current_duration: None,
            current_file: None,
            is_started: false,
            shuffled_order: Vec::new(),
        })
    }

    /// Try creating a player, or return None if no audio device is available.
    pub fn try_new() -> Option<Self> {
        Self::new().ok()
    }

    pub fn set_queue(&mut self, queue: Vec<PathBuf>) {
        self.queue = queue;
        self.rebuild_order();
        self.pos = 0;
    }

    pub fn add_to_queue(&mut self, path: PathBuf) {
        self.queue.push(path);
        self.rebuild_order();
    }

    pub fn remove_from_queue(&mut self, index: usize) {
        if index < self.queue.len() {
            self.queue.remove(index);
            self.rebuild_order();
            if self.pos >= self.queue.len() && !self.queue.is_empty() {
                self.pos = self.queue.len() - 1;
            }
        }
    }

    fn rebuild_order(&mut self) {
        self.shuffled_order = (0..self.queue.len()).collect();
        if self.shuffle {
            // Simple deterministic/random shuffle without extra rand crate dependency
            let len = self.shuffled_order.len();
            if len > 1 {
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(42);
                let mut state = (seed ^ 0x5DEECE66D) as u64;
                for i in (1..len).rev() {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    let j = (state as usize) % (i + 1);
                    self.shuffled_order.swap(i, j);
                }
            }
        }
    }

    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.rebuild_order();
    }

    pub fn toggle_repeat(&mut self) {
        self.repeat_one = !self.repeat_one;
    }

    /// Handle a playback control key (`space` / `n` / `p` / `s` / `r`).
    ///
    /// Shared by the TUI and the CLI `play` subcommand so key bindings live in
    /// one place. Returns an optional status message for the caller to show.
    pub fn handle_control_key(&mut self, code: KeyCode) -> ControlOutcome {
        match code {
            KeyCode::Char(' ') => {
                self.toggle_pause();
                ControlOutcome::Handled
            }
            KeyCode::Char('n') => match self.next() {
                Ok(()) => ControlOutcome::Handled,
                Err(e) => ControlOutcome::Message(format!("Error: {e:#}")),
            },
            KeyCode::Char('p') => match self.prev() {
                Ok(()) => ControlOutcome::Handled,
                Err(e) => ControlOutcome::Message(format!("Error: {e:#}")),
            },
            KeyCode::Char('s') => {
                self.toggle_shuffle();
                ControlOutcome::Message(toggle_label(self.shuffle, "Shuffle"))
            }
            KeyCode::Char('r') => {
                self.toggle_repeat();
                ControlOutcome::Message(toggle_label(self.repeat_one, "Repeat-one"))
            }
            _ => ControlOutcome::Unhandled,
        }
    }

    pub fn current_song_index(&self) -> Option<usize> {
        if self.queue.is_empty() {
            None
        } else if self.shuffle {
            self.shuffled_order.get(self.pos).copied()
        } else {
            Some(self.pos)
        }
    }

    pub fn play_index(&mut self, index: usize) -> Result<()> {
        if index >= self.queue.len() {
            return Ok(());
        }
        if self.shuffle {
            if let Some(pos) = self.shuffled_order.iter().position(|&i| i == index) {
                self.pos = pos;
            } else {
                self.pos = index;
            }
        } else {
            self.pos = index;
        }
        self.play_current()
    }

    pub fn play_current(&mut self) -> Result<()> {
        let Some(idx) = self.current_song_index() else {
            return Ok(());
        };
        let Some(path) = self.queue.get(idx).cloned() else {
            return Ok(());
        };
        self.play_file(&path)
    }

    pub fn play_file(&mut self, path: &Path) -> Result<()> {
        self.player.clear();
        let file =
            File::open(path).with_context(|| format!("could not open {}", path.display()))?;
        let decoder = Decoder::try_from(file)
            .with_context(|| format!("could not decode {}", path.display()))?;
        self.current_duration = decoder.total_duration();
        self.player.append(decoder);
        self.player.play();
        self.current_file = Some(path.to_path_buf());
        self.is_started = true;
        Ok(())
    }

    pub fn toggle_pause(&self) {
        if self.player.is_paused() {
            self.player.play();
        } else {
            self.player.pause();
        }
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume.clamp(0.0, 1.0));
    }

    pub fn is_paused(&self) -> bool {
        self.player.is_paused()
    }

    pub fn is_empty(&self) -> bool {
        self.player.empty()
    }

    pub fn position(&self) -> Duration {
        self.player.get_pos()
    }

    pub fn seek(&self, seconds: f64) -> Result<()> {
        self.player
            .try_seek(Duration::from_secs_f64(seconds.max(0.0)))
            .map_err(|e| anyhow::anyhow!("seek error: {e}"))
    }

    pub fn next(&mut self) -> Result<()> {
        if self.queue.is_empty() {
            return Ok(());
        }
        self.pos = (self.pos + 1) % self.queue.len();
        self.play_current()
    }

    pub fn prev(&mut self) -> Result<()> {
        if self.queue.is_empty() {
            return Ok(());
        }
        if self.position() > Duration::from_secs(3) {
            self.seek(0.0)?;
            self.player.play();
            return Ok(());
        }
        if self.pos == 0 {
            self.pos = self.queue.len() - 1;
        } else {
            self.pos -= 1;
        }
        self.play_current()
    }

    /// Periodic poll to check if the current song finished.
    pub fn tick(&mut self) {
        if self.is_started && !self.is_paused() && self.is_empty() && !self.queue.is_empty() {
            if self.repeat_one {
                let _ = self.play_current();
            } else {
                let _ = self.next();
            }
        }
    }

    pub fn progress_ratio(&self) -> f64 {
        match self.current_duration {
            Some(total) if total.as_secs_f64() > 0.0 => {
                (self.position().as_secs_f64() / total.as_secs_f64()).clamp(0.0, 1.0)
            }
            _ => 0.0,
        }
    }

    pub fn progress_str(&self) -> String {
        let pos = format_time(self.position());
        match self.current_duration {
            Some(dur) => format!("{pos}/{}", format_time(dur)),
            None => pos,
        }
    }
}

pub fn format_time(dur: Duration) -> String {
    let total_secs = dur.as_secs();
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}
