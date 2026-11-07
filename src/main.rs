//! ytcut: local web app to download YouTube videos (optionally trimmed) and play them on repeat.
//!
//! Usage: ytcut [--no-window] [--port PORT] [FOLDER]
//!
//! By default it opens its own window, keeps videos in the Music folder, and stops once no
//! page is open and no download runs. With --no-window it only serves the page until killed.

mod api;
mod app;
mod browser;
mod downloads;
mod library;
mod tools;

use std::fs::{self, File};
use std::io;
use std::path::{self, Path, PathBuf};
use std::process;
use std::time::Duration;

use anyhow::{bail, Context};
use clap::Parser;
use tokio::net::TcpListener;

use app::App;
use browser::Browser;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Folder that holds the videos. Defaults to your Music folder.
    folder: Option<PathBuf>,
    /// Only serve the page: open no window, and keep running with no page open.
    #[arg(long)]
    no_window: bool,
    /// Port to listen on.
    #[arg(long, env = "PORT", default_value_t = 8765)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let data = dirs::data_local_dir()
        .context("no data folder on this system")?
        .join("ytcut");
    fs::create_dir_all(&data).with_context(|| format!("could not create {}", data.display()))?;
    let url = format!("http://127.0.0.1:{}", args.port);
    let browser = (!args.no_window).then(|| Browser::find(&data)).flatten();

    let Some(_lock) = single_instance_lock(&data)? else {
        if args.no_window {
            eprintln!("ytcut is already running");
            process::exit(1);
        }
        browser::open(browser.as_ref(), &url);
        return Ok(());
    };

    let out = video_folder(args.folder)?;
    // We hold the lock, so no download is using this.
    let _ = fs::remove_dir_all(out.join(downloads::TEMP_DIR));
    let cookies = browser.as_ref().map(Browser::cookies);
    let app = App::new(out.clone(), args.port, cookies, data.join("tools"));
    let listener = TcpListener::bind(("127.0.0.1", args.port)).await?;
    println!("{url}  ->  {}", out.display());

    tokio::spawn(keep_current(app.clone()));
    if !args.no_window {
        browser::open(browser.as_ref(), &url);
        tokio::spawn(exit_when_idle(app.clone()));
    }
    axum::serve(listener, api::router(app)).await?;
    Ok(())
}

/// The folder asked for, which must exist, or the Music folder, which is made if needed.
fn video_folder(folder: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let Some(folder) = folder else {
        let music = dirs::audio_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join("Music")))
            .context("no Music folder on this system")?;
        fs::create_dir_all(&music)
            .with_context(|| format!("could not create {}", music.display()))?;
        return Ok(music);
    };
    if !folder.is_dir() {
        bail!("video folder not found: {}", folder.display());
    }
    Ok(path::absolute(folder)?)
}

/// One server per user, whatever the port or folder. The OS drops the lock when we exit.
/// None when another copy holds it.
fn single_instance_lock(data: &Path) -> io::Result<Option<File>> {
    let lock = File::create(data.join("ytcut.lock"))?;
    Ok(lock.try_lock().is_ok().then_some(lock))
}

/// Get the tools ready for the first request, then update them and ytcut in the background.
async fn keep_current(app: App) {
    match app.tools().await {
        Ok(tools) => tools::update_yt_dlp(&tools).await,
        Err(e) => eprintln!("{e}"),
    }
    if !cfg!(debug_assertions) {
        tools::update_self().await;
    }
}

async fn exit_when_idle(app: App) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        if !app.busy() {
            process::exit(0);
        }
    }
}
