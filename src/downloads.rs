//! yt-dlp: look up a video, download it in the background, and track progress.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::thread;

use crate::app::App;
use crate::json;
use crate::library::{clean_name, MAX_NAME_BYTES};

#[derive(Clone, Copy, PartialEq)]
pub enum Status {
    Running,
    Done,
    Error,
}

pub struct Job {
    /// The chosen file name, or the link when the video title is used.
    label: String,
    status: Status,
    progress: f64,
    /// The saved file name, or the error.
    message: String,
}

impl Job {
    pub fn is_running(&self) -> bool {
        self.status == Status::Running
    }
}

/// Title and duration as JSON, for the download form preview.
pub fn info(app: &App, url: &str) -> Result<String, String> {
    check_url(url)?;
    let out = yt_dlp(app)
        .args([
            "--skip-download",
            "--print",
            "duration",
            "--print",
            "title",
            url,
        ])
        .output()
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    match (out.status.success(), lines.next(), lines.next()) {
        (true, Some(duration), Some(title)) => Ok(json::object(&[
            ("title", json::string(title)),
            (
                "duration",
                finite(duration).map_or("null".into(), |d| d.to_string()),
            ),
        ])),
        _ => Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .rfind(|l| l.starts_with("ERROR"))
            .map_or(
                "Could not find this video. Check the link.".into(),
                yt_error,
            )),
    }
}

/// Validate the request, then download in the background. `start` and `end` may be empty.
pub fn start(app: &Arc<App>, url: &str, name: &str, start: &str, end: &str) -> Result<(), String> {
    check_url(url)?;
    let name = clean_name(name);
    let taken = app.out.join(format!("{name}.mp4")).exists()
        || app
            .jobs
            .lock()
            .unwrap()
            .iter()
            .any(|j| j.is_running() && j.label == name);
    if !name.is_empty() && taken {
        return Err(format!("{name}.mp4 already exists. Pick another name."));
    }
    if let (Some(s), Some(e)) = (seconds(start)?, seconds(end)?) {
        if e <= s {
            return Err("End must be after start.".into());
        }
    }

    let child = download_command(app, &name, start, end)
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run yt-dlp: {e}"))?;
    track(app, child, or(&name, url).to_string());
    Ok(())
}

pub fn jobs_json(jobs: &[Job]) -> String {
    json::array(jobs.iter().rev().map(|j| {
        let status = match j.status {
            Status::Running => "running",
            Status::Done => "done",
            Status::Error => "error",
        };
        json::object(&[
            ("label", json::string(&j.label)),
            ("status", json::string(status)),
            ("progress", j.progress.to_string()),
            ("message", json::string(&j.message)),
        ])
    }))
}

/// yt-dlp for a single video, signed in with the Chrome profile's cookies if there are any.
fn yt_dlp(app: &App) -> Command {
    let mut cmd = Command::new("yt-dlp");
    cmd.arg("--no-playlist");
    if let Some(dir) = app
        .chrome_profile
        .as_deref()
        .filter(|d| Path::new(d).is_dir())
    {
        cmd.args(["--cookies-from-browser", &format!("chrome:{dir}")]);
    }
    cmd
}

/// Download to `<folder>/<name>.mp4`, or the video title when `name` is empty.
fn download_command(app: &App, name: &str, start: &str, end: &str) -> Command {
    // `%` would be read as a yt-dlp output template field.
    let template = if name.is_empty() {
        // Cut long titles to fit the 255-byte file name limit.
        format!("%(title).{MAX_NAME_BYTES}B")
    } else {
        name.replace('%', "%%")
    };
    let mut cmd = yt_dlp(app);
    cmd.args(["--newline", "--progress"])
        .args([
            "--progress-template",
            "download:PROGRESS %(progress._percent_str)s",
        ])
        .args(["--print", "after_move:filepath"])
        // Prefer H.264 + AAC so the MP4 plays everywhere.
        .args([
            "-f",
            "bv*+ba/b",
            "-S",
            "vcodec:h264,res,acodec:aac",
            "--merge-output-format",
            "mp4",
        ])
        .arg("-o")
        .arg(app.out.join(format!("{template}.%(ext)s")));
    if !start.is_empty() || !end.is_empty() {
        let range = format!("*{}-{}", or(start, "0"), or(end, "inf"));
        cmd.args(["--download-sections", &range, "--force-keyframes-at-cuts"]);
    }
    cmd
}

