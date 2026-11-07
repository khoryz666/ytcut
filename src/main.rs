//! ytcut: terminal music player and YouTube audio downloader.

mod config;
mod downloads;
mod library;
mod player;
mod tools;
mod ui;
mod update;

use std::fs::{self, File};
use std::io::{self, stdout, Write};
use std::path::{self, Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

use config::Config;
use downloads::StartOptions;
use player::Player;

#[derive(Parser)]
#[command(
    name = "ytcut",
    version = "0.3.0",
    about = "Terminal music player and YouTube audio downloader"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Library folder (defaults to your OS Music folder)
    #[arg(long, global = true)]
    pub out: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Download audio (.m4a)
    Add {
        /// YouTube URL
        url: String,
        /// Custom song name
        #[arg(long)]
        name: Option<String>,
        /// Split start time (e.g. 90, 1:30)
        #[arg(long)]
        start: Option<String>,
        /// Split end time (e.g. 120, 2:00)
        #[arg(long)]
        end: Option<String>,
        /// Output library directory
        #[arg(long)]
        out: Option<PathBuf>,
    },
    /// Play files, or the whole library if none
    Play {
        /// Files to play
        files: Vec<String>,
        /// Shuffle playlist
        #[arg(long, short)]
        shuffle: bool,
        /// Repeat current song
        #[arg(long, short)]
        repeat: bool,
        /// Start position in seconds (e.g. 90 or 1:30)
        #[arg(long)]
        start: Option<String>,
    },
    /// Print the library (name + size)
    List {
        /// Optional directory (defaults to library)
        dir: Option<PathBuf>,
    },
    /// Print title + duration, no download
    Info {
        /// YouTube URL
        url: String,
    },
    /// Delete a song
    Rm {
        /// Song name to delete
        name: String,
    },
    /// Rename a song
    Mv {
        /// Current song name
        name: String,
        /// New song name
        new: String,
    },
    /// Update yt-dlp + self (VERSION optional: pin/downgrade)
    Update {
        /// Target version
        version: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let data = data_dir()?;
    let config = Config::load();

    match cli.command {
        None => {
            // Launch the TUI (main interface)
            let Some(_lock) = single_instance_lock(&data)? else {
                eprintln!("ytcut is already running");
                process::exit(1);
            };

            let first_run = !Config::path().exists();
            let lib_dir = music_folder(cli.out, &config)?;
            downloads::clean_temp(&lib_dir);

            update::check_background(config.autoupdate.as_deref());

            let tools_dir = data.join("tools");
            let tools = tools::prepare(&tools_dir).ok();
            if let Some(t) = &tools {
                let t_clone = t.clone();
                std::thread::spawn(move || {
                    tools::update_yt_dlp(&t_clone);
                });
            }

            ui::run(lib_dir, tools, config, first_run)?;
        }
        Some(Commands::Add {
            url,
            name,
            start,
            end,
            out,
        }) => {
            let lib_dir = music_folder(out.or(cli.out), &config)?;
            downloads::clean_temp(&lib_dir);
            let tools = tools::prepare(&data.join("tools"))?;

            let jobs = Arc::new(Mutex::new(Vec::new()));
            let opts = StartOptions {
                url,
                name: name.unwrap_or_default(),
                start: start.unwrap_or_default(),
                end: end.unwrap_or_default(),
            };

            let browser = downloads::resolve_browser(config.browser.as_deref());
            let id = downloads::start(
                &tools,
                &lib_dir,
                Arc::clone(&jobs),
                opts,
                browser.as_deref(),
            )
            .map_err(|e| anyhow::anyhow!(e))?;

            loop {
                std::thread::sleep(Duration::from_millis(100));
                let jobs_guard = jobs.lock().unwrap();
                let job = &jobs_guard[id];
                match job.status {
                    downloads::Status::Running => {
                        print!("\rDownloading: {:.1}% ", job.progress);
                        let _ = stdout().flush();
                    }
                    downloads::Status::Done => {
                        println!("\rSaved: {}             ", job.message);
                        break;
                    }
                    downloads::Status::Exists => {
                        println!("\nAlready exists: {}", job.message);
                        break;
                    }
                    downloads::Status::Error => {
                        eprintln!("\nError: {}", job.message);
                        process::exit(1);
                    }
                }
            }
        }
        Some(Commands::Play {
            files,
            shuffle,
            repeat,
            start,
        }) => {
            let lib_dir = music_folder(cli.out, &config)?;
            let mut paths = Vec::new();

            if files.is_empty() {
                let songs = library::list(&lib_dir)?;
                if songs.is_empty() {
                    println!("Library is empty.");
                    return Ok(());
                }
                for s in songs {
                    paths.push(lib_dir.join(s.name));
                }
            } else {
                for f in files {
                    let path = Path::new(&f);
                    if path.is_file() {
                        paths.push(path.to_path_buf());
                    } else {
                        match library::find(&lib_dir, &f) {
                            Ok(found) => paths.push(found),
                            Err(e) => bail!("{e}"),
                        }
                    }
                }
            }

            let mut player = Player::new()?;
            player.set_queue(paths);
            if shuffle {
                player.toggle_shuffle();
            }
            if repeat {
                player.toggle_repeat();
            }

            player.play_index(0)?;
            if let Some(s) = start {
                if let Some(secs) = downloads::seconds(&s).map_err(|e| anyhow::anyhow!(e))? {
                    player.seek(secs)?;
                }
            }

            let is_tty = crossterm::tty::IsTty::is_tty(&std::io::stdin());
            if is_tty {
                let _ = enable_raw_mode();
            }

            println!("Playing. Press [q] to stop, [space] to pause, [n] next, [p] prev");

            loop {
                player.tick();

                if player.is_empty() && player.pos >= player.queue.len() - 1 && !player.repeat_one {
                    break;
                }

                let cur_name = player
                    .current_file
                    .as_ref()
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy())
                    .unwrap_or_default();
                let status_icon = if player.is_paused() { "⏸" } else { "▶" };
                print!(
                    "\r{} [{}] {}   ",
                    status_icon,
                    player.progress_str(),
                    cur_name
                );
                let _ = stdout().flush();

                if is_tty && event::poll(Duration::from_millis(200)).unwrap_or(false) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Esc => break,
                                KeyCode::Char('c')
                                    if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                                {
                                    break
                                }
                                KeyCode::Char(' ') => player.toggle_pause(),
                                KeyCode::Char('n') => {
                                    let _ = player.next();
                                }
                                KeyCode::Char('p') => {
                                    let _ = player.prev();
                                }
                                KeyCode::Char('s') => player.toggle_shuffle(),
                                KeyCode::Char('r') => player.toggle_repeat(),
                                _ => {}
                            }
                        }
                    }
                } else if !is_tty {
                    std::thread::sleep(Duration::from_millis(200));
                }
            }

            if is_tty {
                let _ = disable_raw_mode();
            }
            println!("\nStopped.");
        }
        Some(Commands::List { dir }) => {
            let lib_dir = match dir {
                Some(d) => d,
                None => music_folder(cli.out, &config)?,
            };
            let songs = library::list(&lib_dir)?;
            if songs.is_empty() {
                println!("No songs found in {}", lib_dir.display());
            } else {
                for s in songs {
                    println!("▶ {:<45} {:>10}", s.name, s.formatted_size());
                }
            }
        }
        Some(Commands::Info { url }) => {
            let lib_dir = music_folder(cli.out, &config)?;
            let tools = tools::prepare(&data.join("tools"))?;
            let browser = downloads::resolve_browser(config.browser.as_deref());
            let info = downloads::info(&tools, &lib_dir, &url, browser.as_deref())
                .map_err(|e| anyhow::anyhow!(e))?;
            println!("Title: {}", info.title);
            if let Some(d) = info.duration {
                println!(
                    "Duration: {}",
                    player::format_time(Duration::from_secs_f64(d))
                );
            }
            if let Some(thumb) = info.thumbnail {
                println!("Thumbnail: {}", thumb);
            }
        }
        Some(Commands::Rm { name }) => {
            let lib_dir = music_folder(cli.out, &config)?;
            library::delete(&lib_dir, &name).map_err(|e| anyhow::anyhow!(e))?;
            println!("Deleted {name}");
        }
        Some(Commands::Mv { name, new }) => {
            let lib_dir = music_folder(cli.out, &config)?;
            let new_name =
                library::rename(&lib_dir, &name, &new).map_err(|e| anyhow::anyhow!(e))?;
            println!("Renamed {name} to {new_name}");
        }
        Some(Commands::Update { version }) => {
            println!("Updating ytcut...");
            let tools_dir = data.join("tools");
            if let Ok(tools) = tools::prepare(&tools_dir) {
                print!("updating yt-dlp... ");
                let _ = stdout().flush();
                tools::update_yt_dlp(&tools);
                println!("done.");
            }
            update::manual_update(version.as_deref())?;
        }
    }

    Ok(())
}

fn data_dir() -> Result<PathBuf> {
    let data = dirs::data_local_dir()
        .context("no data folder on this system")?
        .join("ytcut");
    fs::create_dir_all(&data).with_context(|| format!("could not create {}", data.display()))?;
    Ok(data)
}

fn music_folder(folder: Option<PathBuf>, config: &Config) -> Result<PathBuf> {
    let folder = folder.or_else(|| config.library_dir.clone());
    if let Some(folder) = folder {
        fs::create_dir_all(&folder)
            .with_context(|| format!("could not create {}", folder.display()))?;
        return Ok(path::absolute(folder)?);
    }
    let music = config::default_music_dir().context("no Music folder on this system")?;
    fs::create_dir_all(&music).with_context(|| format!("could not create {}", music.display()))?;
    Ok(music)
}

fn single_instance_lock(data: &Path) -> io::Result<Option<File>> {
    let lock = File::create(data.join("ytcut.lock"))?;
    Ok(lock.try_lock().is_ok().then_some(lock))
}
