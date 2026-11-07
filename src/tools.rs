//! yt-dlp and ffmpeg: embedded in release builds, taken from PATH in a dev build.
//!
//! The release build carries both programs as gzip streams and unpacks them to the
//! user's cache folder once per build, so the binary runs without Nix or any
//! separately installed downloader.

use std::path::PathBuf;

use anyhow::Result;

#[derive(Clone)]
pub struct Tools {
    pub yt_dlp: PathBuf,
    /// The ffmpeg binary to hand yt-dlp, or none to let it look on PATH.
    pub ffmpeg: Option<PathBuf>,
}

/// Find the tools this build uses, unpacking the embedded ones on first run.
pub fn prepare() -> Result<Tools> {
    #[cfg(feature = "bundle")]
    {
        extract()
    }
    #[cfg(not(feature = "bundle"))]
    {
        Ok(Tools {
            yt_dlp: "yt-dlp".into(),
            ffmpeg: None,
        })
    }
}

#[cfg(feature = "bundle")]
fn extract() -> Result<Tools> {
    use std::fs;
    use std::io::{copy, Write};
    use std::path::Path;

    use anyhow::Context;
    use flate2::read::GzDecoder;

    static YT_DLP: &[u8] = include_bytes!(env!("YTCUT_YTDLP"));
    static FFMPEG: &[u8] = include_bytes!(env!("YTCUT_FFMPEG"));

    /// Unpack one gzip stream to an executable, without leaving a half-written file.
    fn unpack(data: &[u8], path: &Path) -> Result<()> {
        let temp = path.with_extension("part");
        let mut file = fs::File::create(&temp)
            .with_context(|| format!("could not write {}", temp.display()))?;
        copy(&mut GzDecoder::new(data), &mut file)
            .with_context(|| format!("could not unpack {}", path.display()))?;
        file.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(&temp, path)?;
        Ok(())
    }

    let dir = cache_dir()?.join(env!("YTCUT_TOOLS_ID"));
    let suffix = std::env::consts::EXE_SUFFIX;
    let yt_dlp = dir.join(format!("yt-dlp{suffix}"));
    let ffmpeg = dir.join(format!("ffmpeg{suffix}"));
    let ready = dir.join(".ready");
    if !ready.is_file() || !yt_dlp.is_file() || !ffmpeg.is_file() {
        fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
        unpack(YT_DLP, &yt_dlp)?;
        unpack(FFMPEG, &ffmpeg)?;
        fs::write(&ready, b"")?;
    }
    Ok(Tools {
        yt_dlp,
        ffmpeg: Some(ffmpeg),
    })
}

/// `%LOCALAPPDATA%\ytcut` on Windows, `$XDG_CACHE_HOME/ytcut` or `~/.cache/ytcut` elsewhere.
#[cfg(feature = "bundle")]
fn cache_dir() -> Result<PathBuf> {
    use std::env;

    use anyhow::bail;

    if cfg!(windows) {
        match env::var_os("LOCALAPPDATA").filter(|dir| !dir.is_empty()) {
            Some(base) => return Ok(PathBuf::from(base).join("ytcut")),
            None => bail!("LOCALAPPDATA is not set"),
        }
    }
    if let Some(base) = env::var_os("XDG_CACHE_HOME").filter(|dir| !dir.is_empty()) {
        return Ok(PathBuf::from(base).join("ytcut"));
    }
    match env::var_os("HOME").filter(|dir| !dir.is_empty()) {
        Some(home) => Ok(PathBuf::from(home).join(".cache").join("ytcut")),
        None => bail!("neither XDG_CACHE_HOME nor HOME is set"),
    }
}