/// Add a job for the running download and update it until yt-dlp exits.
fn track(app: &Arc<App>, mut child: Child, label: String) {
    let id = {
        let mut jobs = app.jobs.lock().unwrap();
        jobs.push(Job {
            label,
            status: Status::Running,
            progress: 0.0,
            message: String::new(),
        });
        jobs.len() - 1
    };
    let readers = [
        watch(child.stdout.take().unwrap(), app.clone(), id),
        watch(child.stderr.take().unwrap(), app.clone(), id),
    ];
    let app = app.clone();
    thread::spawn(move || {
        readers.into_iter().for_each(|r| drop(r.join()));
        let ok = child.wait().is_ok_and(|s| s.success());
        let job = &mut app.jobs.lock().unwrap()[id];
        job.status = if ok { Status::Done } else { Status::Error };
        if ok {
            job.progress = 100.0;
        }
    });
}

/// Feed yt-dlp output lines into the job: progress, final file path, or last error.
fn watch(stream: impl Read + Send + 'static, app: Arc<App>, id: usize) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        // Read bytes, not lines: a non-UTF-8 line must not stop us draining the pipe.
        for line in BufReader::new(stream).split(b'\n').map_while(Result::ok) {
            let line = String::from_utf8_lossy(&line).into_owned();
            let job = &mut app.jobs.lock().unwrap()[id];
            if let Some(p) = line.strip_prefix("PROGRESS ") {
                job.progress = finite(p.trim().trim_end_matches('%')).unwrap_or(job.progress);
            } else if line.starts_with('/') {
                job.message = Path::new(&line)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
            } else if line.starts_with("ERROR") {
                job.message = yt_error(&line);
            }
        }
    })
}

/// "ERROR: [youtube] id: Sign in to confirm. Use --cookies ..." -> "Sign in to confirm."
fn yt_error(line: &str) -> String {
    let msg = line.trim_start_matches("ERROR:").trim();
    let msg = match msg.strip_prefix('[') {
        Some(rest) => rest.split_once(": ").map_or(msg, |(_, m)| m),
        None => msg,
    };
    msg.split(" Use --cookies")
        .next()
        .unwrap_or(msg)
        .trim()
        .to_string()
}

fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err("Enter a valid link.".into())
    }
}

/// Parse "", "90", "1:30" or "1:02:03.5" into seconds.
fn seconds(t: &str) -> Result<Option<f64>, String> {
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("Bad time '{t}'. Use seconds or minutes like 1:30.");
    let allowed = |c: char| c.is_ascii_digit() || c == '.' || c == ':';
    if t.split(':').count() > 3 || !t.chars().all(allowed) {
        return Err(bad());
    }
    t.split(':')
        .try_fold(0.0, |acc, part| part.parse::<f64>().map(|v| acc * 60.0 + v))
        .map(Some)
        .map_err(|_| bad())
}

fn finite(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.is_empty() {
        default
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_parses_clock_times() {
        assert_eq!(seconds(""), Ok(None));
        assert_eq!(seconds("90"), Ok(Some(90.0)));
        assert_eq!(seconds("1:30"), Ok(Some(90.0)));
        assert_eq!(seconds("1:02:03.5"), Ok(Some(3723.5)));
        for bad in ["-1", "inf", "NaN", "1:2:3:4", "1::2"] {
            assert!(seconds(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn finite_rejects_nan_and_infinity() {
        assert_eq!(finite("12.5"), Some(12.5));
        assert_eq!(finite("NaN"), None);
        assert_eq!(finite("inf"), None);
        assert_eq!(finite("N/A"), None);
    }

    #[test]
    fn yt_error_keeps_the_reason_only() {
        assert_eq!(
            yt_error("ERROR: [youtube] abc: Sign in to confirm you're not a bot. Use --cookies-from-browser or --cookies"),
            "Sign in to confirm you're not a bot."
        );
        assert_eq!(yt_error("ERROR: Unable to download"), "Unable to download");
    }
}
