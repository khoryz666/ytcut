//! State shared by all request threads.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::downloads::Job;
use crate::http::Request;

pub struct App {
    /// The video folder.
    pub out: PathBuf,
    pub port: String,
    /// yt-dlp's `--cookies-from-browser` value, "BROWSER:PROFILE_DIR", to sign in to YouTube.
    pub cookies: Option<String>,
    pub jobs: Mutex<Vec<Job>>,
    /// Open pages, counted by their /alive connections.
    pub pages: AtomicUsize,
}

impl App {
    pub fn new(out: PathBuf, port: String, cookies: Option<String>) -> Self {
        App {
            out,
            port,
            cookies,
            jobs: Mutex::default(),
            pages: AtomicUsize::new(0),
        }
    }

    /// A page is open or a download is running.
    pub fn busy(&self) -> bool {
        self.pages.load(Ordering::SeqCst) > 0
            || self.jobs.lock().unwrap().iter().any(|j| j.is_running())
    }

    /// Only our own page may call us: blocks other websites (CSRF) and DNS rebinding.
    pub fn trusted(&self, req: &Request) -> bool {
        let hosts = [
            format!("127.0.0.1:{}", self.port),
            format!("localhost:{}", self.port),
        ];
        req.header("host")
            .is_some_and(|h| hosts.iter().any(|ok| ok == h))
            && req
                .header("origin")
                .is_none_or(|o| hosts.iter().any(|ok| o == format!("http://{ok}")))
    }
}
