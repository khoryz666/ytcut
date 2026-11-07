//! yt-dlp, ffmpeg and deno: downloaded from their own releases on first use and checked
//! against the checksums published there. After that yt-dlp keeps itself current, which
//! matters because YouTube keeps breaking old versions.

use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};

#[cfg(not(all(target_arch = "x86_64", any(target_os = "linux", windows))))]
compile_error!("ytcut only knows the tool downloads for x86-64 Linux and Windows");

/// A release file: its URL, the URL of its checksums, and where the program is inside it.
struct Asset {
    url: &'static str,
    sums: &'static str,
    /// The path of the program in the archive, or none when the file is the program.
    inside: Option<&'static str>,
}

#[cfg(not(windows))]
const ASSETS: [Asset; 3] = [
    Asset {
        url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux",
        sums: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS",
        inside: None,
    },
    Asset {
        url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz",
        sums: "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256",
        inside: Some("ffmpeg-master-latest-linux64-gpl/bin/ffmpeg"),
    },
    Asset {
        url: "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip",
        sums: "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-unknown-linux-gnu.zip.sha256sum",
        inside: Some("deno"),
    },
];

#[cfg(windows)]
const ASSETS: [Asset; 3] = [
    Asset {
        url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
        sums: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/SHA2-256SUMS",
        inside: None,
    },
    Asset {
        url: "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip",
        sums: "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/checksums.sha256",
        inside: Some("ffmpeg-master-latest-win64-gpl/bin/ffmpeg.exe"),
    },
    Asset {
        url: "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip",
        sums: "https://github.com/denoland/deno/releases/latest/download/deno-x86_64-pc-windows-msvc.zip.sha256sum",
        inside: Some("deno.exe"),
    },
];

#[derive(Clone, Debug)]
pub struct Tools {
    pub yt_dlp: PathBuf,
    pub ffmpeg: PathBuf,
    pub deno: PathBuf,
}

/// The tools in `dir`, downloading any that are missing or falling back to PATH.
pub fn prepare(dir: &Path) -> Result<Tools> {
    let _ = fs::create_dir_all(dir);
    let path = |name: &str| dir.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    let mut tools = Tools {
        yt_dlp: path("yt-dlp"),
        ffmpeg: path("ffmpeg"),
        deno: path("deno"),
    };
    let [yt_dlp, ffmpeg, deno] = &ASSETS;
    let _ = install(yt_dlp, &tools.yt_dlp);
    let _ = install(ffmpeg, &tools.ffmpeg);
    let _ = install(deno, &tools.deno);

    // Fall back to system PATH if missing in dir
    if !tools.yt_dlp.is_file() {
        if let Some(p) = find_in_path("yt-dlp") {
            tools.yt_dlp = p;
        }
    }
    if !tools.ffmpeg.is_file() {
        if let Some(p) = find_in_path("ffmpeg") {
            tools.ffmpeg = p;
        }
    }
    if !tools.deno.is_file() {
        if let Some(p) = find_in_path("deno") {
            tools.deno = p;
        }
    }

    if tools.yt_dlp.is_file() {
        Ok(tools)
    } else {
        bail!("yt-dlp is not available (failed downloading and not found in PATH)");
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name).with_extension(std::env::consts::EXE_EXTENSION))
        .find(|file| file.is_file())
}

