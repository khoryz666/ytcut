//! Local web frontend for yt-dlp: download a video, optionally trim it, name the file.
//! Usage: ytcut [OUT_DIR]   (PORT env var overrides the default 8765)

use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::{env, thread};

const PAGE: &str = include_str!("index.html");

struct Job {
    label: String,
    status: &'static str,
    progress: f32,
    message: String,
}

type Jobs = Arc<Mutex<Vec<Job>>>;

fn main() -> io::Result<()> {
    let out: PathBuf = env::args().nth(1).map_or(env::current_dir()?, PathBuf::from).canonicalize()?;
    let port = env::var("PORT").unwrap_or_else(|_| "8765".into());
    let listener = TcpListener::bind(format!("127.0.0.1:{port}"))?;
    println!("http://127.0.0.1:{port}  ->  {}", out.display());

    let jobs: Jobs = Default::default();
    for stream in listener.incoming().flatten() {
        let (jobs, out) = (jobs.clone(), out.clone());
        thread::spawn(move || handle(stream, &jobs, &out));
    }
    Ok(())
}

fn handle(stream: TcpStream, jobs: &Jobs, out: &Path) -> io::Result<()> {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));

    let mut len = 0;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = header.split_once(':') {
            if k.eq_ignore_ascii_case("content-length") {
                len = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut body = vec![0; len.min(1 << 16)];
    reader.read_exact(&mut body)?;

    let (status, ctype, body) = match (method, path) {
        ("GET", "/") => (200, "text/html; charset=utf-8", PAGE.to_string()),
        ("GET", "/jobs") => (200, "application/json", jobs_json(&jobs.lock().unwrap())),
        ("POST", "/download") => match start(&form(&String::from_utf8_lossy(&body)), jobs, out) {
            Ok(()) => (202, "text/plain", String::new()),
            Err(e) => (400, "text/plain; charset=utf-8", e),
        },
        _ => (404, "text/plain", "not found".into()),
    };
    write!(
        &stream,
        "HTTP/1.1 {status} \r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// Validate the form, then run yt-dlp in the background.
fn start(f: &HashMap<String, String>, jobs: &Jobs, out: &Path) -> Result<(), String> {
    let get = |k: &str| f.get(k).map_or("", |v| v.trim());
    let (url, start, end) = (get("url"), get("start"), get("end"));
    let name: String = get("name")
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c })
        .collect();
    let name = name.trim_matches('.');

    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err("Enter a valid link".into());
    }
    if !name.is_empty() && out.join(format!("{name}.mp4")).exists() {
        return Err(format!("{name}.mp4 already exists"));
    }
    if let (Some(s), Some(e)) = (seconds(start)?, seconds(end)?) {
        if e <= s {
            return Err("End must be after start".into());
        }
    }

    // `%` would be read as a yt-dlp output template field.
    let template = if name.is_empty() { "%(title)s".into() } else { name.replace('%', "%%") };
    let mut cmd = Command::new("yt-dlp");
    cmd.args(["--no-playlist", "--newline", "--progress"])
        .args(["--progress-template", "download:PROGRESS %(progress._percent_str)s"])
        .args(["--print", "after_move:filepath"])
        // Prefer H.264 + AAC so the MP4 plays everywhere.
        .args(["-f", "bv*+ba/b", "-S", "vcodec:h264,res,acodec:aac", "--merge-output-format", "mp4"])
        .arg("-o")
        .arg(out.join(format!("{template}.%(ext)s")));
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
        let mut jobs = jobs.lock().unwrap();
        let label = or(name, url).to_string();
        jobs.push(Job { label, status: "running", progress: 0.0, message: String::new() });
        jobs.len() - 1
    };
    let readers = [
        watch(child.stdout.take().unwrap(), jobs.clone(), id),
        watch(child.stderr.take().unwrap(), jobs.clone(), id),
    ];
    let jobs = jobs.clone();
    thread::spawn(move || {
        readers.into_iter().for_each(|r| drop(r.join()));
        let ok = child.wait().is_ok_and(|s| s.success());
        let job = &mut jobs.lock().unwrap()[id];
        job.status = if ok { "done" } else { "error" };
        if ok {
            job.progress = 100.0;
        }
    });
    Ok(())
}

/// Feed yt-dlp output lines into the job: progress, final file path, or last error.
fn watch(stream: impl Read + Send + 'static, jobs: Jobs, id: usize) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            let job = &mut jobs.lock().unwrap()[id];
            if let Some(p) = line.strip_prefix("PROGRESS ") {
                job.progress = p.trim().trim_end_matches('%').parse().unwrap_or(job.progress);
            } else if line.starts_with('/') {
                job.message = Path::new(&line).file_name().unwrap_or_default().to_string_lossy().into();
            } else if line.starts_with("ERROR") {
                job.message = line;
            }
        }
    })
}

/// Parse "", "90", "1:30" or "1:02:03.5" into seconds.
fn seconds(t: &str) -> Result<Option<f64>, String> {
    if t.is_empty() {
        return Ok(None);
    }
    let bad = || format!("Bad time '{t}' (use seconds or H:MM:SS)");
    if t.split(':').count() > 3 || !t.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':') {
        return Err(bad());
    }
    t.split(':')
        .try_fold(0.0, |acc, part| part.parse::<f64>().map(|v| acc * 60.0 + v))
        .map(Some)
        .map_err(|_| bad())
}

fn or<'a>(s: &'a str, default: &'a str) -> &'a str {
    if s.is_empty() { default } else { s }
}

fn form(body: &str) -> HashMap<String, String> {
    body.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (decode(k), decode(v))).collect()
}

fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = b.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
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

fn jobs_json(jobs: &[Job]) -> String {
    let items: Vec<String> = jobs
        .iter()
        .rev()
        .map(|j| {
            format!(
                r#"{{"label":{},"status":"{}","progress":{},"message":{}}}"#,
                json_str(&j.label),
                j.status,
                j.progress,
                json_str(&j.message)
            )
        })
        .collect();
    format!("[{}]", items.join(","))
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
