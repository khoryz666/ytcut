//! ytcut: local web app to download YouTube videos (optionally trimmed) and play them on repeat.
//!
//! Usage: ytcut [--exit-when-idle] [--cookies-from-browser=BROWSER:PROFILE_DIR] [FOLDER]
//! (PORT env var overrides the default 8765)
//!
//! With --exit-when-idle the server stops once no page is open and no download runs.
//! With --cookies-from-browser, yt-dlp signs in to YouTube with that browser profile's
//! cookies, e.g. `chrome:~/.local/share/ytcut/chrome/Default`.

mod api;
mod app;
mod downloads;
mod library;

use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::process;
use std::time::Duration;

use anyhow::Context;
use clap::Parser;
use tokio::net::TcpListener;

use app::App;

#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Folder that holds the videos. Defaults to the current folder.
    folder: Option<PathBuf>,
    /// Stop once no page is open and no download runs.
    #[arg(long)]
    exit_when_idle: bool,
    /// yt-dlp `--cookies-from-browser` value, e.g. chrome:PROFILE_DIR.
    #[arg(long)]
    cookies_from_browser: Option<String>,
    /// Port to listen on.
    #[arg(long, env = "PORT", default_value_t = 8765)]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let out = args
        .folder
        .unwrap_or(std::env::current_dir()?)
        .canonicalize()
        .context("video folder not found")?;
    let _lock = single_instance_lock()?;
    let app = App::new(out.clone(), args.port, args.cookies_from_browser);
    let listener = TcpListener::bind(("127.0.0.1", args.port)).await?;
    println!("http://127.0.0.1:{}  ->  {}", args.port, out.display());

    if args.exit_when_idle {
        let app = app.clone();
        tokio::spawn(async move { exit_when_idle(app).await });
    }
    axum::serve(listener, api::router(app)).await?;
    Ok(())
}

/// One server per user, whatever the port or folder. The OS drops the lock when we exit.
fn single_instance_lock() -> io::Result<File> {
    let runtime_dir =
        std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let lock = File::create(runtime_dir.join("ytcut.lock"))?;
    if lock.try_lock().is_err() {
        eprintln!("ytcut is already running");
        process::exit(1);
    }
    Ok(lock)
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
