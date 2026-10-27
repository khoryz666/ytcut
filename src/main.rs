//! ytcut: local web app to download YouTube videos (optionally trimmed) and play them on repeat.
//! Usage: ytcut [FOLDER]   (PORT env var overrides the default 8765)

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
use std::{env, thread};

const PAGE: &str = include_str!("index.html");
const JSON: &str = "application/json";
const TEXT: &str = "text/plain; charset=utf-8";
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
    progress: f32,
    message: String,
}

struct App {
    out: PathBuf,
    jobs: Mutex<Vec<Job>>,
}

struct Request {
    method: String,
    path: String,
    range: Option<String>,
    /// Query string and form body fields together.
    form: HashMap<String, String>,
}

type Reply = (u16, &'static str, String);

fn main() -> io::Result<()> {
    let out = env::args()
        .nth(1)
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
        jobs: Mutex::default(),
    });
    for stream in listener.incoming().flatten() {
        let app = app.clone();
        thread::spawn(move || handle(&stream, &app));
    }
    Ok(())
}

fn handle(stream: &TcpStream, app: &Arc<App>) -> io::Result<()> {
    let req = read_request(stream)?;
    if let ("GET", Some(name)) = (req.method.as_str(), req.path.strip_prefix("/media/")) {
        return serve_media(stream, &app.out, &decode(name), req.range.as_deref());
    }
    let (status, ctype, body) = route(&req, app).unwrap_or_else(|e| (400, TEXT, e));
    write_head(stream, status, ctype, body.len() as u64, "")?;
    let mut w = stream;
    w.write_all(body.as_bytes())
}

fn read_request(stream: &TcpStream) -> io::Result<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("");
    let (path, query) = target.split_once('?').unwrap_or((target, ""));

    let (mut len, mut range) = (0, None);
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            } else if k.eq_ignore_ascii_case("range") {
                range = Some(v.trim().to_string());
            }
        }
    }
    let mut body = vec![0; len.min(1 << 16)];
    reader.read_exact(&mut body)?;

    let mut form = parse_form(query);
    form.extend(parse_form(&String::from_utf8_lossy(&body)));
    Ok(Request {
        method,
        path: path.to_string(),
        range,
        form,
    })
}

fn route(req: &Request, app: &Arc<App>) -> Result<Reply, String> {
    let get = |k: &str| req.form.get(k).map_or("", |v| v.trim());
    Ok(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/") => (200, "text/html; charset=utf-8", PAGE.into()),
        ("GET", "/jobs") => (200, JSON, jobs_json(&app.jobs.lock().unwrap())),
        ("GET", "/files") => (200, JSON, files_json(&app.out).map_err(err)?),
        ("GET", "/info") => (200, JSON, info(get("url"))?),
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
    len: u64,
    extra: &str,
) -> io::Result<()> {
    let mut w = stream;
    write!(w, "HTTP/1.1 {status} \r\nContent-Type: {ctype}\r\nContent-Length: {len}\r\n{extra}Connection: close\r\n\r\n")
}

/// Stream a library video, honouring Range requests so the player can seek.
fn serve_media(stream: &TcpStream, out: &Path, name: &str, range: Option<&str>) -> io::Result<()> {
    let Ok(path) = library_file(out, name) else {
        return write_head(stream, 404, TEXT, 0, "");
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
    write_head(stream, status, mime(&path).unwrap_or(JSON), len, &extra)?;
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
fn info(url: &str) -> Result<String, String> {
    check_url(url)?;
    let out = Command::new("yt-dlp")
        .args([
            "--no-playlist",
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
                duration
                    .parse::<f64>()
                    .map_or("null".into(), |d| d.to_string()),
            ),
        ])),
        _ => Err("Could not find this video. Check the link.".into()),
    }
}

/// Validate the form, then run yt-dlp in the background.
fn download(app: &Arc<App>, url: &str, name: &str, start: &str, end: &str) -> Result<(), String> {
    check_url(url)?;
    let name = clean_name(name);
    if !name.is_empty() && app.out.join(format!("{name}.mp4")).exists() {
        return Err(format!("{name}.mp4 already exists. Pick another name."));
    }
    if let (Some(s), Some(e)) = (seconds(start)?, seconds(end)?) {
        if e <= s {
            return Err("End must be after start.".into());
        }
    }

    // `%` would be read as a yt-dlp output template field.
    let template = if name.is_empty() {
        "%(title)s".into()
    } else {
        name.replace('%', "%%")
    };
    let mut cmd = Command::new("yt-dlp");
    cmd.args(["--no-playlist", "--newline", "--progress"])
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
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let job = &mut app.jobs.lock().unwrap()[id];
            if let Some(p) = line.strip_prefix("PROGRESS ") {
                job.progress = p
                    .trim()
                    .trim_end_matches('%')
                    .parse()
                    .unwrap_or(job.progress);
            } else if line.starts_with('/') {
                job.message = Path::new(&line)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
            } else if line.starts_with("ERROR") {
                job.message = line;
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

/// Replace characters that are invalid in file names.
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
    name.trim().trim_matches('.').to_string()
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
