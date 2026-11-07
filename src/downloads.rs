//! yt-dlp: look up a song, download its audio in the background, and track progress.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::library::{clean_name, MAX_NAME_BYTES};
use crate::tools::Tools;

/// Where yt-dlp keeps unfinished files, inside the music folder. Emptied at startup.
pub const TEMP_DIR: &str = ".ytcut-tmp";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    Done,
    /// The file was already there, so nothing was downloaded.
    Exists,
    Error,
}

#[derive(Clone, Debug)]
pub struct Job {
    /// The chosen file name, or the link when the song title is used.
    pub label: String,
    pub status: Status,
    pub progress: f64,
    /// The saved file name, or the error message.
    pub message: String,
}

impl Job {
    pub fn is_running(&self) -> bool {
        self.status == Status::Running
    }
}

pub struct SongInfo {
    pub title: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
}

pub struct StartOptions {
    pub url: String,
    pub name: String,
    pub start: String,
    pub end: String,
}

/// Remove unfinished downloads from `.ytcut-tmp`.
pub fn clean_temp(out: &Path) {
    let _ = fs::remove_dir_all(out.join(TEMP_DIR));
}

/// Title, duration and thumbnail URL, for preview.
pub fn info(
    tools: &Tools,
    out: &Path,
    url: &str,
    browser: Option<&str>,
) -> Result<SongInfo, String> {
    check_url(url)?;
    let mut cmd = base_yt_dlp(tools, out, browser);
    cmd.args([
        "--skip-download",
        "--print",
        "duration",
        "--print",
        "title",
        "--print",
        "thumbnail",
        url,
    ]);
    let out = cmd.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    let duration = lines.next().and_then(finite);
    match (out.status.success(), lines.next()) {
        (true, Some(title)) => Ok(SongInfo {
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
            .map_or("Could not find this song. Check the link.".into(), yt_error)),
    }
}

/// Validate the request, then download in the background.
pub fn start(
    tools: &Tools,
    out: &Path,
    jobs: Arc<Mutex<Vec<Job>>>,
    opts: StartOptions,
    browser: Option<&str>,
) -> Result<usize, String> {
    check_url(&opts.url)?;
    let name = clean_name(&opts.name);
    let taken = out.join(format!("{name}.m4a")).exists()
        || jobs
            .lock()
            .unwrap()
            .iter()
            .any(|j| j.is_running() && j.label == name);
    if !name.is_empty() && taken {
        return Err(format!("{name}.m4a already exists. Pick another name."));
    }
    if let (Some(s), Some(e)) = (seconds(&opts.start)?, seconds(&opts.end)?) {
        if e <= s {
            return Err("End must be after start.".into());
        }
    }

    let before: HashSet<String> = fs::read_dir(out)
        .map(|dir| {
            dir.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();

    let mut cmd = download_command(tools, out, &name, &opts.start, &opts.end, browser);
    cmd.arg(&opts.url);
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Could not run yt-dlp: {e}"))?;

    let label = if !name.is_empty() {
        name
    } else {
        opts.url.clone()
    };

    let id = {
        let mut list = jobs.lock().unwrap();
        list.push(Job {
            label,
            status: Status::Running,
            progress: 0.0,
            message: String::new(),
        });
        list.len() - 1
    };

    let stdout = child.stdout.take().expect("stdout is piped");
    let stderr = child.stderr.take().expect("stderr is piped");
    let jobs_clone = Arc::clone(&jobs);
    let out_dir = out.to_path_buf();

    thread::spawn(move || {
        let stdout_reader = BufReader::new(stdout);
        let stderr_reader = BufReader::new(stderr);

        let jobs_out = Arc::clone(&jobs_clone);
        let out_handle = thread::spawn(move || {
            for line in stdout_reader.lines().map_while(Result::ok) {
                let line = line.trim();
                let mut jobs = jobs_out.lock().unwrap();
                if let Some(job) = jobs.get_mut(id) {
                    if let Some(p) = line.strip_prefix("download:PROGRESS ") {
                        let pct_str = p.trim().trim_end_matches('%');
                        if let Some(pct) = finite(pct_str) {
                            job.progress = pct;
                        }
                    } else if let Some(path) = line.strip_prefix("after_move:SAVED ") {
                        job.message = Path::new(path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into();
                    }
                }
            }
        });

        let jobs_err = Arc::clone(&jobs_clone);
        let err_handle = thread::spawn(move || {
            for line in stderr_reader.lines().map_while(Result::ok) {
                let line = line.trim();
                let mut jobs = jobs_err.lock().unwrap();
                if let Some(job) = jobs.get_mut(id) {
                    if line.starts_with("ERROR") {
                        job.message = yt_error(line);
                    }
                }
            }
        });

        let _ = out_handle.join();
        let _ = err_handle.join();

        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        let mut jobs = jobs_clone.lock().unwrap();
        if let Some(job) = jobs.get_mut(id) {
            if ok && job.message.is_empty() {
                if let Ok(dir) = fs::read_dir(&out_dir) {
                    for entry in dir.flatten() {
                        if let Ok(fname) = entry.file_name().into_string() {
                            if !before.contains(&fname) && !fname.starts_with('.') {
                                job.message = fname;
                                break;
                            }
                        }
                    }
                }
            }
            let existed = !job.message.is_empty() && before.contains(&job.message);
            job.status = match (ok, existed) {
                (false, _) => Status::Error,
                (true, true) => Status::Exists,
                (true, false) => Status::Done,
            };
            if ok {
                job.progress = 100.0;
            } else if job.message.is_empty() {
                job.message = "Download failed.".into();
            }
        }
    });

    Ok(id)
}

/// yt-dlp base command configured for library directory and cookies.
fn base_yt_dlp(tools: &Tools, out: &Path, browser: Option<&str>) -> Command {
    let mut cmd = Command::new(&tools.yt_dlp);
    cmd.current_dir(out)
        .stdin(Stdio::null())
        .args(["--ignore-config", "--no-playlist", "--encoding", "utf-8"]);

    if tools.ffmpeg.is_file() {
        cmd.arg("--ffmpeg-location").arg(&tools.ffmpeg);
    }
    if tools.deno.is_file() {
        cmd.arg("--js-runtimes")
            .arg(format!("deno:{}", tools.deno.display()));
    }

    if let Some(browser) = browser {
        cmd.args(["--cookies-from-browser", browser]);
    }
    cmd
}

/// Download `<folder>/<name>.m4a` (audio only), or the song title when `name` is empty.
fn download_command(
    tools: &Tools,
    out: &Path,
    name: &str,
    start: &str,
    end: &str,
    browser: Option<&str>,
) -> Command {
    let cut = !start.is_empty() || !end.is_empty();
    let template = if !name.is_empty() {
        let base = name.replace('%', "%%");
        if cut {
            let part =
                format!("{}-{}", or_default(start, "0"), or_default(end, "end")).replace(':', ".");
            format!("{base} ({part})")
        } else {
            base
        }
    } else if cut {
        let part =
            format!("{}-{}", or_default(start, "0"), or_default(end, "end")).replace(':', ".");
        format!("%(title).{MAX_NAME_BYTES}B ({part})")
    } else {
        format!("%(title).{MAX_NAME_BYTES}B")
    };

    let mut cmd = base_yt_dlp(tools, out, browser);
    cmd.args(["--newline", "--progress"])
        .args([
            "--progress-template",
            "download:PROGRESS %(progress._percent_str)s",
        ])
        .args(["--print", "after_move:SAVED %(filepath)s"])
        .args(["-P", &format!("temp:{TEMP_DIR}")])
        .args([
            "-f",
            "bestaudio/best",
            "-x",
            "--audio-format",
            "m4a",
            "--audio-quality",
            "0",
        ])
        .arg("-o")
        .arg(format!("{template}.%(ext)s"));

    if cut {
        let range = format!("*{}-{}", or_default(start, "0"), or_default(end, "inf"));
        cmd.args(["--download-sections", &range]);
    }
    cmd
}

/// Resolve the config `browser` value to a yt-dlp browser name, or None.
/// "none" disables cookies; "auto" (or unset) auto-detects an installed browser.
pub fn resolve_browser(config_browser: Option<&str>) -> Option<String> {
    match config_browser {
        Some("none") => None,
        Some(name) if name != "auto" => Some(name.to_string()),
        _ => detect_browser(),
    }
}

/// Detect an installed Chromium browser name for `--cookies-from-browser <name>`.
pub fn detect_browser() -> Option<String> {
    if let Ok(chosen) = env::var("YTCUT_BROWSER") {
        if !chosen.trim().is_empty() {
            let name = chosen.to_ascii_lowercase();
            return Some(browser_kind(&name).to_string());
        }
    }
    for candidate in browser_candidates() {
        if locate_browser(&candidate).is_some() {
            let stem = candidate
                .file_stem()?
                .to_string_lossy()
                .to_ascii_lowercase();
            return Some(browser_kind(&stem).to_string());
        }
    }
    None
}

fn browser_kind(name: &str) -> &'static str {
    [
        ("chromium", "chromium"),
        ("brave", "brave"),
        ("edge", "edge"),
        ("vivaldi", "vivaldi"),
    ]
    .into_iter()
    .find(|(part, _)| name.contains(part))
    .map_or("chrome", |(_, kind)| kind)
}

fn locate_browser(program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return program.is_file().then(|| program.to_path_buf());
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program).with_extension(env::consts::EXE_EXTENSION))
        .find(|file| file.is_file())
}

#[cfg(not(windows))]
fn browser_candidates() -> Vec<PathBuf> {
    [
        "google-chrome-stable",
        "google-chrome",
        "chromium",
        "chromium-browser",
        "brave-browser",
        "brave",
        "microsoft-edge-stable",
        "microsoft-edge",
        "vivaldi-stable",
        "vivaldi",
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect()
}

#[cfg(windows)]
fn browser_candidates() -> Vec<PathBuf> {
    let programs = [
        r"Google\Chrome\Application\chrome.exe",
        r"Microsoft\Edge\Application\msedge.exe",
        r"BraveSoftware\Brave-Browser\Application\brave.exe",
        r"Vivaldi\Application\vivaldi.exe",
    ];
    let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .into_iter()
        .filter_map(env::var_os)
        .map(PathBuf::from)
        .collect();
    programs
        .iter()
        .flat_map(|program| roots.iter().map(move |root| root.join(program)))
        .collect()
}

/// "ERROR: [youtube] id: Sign in to confirm. Use --cookies ..." -> "Sign in to confirm."
pub fn yt_error(line: &str) -> String {
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

pub fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err("Enter a valid link.".into())
    }
}

/// Parse "", "90", "1:30" or "1:02:03.5" into seconds.
pub fn seconds(t: &str) -> Result<Option<f64>, String> {
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

pub fn finite(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn or_default<'a>(s: &'a str, default: &'a str) -> &'a str {
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
