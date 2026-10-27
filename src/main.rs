//! ytcut: local web app to download YouTube videos (optionally trimmed) and play them on repeat.
//! Usage: ytcut [--exit-when-idle] [--chrome-profile=DIR] [FOLDER]
//! (PORT env var overrides the default 8765)
//!
//! With --exit-when-idle the server stops once no page is open and no download runs.
//! With --chrome-profile, yt-dlp signs in to YouTube with that Chrome profile's cookies.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};
use std::{env, thread};

const PAGE: &str = include_str!("index.html");
const JSON: &str = "application/json";
const TEXT: &str = "text/plain; charset=utf-8";
/// How long an idle server waits for a page: at startup, and after the last page closed.
const STARTUP_GRACE: Duration = Duration::from_secs(60);
const IDLE_GRACE: Duration = Duration::from_secs(5);
/// Leaves room for an extension and yt-dlp's temporary suffixes within 255 bytes.
const MAX_NAME_BYTES: usize = 200;
const VIDEO_TYPES: [(&str, &str); 5] = [
    ("mp4", "video/mp4"),
    ("m4v", "video/mp4"),
    ("webm", "video/webm"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
];

struct Job {
    label: String,
    status: &'static str,
    progress: f64,
    message: String,
}

struct App {
    out: PathBuf,
    port: String,
    /// Chrome profile directory to read YouTube cookies from.
    chrome_profile: Option<String>,
    jobs: Mutex<Vec<Job>>,
    /// Open pages, counted by their /alive connections.
    pages: AtomicUsize,
}

impl App {
    fn busy(&self) -> bool {
        self.pages.load(Ordering::SeqCst) > 0
            || self
                .jobs
                .lock()
                .unwrap()
                .iter()
                .any(|j| j.status == "running")
    }

    /// Only our own page may call us: blocks other websites (CSRF) and DNS rebinding.
    fn trusted(&self, req: &Request) -> bool {
        let hosts = [
            format!("127.0.0.1:{}", self.port),
            format!("localhost:{}", self.port),
        ];
        let header = |k| req.headers.get(k).map(String::as_str);
        header("host").is_some_and(|h| hosts.iter().any(|ok| ok == h))
            && header("origin").is_none_or(|o| hosts.iter().any(|ok| o == format!("http://{ok}")))
    }
}

struct Request {
    method: String,
    path: String,
    /// Header names in lower case.
    headers: HashMap<String, String>,
    /// Query string and form body fields together.
    form: HashMap<String, String>,
}

type Reply = (u16, &'static str, String);

fn main() -> io::Result<()> {
    let (flags, folders): (Vec<String>, Vec<String>) =
        env::args().skip(1).partition(|a| a.starts_with("--"));
    let out = folders
        .first()
        .map_or(env::current_dir()?, PathBuf::from)
        .canonicalize()?;
    // One server per user, whatever the port or folder. The OS drops the lock when we exit.
    let runtime_dir = env::var_os("XDG_RUNTIME_DIR").map_or_else(env::temp_dir, PathBuf::from);
    let lock = File::create(runtime_dir.join("ytcut.lock"))?;
    if lock.try_lock().is_err() {
        eprintln!("ytcut is already running");
        std::process::exit(1);
    }
    let port = env::var("PORT").unwrap_or_else(|_| "8765".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))?;
    println!("http://127.0.0.1:{port}  ->  {}", out.display());

    let app = Arc::new(App {
        out,
        port,
        chrome_profile: flags
            .iter()
            .find_map(|f| f.strip_prefix("--chrome-profile="))
            .map(String::from),
        jobs: Mutex::default(),
        pages: AtomicUsize::new(0),
    });
    if flags.iter().any(|f| f == "--exit-when-idle") {
        let app = app.clone();
        thread::spawn(move || exit_when_idle(&app));
    }
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        thread::spawn(move || handle(&stream, &app));
    }
    Ok(())
}

fn exit_when_idle(app: &App) {
    let (mut since, mut grace) = (Instant::now(), STARTUP_GRACE);
    loop {
        thread::sleep(Duration::from_secs(1));
        if app.busy() {
            (since, grace) = (Instant::now(), IDLE_GRACE);
        } else if since.elapsed() > grace {
            std::process::exit(0);
        }
    }
}

fn handle(stream: &TcpStream, app: &Arc<App>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    let req = read_request(stream)?;
    if !app.trusted(&req) {
        return reply(stream, (403, TEXT, "forbidden".into()));
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/alive") => keep_alive(stream, app),
        ("GET", path) if path.starts_with("/media/") => {
            let range = req.headers.get("range").map(String::as_str);
            serve_media(stream, &app.out, &decode(&path["/media/".len()..]), range)
        }
        _ => reply(stream, route(&req, app).unwrap_or_else(|e| (400, TEXT, e))),
    }
}

fn reply(stream: &TcpStream, (status, ctype, body): Reply) -> io::Result<()> {
    write_head(stream, status, ctype, Some(body.len() as u64), "")?;
    let mut w = stream;
    w.write_all(body.as_bytes())
}

