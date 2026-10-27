//! A tiny HTTP/1.1 layer: read one request, write one response, then close.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

pub const JSON: &str = "application/json";
pub const TEXT: &str = "text/plain; charset=utf-8";

/// Status, content type and body.
pub type Reply = (u16, &'static str, String);

pub struct Request {
    pub method: String,
    pub path: String,
    /// Header names in lower case.
    headers: HashMap<String, String>,
    /// Query string and form body fields together.
    form: HashMap<String, String>,
}

impl Request {
    pub fn read(stream: &TcpStream) -> io::Result<Self> {
        // An idle connection must not hold its thread forever.
        stream.set_read_timeout(Some(Duration::from_secs(30)))?;
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

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    /// A query or form field, trimmed; empty when missing.
    pub fn field(&self, name: &str) -> &str {
        self.form.get(name).map_or("", |v| v.trim())
    }
}

pub fn reply(stream: &TcpStream, (status, ctype, body): Reply) -> io::Result<()> {
    write_head(stream, status, ctype, Some(body.len() as u64), "")?;
    let mut w = stream;
    w.write_all(body.as_bytes())
}

/// Status line and headers. Without a length, the body runs until the connection closes.
pub fn write_head(
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

/// Send a file, honouring a Range header so video players can seek.
pub fn send_file(
    stream: &TcpStream,
    path: &Path,
    ctype: &str,
    range: Option<&str>,
) -> io::Result<()> {
    let mut file = File::open(path)?;
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
    write_head(stream, status, ctype, Some(len), &extra)?;
    file.seek(SeekFrom::Start(start))?;
    let mut w = stream;
    io::copy(&mut file.take(len), &mut w).map(drop)
}

/// Parse "bytes=start-end" (either end optional) into an inclusive range within the file.
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

fn parse_form(body: &str) -> HashMap<String, String> {
    body.split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect()
}

/// Percent-decode a URL or form value ("+" is a space).
pub fn decode(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn decode_handles_plus_percent_and_utf8() {
        assert_eq!(decode("a+b%2B%E6%AD%8C%zz"), "a b+歌%zz");
    }
}
