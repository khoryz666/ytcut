//! Which handler answers which request.

use std::io::{self, Write};
use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::app::App;
use crate::http::{self, Reply, Request, JSON, TEXT};
use crate::{downloads, library};

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";
const SVG: &str = "image/svg+xml";

/// A script compiled from src/web/ts by build.rs.
macro_rules! script {
    ($name:literal) => {
        (
            concat!("/js/", $name),
            JS,
            include_str!(concat!(env!("OUT_DIR"), "/js/", $name)),
        )
    };
}

/// The page, compiled into the binary: (path, content type, content).
const ASSETS: [(&str, &str, &str); 11] = [
    ("/", HTML, include_str!("web/index.html")),
    ("/style.css", CSS, include_str!("web/style.css")),
    ("/logo.svg", SVG, include_str!("web/logo.svg")),
    script!("main.js"),
    script!("dom.js"),
    script!("format.js"),
    script!("api.js"),
    script!("cut.js"),
    script!("download.js"),
    script!("library.js"),
    script!("player.js"),
];

pub fn handle(stream: &TcpStream, app: &Arc<App>) -> io::Result<()> {
    let req = Request::read(stream)?;
    if !app.trusted(&req) {
        return http::reply(stream, (403, TEXT, "forbidden".into()));
    }
    if req.method == "GET" {
        if let Some(&(_, ctype, body)) = ASSETS.iter().find(|(path, ..)| *path == req.path) {
            return http::reply(stream, (200, ctype, body.into()));
        }
        if req.path == "/alive" {
            return keep_alive(stream, app);
        }
        if let Some(name) = req.path.strip_prefix("/media/") {
            return media(stream, app, &http::decode(name), req.header("range"));
        }
    }
    http::reply(stream, api(&req, app).unwrap_or_else(|e| (400, TEXT, e)))
}

fn api(req: &Request, app: &Arc<App>) -> Result<Reply, String> {
    let field = |name| req.field(name);
    Ok(match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/jobs") => (200, JSON, downloads::jobs_json(&app.jobs.lock().unwrap())),
        ("GET", "/files") => (
            200,
            JSON,
            library::list_json(&app.out).map_err(|e| e.to_string())?,
        ),
        ("GET", "/info") => (200, JSON, downloads::info(app, field("url"))?),
        ("POST", "/download") => {
            downloads::start(
                app,
                field("url"),
                field("name"),
                field("start"),
                field("end"),
            )?;
            (202, TEXT, String::new())
        }
        ("POST", "/rename") => (
            200,
            TEXT,
            library::rename(&app.out, field("name"), field("to"))?,
        ),
        ("POST", "/delete") => {
            library::delete(&app.out, field("name"))?;
            (200, TEXT, String::new())
        }
        _ => (404, TEXT, "not found".into()),
    })
}

/// Stream a library video.
fn media(stream: &TcpStream, app: &App, name: &str, range: Option<&str>) -> io::Result<()> {
    match library::find(&app.out, name) {
        Ok(path) => {
            let ctype = library::mime(&path).unwrap_or("application/octet-stream");
            http::send_file(stream, &path, ctype, range)
        }
        Err(e) => http::reply(stream, (404, TEXT, e)),
    }
}

/// Hold an event stream open while the page is open; a failed write means it closed.
fn keep_alive(stream: &TcpStream, app: &App) -> io::Result<()> {
    http::write_head(
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