/// Download, check and unpack one tool. It only appears at `dest` once complete.
fn install(asset: &Asset, dest: &Path) -> Result<()> {
    if dest.is_file() {
        return Ok(());
    }
    println!("downloading {}", asset.url);
    let temp = dest.with_extension("download");
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp)?;

    let file_name = asset.url.rsplit('/').next().unwrap_or_default();
    let archive = temp.join(file_name);

    // Download archive file
    {
        let resp = ureq::get(asset.url)
            .call()
            .with_context(|| format!("could not download {}", asset.url))?;
        let mut reader = resp.into_reader();
        let mut writer = BufWriter::new(File::create(&archive)?);
        io::copy(&mut reader, &mut writer)
            .with_context(|| format!("failed writing {file_name}"))?;
    }

    // Download checksum file
    let sums = {
        let resp = ureq::get(asset.sums)
            .call()
            .with_context(|| format!("could not download checksums from {}", asset.sums))?;
        resp.into_string()
            .with_context(|| format!("failed reading checksums for {file_name}"))?
    };

    let expected = expected_hash(&sums, file_name)
        .with_context(|| format!("no published checksum for {file_name}"))?;
    let actual = sha256(&archive)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("{file_name} does not match its published checksum");
    }

    let program = match asset.inside {
        Some(inside) => {
            let extracted = temp.join("extracted_bin");
            if file_name.ends_with(".tar.xz") {
                let tar_path = temp.join("archive.tar");
                {
                    let mut src = BufReader::new(File::open(&archive)?);
                    let mut dst = BufWriter::new(File::create(&tar_path)?);
                    lzma_rs::xz_decompress(&mut src, &mut dst)
                        .with_context(|| format!("could not decompress xz {file_name}"))?;
                }
                let mut tar_archive = tar::Archive::new(File::open(&tar_path)?);
                let mut found = false;
                for entry in tar_archive.entries()? {
                    let mut entry = entry?;
                    if entry.path()? == Path::new(inside) {
                        entry.unpack(&extracted)?;
                        found = true;
                        break;
                    }
                }
                ensure!(found, "could not find {inside} inside {file_name}");
            } else if file_name.ends_with(".zip") {
                let mut zip_archive = zip::ZipArchive::new(File::open(&archive)?)
                    .with_context(|| format!("could not open zip {file_name}"))?;
                let mut zip_file = zip_archive
                    .by_name(inside)
                    .with_context(|| format!("could not find {inside} inside {file_name}"))?;
                let mut dest_file = File::create(&extracted)?;
                io::copy(&mut zip_file, &mut dest_file)?;
            } else {
                bail!("unsupported archive format for {file_name}");
            }
            extracted
        }
        None => archive,
    };

    ensure!(program.is_file(), "{file_name} did not contain the program");
    make_executable(&program)?;
    fs::rename(&program, dest)?;
    let _ = fs::remove_dir_all(&temp);
    Ok(())
}

#[cfg(unix)]
pub fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
pub fn make_executable(_path: &Path) -> Result<()> {
    Ok(())
}

pub fn sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0; 1 << 20];
    loop {
        let n = file.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        hasher.update(&chunk[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// The hash for `file_name` in a checksum file. Most list `HASH  NAME` lines. Deno's
/// Windows one is PowerShell output for a single file, whose path was the build machine's.
pub fn expected_hash<'a>(sums: &'a str, file_name: &str) -> Option<&'a str> {
    let is_hash = |word: &&str| word.len() == 64 && word.bytes().all(|b| b.is_ascii_hexdigit());
    let listed = sums
        .lines()
        .find(|line| line.split_whitespace().last() == Some(file_name))
        .and_then(|line| line.split_whitespace().find(is_hash));
    listed.or_else(|| {
        let mut hashes = sums.split_whitespace().filter(is_hash);
        hashes.next().filter(|_| hashes.next().is_none())
    })
}

/// Let yt-dlp replace itself with the latest release. Failing is fine: it tries again next start.
pub fn update_yt_dlp(tools: &Tools) {
    let _ = Command::new(&tools.yt_dlp).arg("-U").output();
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "767ba9dce7052aa196f8b12652f433d76de128ba168966fa84b6394d5e6c6795";
    const OTHER: &str = "f5389d3698ecde4ef06f4e6b1ce0294fe9ac1396228c8990b8d3416d6d177a7d";

    #[test]
    fn expected_hash_finds_the_exact_name_in_a_list() {
        let sums = format!("{OTHER}  yt-dlp_linux.zip\n{HASH}  yt-dlp_linux\n");
        assert_eq!(expected_hash(&sums, "yt-dlp_linux"), Some(HASH));
        assert_eq!(expected_hash(&sums, "missing"), None);
    }

    #[test]
    fn expected_hash_reads_powershell_output_for_one_file() {
        let sums = format!(
            "\r\nAlgorithm : SHA256\r\nHash      : {HASH}\r\nPath      : C:\\a\\deno.zip\r\n"
        );
        assert_eq!(expected_hash(&sums, "deno.zip"), Some(HASH));
    }

    #[test]
    fn sha256_hashes_a_file() {
        let path = std::env::temp_dir().join(format!("ytcut-test-{}", std::process::id()));
        fs::write(&path, "hello").unwrap();
        let hash = sha256(&path).unwrap();
        let _ = fs::remove_file(&path);
        assert_eq!(
            hash,
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
