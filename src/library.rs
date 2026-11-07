//! The video folder: listing, renaming, deleting, and safe file names.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;

/// Leaves room for an extension and yt-dlp's temporary suffixes within 255 bytes.
pub const MAX_NAME_BYTES: usize = 200;

const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "m4v", "webm", "mkv", "mov"];

#[derive(Serialize)]
pub struct VideoFile {
    pub name: String,
    pub size: u64,
}

/// A video file directly inside the folder.
pub fn find(out: &Path, name: &str) -> Result<PathBuf, String> {
    let path = out.join(name);
    let video = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| VIDEO_EXTENSIONS.iter().any(|v| v.eq_ignore_ascii_case(e)));
    if name.contains('/') || name.starts_with('.') || !path.is_file() || !video {
        return Err(format!("{name} not found."));
    }
    Ok(path)
}

/// The folder's videos, newest first.
pub fn list(out: &Path) -> io::Result<Vec<VideoFile>> {
    let mut files: Vec<(u64, VideoFile)> = fs::read_dir(out)?
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
                VideoFile {
                    name,
                    size: meta.len(),
                },
            ))
        })
        .collect();
    files.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    Ok(files.into_iter().map(|(_, file)| file).collect())
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
        Some((base, ext)) if VIDEO_EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e)) => base,
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