/// Hold an event stream open while the page is open; a failed write means it closed.
fn keep_alive(stream: &TcpStream, app: &App) -> io::Result<()> {
    write_head(
        stream,
        200,
        "text/event-stream",
        None,
        "Cache-Control: no-store\r\n",
    )?;
    app.pages.fetch_add(1, Ordering::SeqCst);
    let mut w = stream;
    let closed = loop {
        if let Err(e) = w.write_all(b": ping\n\n") {
            break e;
        }
        thread::sleep(Duration::from_secs(1));
    };
    app.pages.fetch_sub(1, Ordering::SeqCst);
    Err(closed)
}

fn read_request(stream: &TcpStream) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let mut headers = HashMap::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let len: usize = headers
        .get("content-length")
        .and_then(|l| l.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; len.min(1 << 16)];
    reader.read_exact(&mut body)?;

    let mut form = parse_form(query);
    form.extend(parse_form(&String::from_utf8_lossy(&body)));
    Ok(Request {
        method,
        path: path.to_string(),
        headers,
        form,
    })
}

fn route(req: &Request, app: &Arc<App>) -> Result<Reply, String> {
    let get = |k: &str| req.form.get(k).map_or("", |v| v.trim());
    Ok(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => (200, "text/html; charset=utf-8", PAGE.into()),
        ("GET", "/jobs") => (200, JSON, jobs_json(&app.jobs.lock().unwrap())),
        ("GET", "/files") => (200, JSON, files_json(&app.out).map_err(err)?),
        ("GET", "/info") => (200, JSON, info(app, get("url"))?),
        ("POST", "/download") => {
            download(app, get("url"), get("name"), get("start"), get("end"))?;
            (202, TEXT, String::new())
        }
        ("POST", "/rename") => (200, TEXT, rename(&app.out, get("name"), get("to"))?),
        ("POST", "/delete") => {
            fs::remove_file(library_file(&app.out, get("name"))?).map_err(err)?;
            (200, TEXT, String::new())
        }
        _ => (404, TEXT, "not found".into()),
    })
}

fn write_head(
    stream: &TcpStream,
    status: u16,
    ctype: &str,
    len: Option<u64>,
    extra: &str,
) -> io::Result<()> {
    let len = len.map_or(String::new(), |l| format!("Content-Length: {l}\r\n"));
    let mut w = stream;
    write!(
        w,
        "HTTP/1.1 {status} \r\nContent-Type: {ctype}\r\n{len}{extra}Connection: close\r\n\r\n"
    )
}

/// Stream a library video, honouring Range requests so the player can seek.
fn serve_media(stream: &TcpStream, out: &Path, name: &str, range: Option<&str>) -> io::Result<()> {
    let Ok(path) = library_file(out, name) else {
        return reply(stream, (404, TEXT, "not found".into()));
    };
    let mut file = File::open(&path)?;
    let size = file.metadata()?.len();
    let span = range.and_then(|r| byte_range(r, size));
    let (start, end) = span.unwrap_or((0, size.saturating_sub(1)));
    let (status, extra) = match span {
        Some(_) => (
            206,
            format!("Accept-Ranges: bytes\r\nContent-Range: bytes {start}-{end}/{size}\r\n"),
        ),
        None => (200, "Accept-Ranges: bytes\r\n".into()),
    };
    let len = if size == 0 { 0 } else { end + 1 - start };
    write_head(
        stream,
        status,
        mime(&path).unwrap_or(JSON),
        Some(len),
        &extra,
    )?;
    file.seek(SeekFrom::Start(start))?;
    let mut w = stream;
    io::copy(&mut file.take(len), &mut w).map(drop)
}

fn byte_range(r: &str, size: u64) -> Option<(u64, u64)> {
    let (a, b) = r.strip_prefix("bytes=")?.split_once('-')?;
    let last = size.checked_sub(1)?;
    let (start, end) = if a.is_empty() {
        (size.saturating_sub(b.parse().ok()?), last)
    } else {
        (a.parse().ok()?, b.parse().unwrap_or(last).min(last))
    };
    (start <= end).then_some((start, end))
}

