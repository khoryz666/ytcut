//! State shared by every request.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::{header, HeaderMap};

use crate::downloads::Job;

/// How long an idle server waits for a page at startup.
pub const STARTUP_GRACE: Duration = Duration::from_secs(60);
/// How long an idle server waits after the last page closed.
pub const IDLE_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct App {
    /// The video folder.
    pub out: PathBuf,
    /// The port the page is served on, used by the request guard below.
    pub port: u16,
    /// yt-dlp's `--cookies-from-browser` value, "BROWSER:PROFILE_DIR".
    pub cookies: Option<String>,
    pub jobs: Arc<Mutex<Vec<Job>>>,
    /// When the page last sent a heartbeat.
    last_seen: Arc<Mutex<Option<Instant>>>,
    started: Instant,
}

impl App {
    pub fn new(out: PathBuf, port: u16, cookies: Option<String>) -> Self {
        App {
            out,
            port,
            cookies,
            jobs: Arc::default(),
            last_seen: Arc::default(),
            started: Instant::now(),
        }
    }

    /// Record that a page is still open.
    pub fn touch(&self) {
        *self.last_seen.lock().unwrap() = Some(Instant::now());
    }

    /// A page is open or a download is running.
    pub fn busy(&self) -> bool {
        if self.jobs.lock().unwrap().iter().any(Job::is_running) {
            return true;
        }
        match *self.last_seen.lock().unwrap() {
            Some(seen) => seen.elapsed() < IDLE_GRACE,
            None => self.started.elapsed() < STARTUP_GRACE,
        }
    }

    /// Only our own page may call us: blocks other websites (CSRF, embedding our files)
    /// and DNS rebinding. Browsers mark every request with Sec-Fetch-Site; tools like
    /// curl send none.
    pub fn trusted(&self, headers: &HeaderMap) -> bool {
        let hosts = [
            format!("127.0.0.1:{}", self.port),
            format!("localhost:{}", self.port),
        ];
        let host = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .map(str::to_ascii_lowercase);
        let host_ok = host
            .as_deref()
            .is_some_and(|h| hosts.iter().any(|ok| ok == h));
        let origin_ok = headers
            .get(header::ORIGIN)
            .and_then(|o| o.to_str().ok())
            .is_none_or(|o| {
                hosts
                    .iter()
                    .any(|ok| o.eq_ignore_ascii_case(&format!("http://{ok}")))
            });
        let fetch_ok = headers
            .get("sec-fetch-site")
            .and_then(|s| s.to_str().ok())
            .is_none_or(|s| s == "same-origin" || s == "none");
        host_ok && origin_ok && fetch_ok
    }
}
