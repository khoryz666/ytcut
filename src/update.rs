//! Self-update following OpenCode update policy:
//! Patch releases update silently in background; Minor/Major notify only.

use std::env;
use std::fs::File;
use std::io::{self, BufReader};
use std::path::Path;
use std::sync::Mutex;
use std::thread;

use anyhow::{bail, ensure, Context, Result};
use semver::Version;

pub static UPDATE_NOTICE: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReleaseGap {
    None,
    Patch,
    Minor,
    Major,
}

pub fn classify_gap(current: &Version, latest: &Version) -> ReleaseGap {
    if latest <= current {
        ReleaseGap::None
    } else if latest.major != current.major {
        ReleaseGap::Major
    } else if latest.minor != current.minor {
        ReleaseGap::Minor
    } else if latest.patch != current.patch {
        ReleaseGap::Patch
    } else {
        ReleaseGap::None
    }
}

pub fn asset_name(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Some("ytcut-x86_64-unknown-linux-gnu.tar.gz"),
        ("windows", "x86_64") => Some("ytcut-x86_64-pc-windows-msvc.zip"),
        _ => None,
    }
}

pub fn current_asset_name() -> Option<&'static str> {
    asset_name(env::consts::OS, env::consts::ARCH)
}

pub fn fetch_latest_version() -> Result<Version> {
    let resp = ureq::get("https://github.com/khoryz666/ytcut/releases/latest")
        .set("User-Agent", "ytcut")
        .call()
        .context("failed to query github releases")?;
    let url = resp.get_url();
    let tag = url
        .rsplit('/')
        .next()
        .context("no tag found in release redirect URL")?
        .trim_start_matches('v');
    let version = Version::parse(tag).with_context(|| format!("invalid semver tag '{tag}'"))?;
    Ok(version)
}

/// Run background update check (non-blocking, swallows all network errors).
/// `autoupdate` is the config value: "off" disables, "notify" only notifies,
/// anything else (auto) installs patches silently and notifies on minor/major.
pub fn check_background(autoupdate: Option<&str>) {
    if env::var("YTCUT_DISABLE_AUTOUPDATE").is_ok() {
        return;
    }
    if autoupdate == Some("off") {
        return;
    }
    let notify_only = autoupdate == Some("notify");
    thread::spawn(move || {
        let _ = run_background_check(notify_only);
    });
}

fn run_background_check(notify_only: bool) -> Result<()> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let latest = fetch_latest_version()?;
    match classify_gap(&current, &latest) {
        ReleaseGap::Patch if !notify_only => {
            // Silently install patch update
            if let Ok(asset) = current_asset_name().context("unsupported platform") {
                let temp_dir =
                    std::env::temp_dir().join(format!("ytcut-update-{}", std::process::id()));
                let _ = std::fs::create_dir_all(&temp_dir);
                if let Ok(()) = install_version(&latest, asset, &temp_dir) {
                    if let Ok(mut notice) = UPDATE_NOTICE.lock() {
                        *notice = Some(format!("Updated to v{latest} — restart to use"));
                    }
                }
                let _ = std::fs::remove_dir_all(&temp_dir);
            }
        }
        ReleaseGap::Minor | ReleaseGap::Major | ReleaseGap::Patch => {
            if let Ok(mut notice) = UPDATE_NOTICE.lock() {
                *notice = Some(format!("v{latest} is available — run `ytcut update`"));
            }
        }
        ReleaseGap::None => {}
    }
    Ok(())
}

/// Manual update to a specific version or latest version.
pub fn manual_update(version_arg: Option<&str>) -> Result<()> {
    let target_version = match version_arg {
        Some(v) => Version::parse(v.trim_start_matches('v'))
            .with_context(|| format!("invalid version format '{v}'"))?,
        None => {
            println!("checking for latest version...");
            fetch_latest_version()?
        }
    };

    let asset =
        current_asset_name().context("no prebuilt binary asset available for this platform")?;

    let temp_dir = std::env::temp_dir().join(format!("ytcut-manual-update-{}", std::process::id()));
    std::fs::create_dir_all(&temp_dir)?;

    println!("downloading v{target_version} ({asset})...");
    let res = install_version(&target_version, asset, &temp_dir);
    let _ = std::fs::remove_dir_all(&temp_dir);

    res?;
    println!("updated to v{target_version} — restart ytcut to use it");
    Ok(())
}

