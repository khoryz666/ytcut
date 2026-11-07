//! Open the page in an app window of a Chromium-based browser, with its own profile.
//! yt-dlp reads that profile's cookies when YouTube asks for a sign-in.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Browser {
    program: PathBuf,
    /// yt-dlp's name for the browser, which decides where it finds the cookie key.
    kind: &'static str,
    profile: PathBuf,
}

impl Browser {
    /// YTCUT_BROWSER if set, or else the first browser found. Profiles live under `data`.
    pub fn find(data: &Path) -> Option<Browser> {
        let chosen = env::var_os("YTCUT_BROWSER").filter(|name| !name.is_empty());
        let program = chosen
            .and_then(|name| locate(Path::new(&name)))
            .or_else(|| candidates().into_iter().find_map(|path| locate(&path)))?;
        let name = program.file_stem()?.to_string_lossy().to_ascii_lowercase();
        let kind = kind(&name);
        Some(Browser {
            program,
            kind,
            profile: data.join(kind),
        })
    }

    /// yt-dlp's `--cookies-from-browser` value.
    pub fn cookies(&self) -> String {
        format!("{}:{}", self.kind, self.profile.join("Default").display())
    }

    fn open(&self, url: &str) -> std::io::Result<()> {
        Command::new(&self.program)
            .arg(format!("--app={url}"))
            .arg("--class=ytcut")
            .arg(format!("--user-data-dir={}", self.profile.display()))
            .args(["--no-first-run", "--no-default-browser-check"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(drop)
    }
}

/// Open the page in the app window, or in the default browser when there is none.
pub fn open(browser: Option<&Browser>, url: &str) {
    let opened = match browser {
        Some(browser) => browser.open(url),
        None => open::that_detached(url),
    };
    if let Err(e) = opened {
        eprintln!("could not open a browser: {e}. Open {url} yourself.");
    }
}

fn kind(name: &str) -> &'static str {
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

/// A program by path, or by name on PATH.
fn locate(program: &Path) -> Option<PathBuf> {
    if program.is_absolute() {
        return program.is_file().then(|| program.to_path_buf());
    }
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(program).with_extension(env::consts::EXE_EXTENSION))
        .find(|file| file.is_file())
}

#[cfg(not(windows))]
fn candidates() -> Vec<PathBuf> {
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
fn candidates() -> Vec<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_matches_yt_dlp_names() {
        assert_eq!(kind("google-chrome-stable"), "chrome");
        assert_eq!(kind("chromium-browser"), "chromium");
        assert_eq!(kind("msedge"), "edge");
        assert_eq!(kind("brave"), "brave");
        assert_eq!(kind("vivaldi-stable"), "vivaldi");
    }
}
