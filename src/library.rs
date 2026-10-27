//! The video folder: listing, renaming, deleting, and safe file names.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::json;

/// Leaves room for an extension and yt-dlp's temporary suffixes within 255 bytes.
pub const MAX_NAME_BYTES: usize = 200;

const VIDEO_TYPES: [(&str, &str); 5] = [
    ("mp4", "video/mp4"),
    ("m4v", "video/mp4"),
    ("webm", "video/webm"),
    ("mkv", "video/x-matroska"),
    ("mov", "video/quicktime"),
];

/// The video MIME type of a path, or None if it is not a video.
pub fn mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    VIDEO_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// A video file directly inside the folder.
pub fn find(out: &Path, name: &str) -> Result<PathBuf, String> {
    let path = out.join(name);
    if name.contains('/') || name.starts_with('.') || !path.is_file() || mime(&path).is_none() {
        return Err(format!("{name} not found."));
    }
    Ok(path)
}

/// The folder's videos as JSON, newest first.
pub fn list_json(out: &Path) -> io::Result<String> {
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
    Ok(json::array(files.iter().map(|(_, name, size)| {
        json::object(&[("name", json::string(name)), ("size", size.to_string())])
    })))
}

/// Rename a video, keeping its extension. Returns the new file name.
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

/// A file name without extension: invalid characters replaced, a typed video extension
/// dropped, and cut to fit the file name length limit.
pub fn clean_name(name: &str) -> String {
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
    let name = name.trim().trim_matches('.');
    let mut end = name.len().min(MAX_NAME_BYTES);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    name[..end].trim_end().to_string()
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
}