/// Title and duration of a video, for the download form preview.
fn info(app: &App, url: &str) -> Result<String, String> {
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
        .map_err(err)?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    match (out.status.success(), lines.next(), lines.next()) {
        (true, Some(duration), Some(title)) => Ok(json_obj(&[
            ("title", json_str(title)),
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

/// Validate the form, then run yt-dlp in the background.
fn download(app: &Arc<App>, url: &str, name: &str, start: &str, end: &str) -> Result<(), String> {
    check_url(url)?;
    let name = clean_name(name);
    let taken = app.out.join(format!("{name}.mp4")).exists()
        || app
            .jobs
            .lock()
            .unwrap()
            .iter()
            .any(|j| j.status == "running" && j.label == name);
    if !name.is_empty() && taken {
        return Err(format!("{name}.mp4 already exists. Pick another name."));
    }
    if let (Some(s), Some(e)) = (seconds(start)?, seconds(end)?) {
        if e <= s {
            return Err("End must be after start.".into());
        }
    }

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
    let mut child = cmd
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not run yt-dlp: {e}"))?;

    let id = {
        let mut jobs = app.jobs.lock().unwrap();
        let label = or(&name, url).to_string();
        jobs.push(Job {
            label,
            status: "running",
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
        job.status = if ok { "done" } else { "error" };
        if ok {
            job.progress = 100.0;
        }
    });
    Ok(())
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

fn rename(out: &Path, name: &str, to: &str) -> Result<String, String> {
    let from = library_file(out, name)?;
    let to = clean_name(to);
    if to.is_empty() {
        return Err("Enter a new name.".into());
    }
    let ext = from.extension().unwrap_or_default().to_string_lossy();
    let new_name = format!("{to}.{ext}");
    let target = out.join(&new_name);
    if target.exists() && target != from {
        return Err(format!("{new_name} already exists."));
    }
    fs::rename(from, target).map_err(err)?;
    Ok(new_name)
}

/// A video file directly inside the library folder.
fn library_file(out: &Path, name: &str) -> Result<PathBuf, String> {
    let path = out.join(name);
    if name.contains('/') || name.starts_with('.') || !path.is_file() || mime(&path).is_none() {
        return Err(format!("{name} not found."));
    }
    Ok(path)
}

fn mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    VIDEO_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// Library videos, newest first.
fn files_json(out: &Path) -> io::Result<String> {
    let mut files: Vec<(u64, String, u64)> = fs::read_dir(out)?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let meta = e.metadata().ok()?;
            if !meta.is_file() || name.starts_with('.') || mime(&e.path()).is_none() {
                return None;
            }
            let modified = meta
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((modified, name, meta.len()))
        })
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    Ok(json_array(files.iter().map(|(_, name, size)| {
        json_obj(&[("name", json_str(name)), ("size", size.to_string())])
    })))
}

fn jobs_json(jobs: &[Job]) -> String {
    json_array(jobs.iter().rev().map(|j| {
        json_obj(&[
            ("label", json_str(&j.label)),
            ("status", json_str(j.status)),
            ("progress", j.progress.to_string()),
            ("message", json_str(&j.message)),
        ])
    }))
}

fn check_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") || url.starts_with("http://") {
        Ok(())
    } else {
        Err("Enter a valid link.".into())
    }
}

/// A file name without extension: invalid characters replaced, a typed video extension
/// dropped, and cut to fit the file name length limit.
fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim();
    let name = match name.rsplit_once('.') {
        Some((base, ext)) if VIDEO_TYPES.iter().any(|(e, _)| ext.eq_ignore_ascii_case(e)) => base,
        _ => name,
    };
    let mut name = name.trim().trim_matches('.');
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name = &name[..end];
    name.trim_end().to_string()
}

fn finite(s: &str) -> Option<f64> {
    s.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parse "", "90", "1:30" or "1:02:03.5" into seconds.
fn seconds(t: &str) -> Result<Option<f64>, String> {
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("Bad time '{t}'. Use seconds or minutes like 1:30.");
    if t.split(':').count() > 3
        || !t
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ':')
    {
        return Err(bad());
    }
    t.split(':')
        .try_fold(0.0, |acc, part| part.parse::<f64>().map(|v| acc * 60.0 + v))
        .map(Some)
        .map_err(|_| bad())
}

fn or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.is_empty() {
        default
    } else {
        s
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b
            .get(i + 1..i + 3)
            .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (b[i], hex) {
            (b'+', _) => out.push(b' '),
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 2;
            }
            (c, _) => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into()
}

fn json_obj(fields: &[(&str, String)]) -> String {
    let fields: Vec<String> = fields.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    format!("{{{}}}", fields.join(","))
}

fn json_array(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(","))
}

fn json_str(s: &str) -> String {
    let mut o = String::from('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                o.push('\\');
                o.push(c);
            }
            c if c.is_control() => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_name_strips_bad_chars_and_typed_extension() {
        assert_eq!(clean_name(" a/b:c.MP4 "), "a_b_c");
        assert_eq!(clean_name("song.mp3"), "song.mp3");
        assert_eq!(clean_name("..hidden.."), "hidden");
        assert_eq!(clean_name("50% off"), "50% off");
    }

    #[test]
    fn clean_name_fits_length_limit_on_char_boundary() {
        let name = clean_name(&"歌".repeat(100));
        assert!(name.len() <= MAX_NAME_BYTES);
        assert_eq!(name, "歌".repeat(MAX_NAME_BYTES / 3));
    }

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
    fn byte_range_handles_open_suffix_and_invalid() {
        assert_eq!(byte_range("bytes=0-", 10), Some((0, 9)));
        assert_eq!(byte_range("bytes=2-4", 10), Some((2, 4)));
        assert_eq!(byte_range("bytes=5-99", 10), Some((5, 9)));
        assert_eq!(byte_range("bytes=-3", 10), Some((7, 9)));
        assert_eq!(byte_range("bytes=20-", 10), None);
        assert_eq!(byte_range("bytes=0-", 0), None);
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

    #[test]
    fn decode_handles_plus_percent_and_utf8() {
        assert_eq!(decode("a+b%2B%E6%AD%8C%zz"), "a b+歌%zz");
    }
}