fn install_version(version: &Version, asset: &str, temp_dir: &Path) -> Result<()> {
    let url = format!("https://github.com/khoryz666/ytcut/releases/download/v{version}/{asset}");
    let archive_path = temp_dir.join(asset);

    let resp = ureq::get(&url)
        .set("User-Agent", "ytcut")
        .call()
        .with_context(|| format!("failed to download release asset from {url}"))?;
    let mut reader = resp.into_reader();
    let mut writer = File::create(&archive_path)?;
    io::copy(&mut reader, &mut writer)?;

    let extracted_bin = temp_dir.join(format!("ytcut-new{}", env::consts::EXE_SUFFIX));

    if asset.ends_with(".tar.gz") {
        let gz = flate2::read::GzDecoder::new(BufReader::new(File::open(&archive_path)?));
        let mut tar = tar::Archive::new(gz);
        let mut found = false;
        for entry in tar.entries()? {
            let mut entry = entry?;
            if entry.path()?.file_name() == Some(std::ffi::OsStr::new("ytcut")) {
                entry.unpack(&extracted_bin)?;
                found = true;
                break;
            }
        }
        ensure!(found, "ytcut binary not found inside tar archive");
    } else if asset.ends_with(".zip") {
        let mut zip = zip::ZipArchive::new(File::open(&archive_path)?)?;
        let exe_name = format!("ytcut{}", env::consts::EXE_SUFFIX);
        let target_name = if zip.file_names().any(|n| n == exe_name) {
            Some(exe_name.clone())
        } else if zip.file_names().any(|n| n == "ytcut.exe") {
            Some("ytcut.exe".to_string())
        } else {
            None
        };
        let target_name =
            target_name.with_context(|| format!("{exe_name} not found in zip archive"))?;
        let mut zip_file = zip.by_name(&target_name)?;
        let mut dest = File::create(&extracted_bin)?;
        io::copy(&mut zip_file, &mut dest)?;
    } else {
        bail!("unsupported archive format '{asset}'");
    }

    #[cfg(unix)]
    crate::tools::make_executable(&extracted_bin)?;

    self_replace::self_replace(&extracted_bin).context("failed to replace current binary")?;

    Ok(())
}

/// Explicit "update now" (from `/update`): fetch latest, install, report via `UPDATE_NOTICE`.
pub fn update_now() {
    thread::spawn(|| {
        let result = (|| -> Result<()> {
            let latest = fetch_latest_version()?;
            let asset = current_asset_name().context("unsupported platform")?;
            let temp_dir =
                std::env::temp_dir().join(format!("ytcut-update-{}", std::process::id()));
            std::fs::create_dir_all(&temp_dir)?;
            let res = install_version(&latest, asset, &temp_dir);
            let _ = std::fs::remove_dir_all(&temp_dir);
            res?;
            set_notice(format!("Updated to v{latest} — restart to use"));
            Ok(())
        })();
        if let Err(e) = result {
            set_notice(format!("Update failed: {e:#}"));
        }
    });
}

fn set_notice(msg: String) {
    if let Ok(mut notice) = UPDATE_NOTICE.lock() {
        *notice = Some(msg);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_gap() {
        let cur = Version::parse("0.3.0").unwrap();
        assert_eq!(
            classify_gap(&cur, &Version::parse("0.3.0").unwrap()),
            ReleaseGap::None
        );
        assert_eq!(
            classify_gap(&cur, &Version::parse("0.2.9").unwrap()),
            ReleaseGap::None
        );
        assert_eq!(
            classify_gap(&cur, &Version::parse("0.3.1").unwrap()),
            ReleaseGap::Patch
        );
        assert_eq!(
            classify_gap(&cur, &Version::parse("0.4.0").unwrap()),
            ReleaseGap::Minor
        );
        assert_eq!(
            classify_gap(&cur, &Version::parse("1.0.0").unwrap()),
            ReleaseGap::Major
        );
    }

    #[test]
    fn test_asset_name_mapping() {
        assert_eq!(
            asset_name("linux", "x86_64"),
            Some("ytcut-x86_64-unknown-linux-gnu.tar.gz")
        );
        assert_eq!(
            asset_name("windows", "x86_64"),
            Some("ytcut-x86_64-pc-windows-msvc.zip")
        );
        assert_eq!(asset_name("macos", "x86_64"), None);
        assert_eq!(asset_name("linux", "arm"), None);
    }
}
