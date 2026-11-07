//! Background daemon playback manager for ytcut.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process;

use anyhow::{Context, Result};

pub fn data_dir() -> Result<PathBuf> {
    let data = dirs::data_local_dir()
        .context("no data folder on this system")?
        .join("ytcut");
    fs::create_dir_all(&data).with_context(|| format!("could not create {}", data.display()))?;
    Ok(data)
}

pub fn pid_file(data: &Path) -> PathBuf {
    data.join("ytcut.pid")
}

#[allow(dead_code)]
pub fn is_running() -> bool {
    let Ok(data) = data_dir() else {
        return false;
    };
    let pfile = pid_file(&data);
    if !pfile.exists() {
        return false;
    }

    #[cfg(unix)]
    {
        let Ok(pid_str) = fs::read_to_string(&pfile) else {
            return false;
        };
        let Ok(pid) = pid_str.trim().parse::<i32>() else {
            return false;
        };
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[cfg(not(unix))]
    true
}

/// Signal the background player to stop. Returns true if a running process was terminated.
pub fn stop() -> Result<bool> {
    let data = data_dir()?;
    let pfile = pid_file(&data);
    if !pfile.exists() {
        return Ok(false);
    }
    let pid_str = fs::read_to_string(&pfile).with_context(|| "could not read pid file")?;
    let _ = fs::remove_file(&pfile);
    let Ok(pid) = pid_str.trim().parse::<i32>() else {
        return Ok(false);
    };
    Ok(kill_background(pid))
}

#[cfg(unix)]
pub fn kill_background(pid: i32) -> bool {
    unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
}

#[cfg(windows)]
pub fn kill_background(pid: i32) -> bool {
    std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/F"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(unix)]
pub fn spawn_detached(exe: &Path, args: &[String]) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    unsafe {
        std::process::Command::new(exe)
            .args(args)
            .env("YTCUT_DAEMON", "1")
            .stdin(process::Stdio::null())
            .stdout(process::Stdio::null())
            .stderr(process::Stdio::null())
            .pre_exec(|| {
                if libc::setsid() == -1 {
                    Err(io::Error::last_os_error())
                } else {
                    Ok(())
                }
            })
            .spawn()?;
    }
    Ok(())
}

#[cfg(windows)]
pub fn spawn_detached(exe: &Path, args: &[String]) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    std::process::Command::new(exe)
        .args(args)
        .env("YTCUT_DAEMON", "1")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::null())
        .stderr(process::Stdio::null())
        .creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP)
        .spawn()?;
    Ok(())
}

/// Spawn the background playback daemon for the given tracks.
pub fn spawn(
    lib_dir: &Path,
    files: &[PathBuf],
    shuffle: bool,
    repeat: bool,
    start_secs: Option<u64>,
) -> Result<()> {
    // Stop any previously running background player first
    let _ = stop();

    let exe = std::env::current_exe().context("could not find this executable")?;
    let mut args = vec![
        "--out".to_string(),
        lib_dir.display().to_string(),
        "play".to_string(),
    ];
    for f in files {
        args.push(f.display().to_string());
    }
    if shuffle {
        args.push("--shuffle".into());
    }
    if repeat {
        args.push("--repeat".into());
    }
    if let Some(s) = start_secs {
        args.push("--start".into());
        args.push(s.to_string());
    }

    spawn_detached(&exe, &args).with_context(|| "failed to spawn detached background player")?;
    Ok(())
}
