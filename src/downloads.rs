//! yt-dlp: look up a video, download it in the background, and track progress.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::{Child, Command};

use crate::app::App;
use crate::library::{clean_name, MAX_NAME_BYTES};

/// Where yt-dlp keeps unfinished files, inside the video folder. Emptied at startup.
pub const TEMP_DIR: &str = ".ytcut-tmp";

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Done,
    /// The file was already there, so nothing was downloaded.
    Exists,
    Error,
}

#[derive(Clone, Serialize)]
pub struct Job {
    /// The chosen file name, or the link when the video title is used.
    pub label: String,
    pub status: Status,
    pub progress: f64,
    /// The saved file name, or the error.
    pub message: String,
}

impl Job {
    pub fn is_running(&self) -> bool {
        self.status == Status::Running
    }
}

#[derive(Serialize)]
pub struct VideoInfo {
    pub title: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
}

/// The download form: only `url` is required.
#[derive(Deserialize)]
pub struct StartForm {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub start: String,
    #[serde(default)]
    pub end: String,
}

/// Title, duration and thumbnail URL, for the download form preview.
pub async fn info(app: &App, url: &str) -> Result<VideoInfo, String> {
    check_url(url)?;
    let out = yt_dlp(app)
        .await?
        .args([
            "--skip-download",
            "--print",
            "duration",
            "--print",
            "title",
            "--print",
            "thumbnail",
            url,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let duration = lines.next().and_then(finite);
    match (out.status.success(), lines.next()) {
        (true, Some(title)) => Ok(VideoInfo {
            title: title.to_string(),
            duration,
            thumbnail: lines
                .next()
                .filter(|t| t.starts_with("https://"))
                .map(String::from),
        }),
        _ => Err(String::from_utf8_lossy(&out.stderr)
            .lines()
            .rfind(|l| l.starts_with("ERROR"))
            .map_or(
                "Could not find this video. Check the link.".into(),
                yt_error,
            )),
    }
}

/// Validate the request, then download in the background.
pub async fn start(app: &App, form: StartForm) -> Result<(), String> {
    check_url(&form.url)?;
    let name = clean_name(&form.name);
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
    if let (Some(s), Some(e)) = (seconds(&form.start)?, seconds(&form.end)?) {
        if e <= s {
            return Err("End must be after start.".into());
        }
    }

    // yt-dlp quietly skips a file that is already there; remember what was.
    let before: HashSet<String> = fs::read_dir(&app.out)
        .map(|dir| {
            dir.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    let child = download_command(app, &name, &form.start, &form.end)
        .await?
        .arg(&form.url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run yt-dlp: {e}"))?;
    track(app, child, or(&name, &form.url).to_string(), before);
    Ok(())
}

/// yt-dlp for a single video, signed in with the browser profile's cookies once it exists.
/// It runs inside the video folder so the folder's path never meets its output template,
/// which would expand any `%` or `$` in it.
async fn yt_dlp(app: &App) -> Result<Command, String> {
    let tools = app.tools().await?;
    let mut cmd = Command::new(&tools.yt_dlp);
    cmd.current_dir(&app.out)
        .stdin(Stdio::null())
        .args(["--ignore-config", "--no-playlist", "--encoding", "utf-8"])
        .arg("--ffmpeg-location")
        .arg(&tools.ffmpeg)
        .arg("--js-runtimes")
        .arg(format!("deno:{}", tools.deno.display()));
    let profile_exists = |spec: &&str| {
        spec.split_once(':')
            .is_some_and(|(_, dir)| Path::new(dir).is_dir())
    };
    if let Some(spec) = app.cookies.as_deref().filter(profile_exists) {
        cmd.args(["--cookies-from-browser", spec]);
    }
    Ok(cmd)
}

/// Download to `<folder>/<name>.mp4`, or the video title when `name` is empty.
async fn download_command(
    app: &App,
    name: &str,
    start: &str,
    end: &str,
) -> Result<Command, String> {
    let cut = !start.is_empty() || !end.is_empty();
    // `%` would be read as a yt-dlp output template field.
    let template = if !name.is_empty() {
        name.replace('%', "%%")
    } else if cut {
        // Each part gets its own file, or yt-dlp would keep the first one and skip the rest.
        let part = format!("{}-{}", or(start, "0"), or(end, "end")).replace(':', ".");
        format!("%(title).{MAX_NAME_BYTES}B ({part})")
    } else {
        // Cut long titles to fit the 255-byte file name limit.
        format!("%(title).{MAX_NAME_BYTES}B")
    };
    let mut cmd = yt_dlp(app).await?;
    cmd.args(["--newline", "--progress"])
        .args([
            "--progress-template",
            "download:PROGRESS %(progress._percent_str)s",
        ])
        .args(["--print", "after_move:SAVED %(filepath)s"])
        // Half-finished files stay out of the library.
        .args(["-P", &format!("temp:{TEMP_DIR}")])
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
        .arg(format!("{template}.%(ext)s"));
    if cut {
        let range = format!("*{}-{}", or(start, "0"), or(end, "inf"));
        cmd.args(["--download-sections", &range, "--force-keyframes-at-cuts"]);
    }
    Ok(cmd)
}

/// Add a job for the running download and update it until yt-dlp exits.
/// `before` holds the folder's file names from before the download.
fn track(app: &App, mut child: Child, label: String, before: HashSet<String>) {
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
    let jobs = app.jobs.clone();
    tokio::spawn(async move {
        let stdout = child.stdout.take().expect("stdout is piped");
        let stderr = child.stderr.take().expect("stderr is piped");
        let ((), ()) = tokio::join!(
            watch(stdout, jobs.clone(), id),
            watch(stderr, jobs.clone(), id),
        );
        let ok = child.wait().await.is_ok_and(|s| s.success());
        let mut jobs = jobs.lock().unwrap();
        let job = &mut jobs[id];
        let existed = before.contains(&job.message);
        job.status = match (ok, existed) {
            (false, _) => Status::Error,
            (true, true) => Status::Exists,
            (true, false) => Status::Done,
        };
        if ok {
            job.progress = 100.0;
        }
    });
}

/// Feed yt-dlp output lines into the job: progress, final file path, or last error.
async fn watch(stream: impl AsyncRead + Unpin, jobs: Arc<Mutex<Vec<Job>>>, id: usize) {
    let mut reader = BufReader::new(stream);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        let mut jobs = jobs.lock().unwrap();
        let Some(job) = jobs.get_mut(id) else {
            continue;
        };
        if let Some(p) = line.strip_prefix("PROGRESS ") {
            job.progress = finite(p.trim().trim_end_matches('%')).unwrap_or(job.progress);
        } else if let Some(path) = line.strip_prefix("SAVED ") {
            job.message = Path::new(path)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into();
        } else if line.starts_with("ERROR") {
            job.message = yt_error(line);
        }
    }
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
