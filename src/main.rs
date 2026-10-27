//! ytcut: local web app to download YouTube videos (optionally trimmed) and play them on repeat.
//!
//! Usage: ytcut [--exit-when-idle] [--chrome-profile=DIR] [FOLDER]
//! (PORT env var overrides the default 8765)
//!
//! With --exit-when-idle the server stops once no page is open and no download runs.
//! With --chrome-profile, yt-dlp signs in to YouTube with that Chrome profile's cookies.

mod app;
mod downloads;
mod http;
mod json;
mod library;
mod routes;

use std::fs::File;
use std::io;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{env, process, thread};

use app::App;

/// How long an idle server waits for a page: at startup, and after the last page closed.
const STARTUP_GRACE: Duration = Duration::from_secs(60);
const IDLE_GRACE: Duration = Duration::from_secs(5);

fn main() -> io::Result<()> {
    let (flags, folders): (Vec<String>, Vec<String>) =
        env::args().skip(1).partition(|a| a.starts_with("--"));
    let out = folders
        .first()
        .map_or(env::current_dir()?, PathBuf::from)
        .canonicalize()?;
    let _lock = single_instance_lock()?;
    let port = env::var("PORT").unwrap_or_else(|_| "8765".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))?;
    println!("http://127.0.0.1:{port}  ->  {}", out.display());

    let chrome_profile = flags
        .iter()
        .find_map(|f| f.strip_prefix("--chrome-profile="))
        .map(String::from);
    let app = Arc::new(App::new(out, port, chrome_profile));
    if flags.iter().any(|f| f == "--exit-when-idle") {
        let app = app.clone();
        thread::spawn(move || exit_when_idle(&app));
    }
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        thread::spawn(move || routes::handle(&stream, &app));
    }
    Ok(())
}

/// One server per user, whatever the port or folder. The OS drops the lock when we exit.
fn single_instance_lock() -> io::Result<File> {
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").map_or_else(env::temp_dir, PathBuf::from);
    let lock = File::create(runtime_dir.join("ytcut.lock"))?;
    if lock.try_lock().is_err() {
        eprintln!("ytcut is already running");
        process::exit(1);
    }
    Ok(lock)
}

fn exit_when_idle(app: &App) {
    let (mut since, mut grace) = (Instant::now(), STARTUP_GRACE);
    loop {
        thread::sleep(Duration::from_secs(1));
        if app.busy() {
            (since, grace) = (Instant::now(), IDLE_GRACE);
        } else if since.elapsed() > grace {
            process::exit(0);
        }
    }
}
