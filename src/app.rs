//! State shared by every request.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::http::{header, HeaderMap};
use tokio::sync::OnceCell;

use crate::downloads::Job;
use crate::tools::{self, Tools};

/// How long an idle server waits for a page at startup.
pub const STARTUP_GRACE: Duration = Duration::from_secs(60);
/// How long a page may stay silent. Browsers slow the timers of hidden pages to one a minute.
pub const IDLE_GRACE: Duration = Duration::from_secs(90);
/// How long an idle server waits after a page said it is closing, in case it is reloading.
pub const CLOSING_GRACE: Duration = Duration::from_secs(5);

#[derive(Clone)]
pub struct App {
    /// The video folder.
    pub out: PathBuf,
    /// The port the page is served on, used by the request guard below.
    pub port: u16,
    /// yt-dlp's `--cookies-from-browser` value, "BROWSER:PROFILE_DIR".
    pub cookies: Option<String>,
    /// Where the downloaded tools live.
    tools_dir: PathBuf,
    /// The downloaded tools, once ready.
    tools: Arc<OnceCell<Tools>>,
    pub jobs: Arc<Mutex<Vec<Job>>>,
    /// When the page last sent a heartbeat.
    last_seen: Arc<Mutex<Option<Instant>>>,
    /// A page said it is closing, and none has sent a heartbeat since.
    closing: Arc<AtomicBool>,
    started: Instant,
}

impl App {
    pub fn new(out: PathBuf, port: u16, cookies: Option<String>, tools_dir: PathBuf) -> Self {
        App {
            out,
            port,
            cookies,
            tools_dir,
            tools: Arc::default(),
            jobs: Arc::default(),
            last_seen: Arc::default(),
            closing: Arc::default(),
            started: Instant::now(),
        }
    }

    /// The tools, downloaded by the first caller. A failed download is retried by the next.
    pub async fn tools(&self) -> Result<Tools, String> {
        self.tools
            .get_or_try_init(|| async {
                let dir = self.tools_dir.clone();
                tokio::task::spawn_blocking(move || tools::prepare(&dir))
                    .await
                    .map_err(|e| e.to_string())?
                    .map_err(|e| format!("Could not set up the download tools: {e:#}"))
            })
            .await
            .cloned()
    }

    /// Record that a page is still open.
    pub fn touch(&self) {
        *self.last_seen.lock().unwrap() = Some(Instant::now());
        self.closing.store(false, Ordering::Relaxed);
    }

    /// Record that a page is closing.
    pub fn closing(&self) {
        self.closing.store(true, Ordering::Relaxed);
    }

    /// A page is open or a download is running.
    pub fn busy(&self) -> bool {
        if self.jobs.lock().unwrap().iter().any(Job::is_running) {
            return true;
        }
        match *self.last_seen.lock().unwrap() {
            Some(seen) if self.closing.load(Ordering::Relaxed) => seen.elapsed() < CLOSING_GRACE,
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn request(pairs: &[(&'static str, &'static str)]) -> bool {
        let app = App::new("/".into(), 8765, None, "/".into());
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_static(value));
        }
        app.trusted(&headers)
    }

    #[test]
    fn trusted_allows_our_page_and_blocks_other_sites() {
        assert!(request(&[("host", "127.0.0.1:8765")]));
        assert!(request(&[
            ("host", "localhost:8765"),
            ("origin", "http://localhost:8765"),
            ("sec-fetch-site", "same-origin"),
        ]));
        assert!(!request(&[]));
        assert!(!request(&[("host", "evil.example:8765")]));
        assert!(!request(&[
            ("host", "127.0.0.1:8765"),
            ("origin", "https://evil.example"),
        ]));
        assert!(!request(&[
            ("host", "127.0.0.1:8765"),
            ("sec-fetch-site", "cross-site"),
        ]));
    }
}
