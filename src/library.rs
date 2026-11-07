//! The music folder: listing, renaming, deleting, and safe file names.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// Leaves room for an extension and yt-dlp's temporary suffixes within 255 bytes.
pub const MAX_NAME_BYTES: usize = 200;

/// Audio files ytcut plays.
pub const AUDIO_EXTENSIONS: [&str; 9] = [
    "m4a", "mp3", "m4b", "aac", "ogg", "oga", "opus", "flac", "wav",
];

/// Video extensions stripped from clean_name.
pub const VIDEO_EXTENSIONS: [&str; 9] = [
    "mp4", "m4v", "mkv", "mov", "avi", "flv", "webm", "3gp", "ts",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Song {
    pub name: String,
    pub size: u64,
}

impl Song {
    pub fn formatted_size(&self) -> String {
        format_size(self.size)
    }
}

pub fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// A song directly inside the folder.
pub fn find(out: &Path, name: &str) -> Result<PathBuf, String> {
    let path = out.join(name);
    let is_audio = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTENSIONS.iter().any(|v| v.eq_ignore_ascii_case(e)));
    // Windows also treats `\\` and `C:` as path syntax.
    let plain = !name.contains(['/', '\\', ':']) && !name.starts_with('.');
    if !plain || !path.is_file() || !is_audio {
        return Err(format!("{name} not found."));
    }
    Ok(path)
}

/// The folder's songs, newest first. Only audio files.
pub fn list(out: &Path) -> io::Result<Vec<Song>> {
    let mut files: Vec<(u64, Song)> = fs::read_dir(out)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            find(out, &name).ok()?;
            let meta = entry.metadata().ok()?;
            let modified = meta
                .modified()
                .ok()?
                .duration_since(UNIX_EPOCH)
                .ok()?
                .as_secs();
            Some((
                modified,
                Song {
                    name,
                    size: meta.len(),
                },
            ))
        })
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    Ok(files.into_iter().map(|(_, file)| file).collect())
}

/// Rename a song, keeping its extension. Returns the new file name.
pub fn rename(out: &Path, name: &str, to: &str) -> Result<String, String> {
    let from = find(out, name)?;
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
    fs::rename(from, target).map_err(|e| e.to_string())?;
    Ok(new_name)
}

pub fn delete(out: &Path, name: &str) -> Result<(), String> {
    fs::remove_file(find(out, name)?).map_err(|e| e.to_string())
}

/// A file name without extension: invalid characters replaced, a typed audio or video
/// extension dropped, and cut to fit the file name length limit. `$` goes too, because
/// yt-dlp expands `$HOME` in its output template and has no escape for it.
pub fn clean_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| {
            if "/\\:*?\"<>|$".contains(c) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let name = name.trim();
    let known = |ext: &str| {
        AUDIO_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))
            || VIDEO_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))
    };
    let name = match name.rsplit_once('.') {
        Some((base, ext)) if known(ext) => base,
        _ => name,
    };
    let name = name.trim().trim_matches('.');
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let name = name[..end].trim_end_matches([' ', '.']);
    // Windows reserves these names whatever the extension.
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.ends_with(|c: char| c.is_ascii_digit()));
    if reserved {
        format!("_{name}")
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_name_strips_bad_chars_and_typed_extension() {
        assert_eq!(clean_name(" a/b:c.MP4 "), "a_b_c");
        assert_eq!(clean_name("song.m4a"), "song");
        assert_eq!(clean_name("song.mp3"), "song");
        assert_eq!(clean_name("..hidden.."), "hidden");
        assert_eq!(clean_name("50% off"), "50% off");
        assert_eq!(clean_name("$HOME"), "_HOME");
        assert_eq!(clean_name("con"), "_con");
        assert_eq!(clean_name("COM1.live"), "_COM1.live");
        assert_eq!(clean_name("console"), "console");
    }

    #[test]
    fn find_rejects_path_syntax() {
        let dir = std::env::temp_dir();
        for name in [
            "../a.m4a",
            "a/b.m4a",
            "..\\a.m4a",
            "x\\..\\a.m4a",
            "C:\\a.m4a",
            ".a.m4a",
            "video.mp4",
        ] {
            assert!(find(&dir, name).is_err(), "{name}");
        }
    }

    #[test]
    fn clean_name_fits_length_limit_on_char_boundary() {
        let name = clean_name(&"歌".repeat(100));
        assert!(name.len() <= MAX_NAME_BYTES);
        assert_eq!(name, "歌".repeat(MAX_NAME_BYTES / 3));
    }
}
