//! A launcher that starts ytcut, made on first run and refreshed if the binary moves.

/// Make sure there is a shortcut that starts this binary. Best effort: a failure is silent,
/// because a shortcut is a convenience, not a requirement.
pub fn ensure() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    #[cfg(target_os = "linux")]
    linux::ensure(&exe);
    #[cfg(target_os = "windows")]
    windows::ensure(&exe);
}

#[cfg(target_os = "linux")]
mod linux {
    use std::fs;
    use std::path::Path;

    /// A `.desktop` file in the app menu, the standard launcher on Linux.
    pub fn ensure(exe: &Path) {
        let Some(dir) = dirs::data_dir().map(|d| d.join("applications")) else {
            return;
        };
        let Some(exe) = exe.to_str() else {
            return;
        };
        let content = format!(
            "[Desktop Entry]\n\
             Type=Application\n\
             Name=ytcut\n\
             Comment=Download YouTube songs and play them on repeat\n\
             Exec={}\n\
             Terminal=false\n\
             StartupWMClass=ytcut\n",
            quote(exe)
        );
        let file = dir.join("ytcut.desktop");
        if fs::read_to_string(&file).ok().as_deref() == Some(content.as_str()) {
            return;
        }
        if fs::create_dir_all(&dir).is_ok() {
            let _ = fs::write(&file, content);
        }
    }

    /// One Exec argument, quoted per the Desktop Entry spec.
    fn quote(path: &str) -> String {
        let mut out = String::from("\"");
        for c in path.chars() {
            if matches!(c, '"' | '`' | '$' | '\\') {
                out.push('\\');
            }
            out.push(c);
        }
        out.push('"');
        out
    }

    #[cfg(test)]
    mod tests {
        use super::quote;

        #[test]
        fn quote_escapes_specials() {
            assert_eq!(quote("/home/me/ytcut"), "\"/home/me/ytcut\"");
            assert_eq!(quote("/a b/$x"), "\"/a b/\\$x\"");
        }
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Shortcuts on the desktop and in the Start menu, made with the shell's shortcut object.
    pub fn ensure(exe: &Path) {
        let Some(target) = exe.to_str() else {
            return;
        };
        let mut shortcuts: Vec<PathBuf> = Vec::new();
        if let Some(dir) = dirs::desktop_dir() {
            shortcuts.push(dir.join("ytcut.lnk"));
        }
        if let Some(dir) = dirs::data_dir().map(|d| {
            d.join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs")
        }) {
            shortcuts.push(dir.join("ytcut.lnk"));
        }
        for shortcut in shortcuts {
            let Some(shortcut) = shortcut.to_str() else {
                continue;
            };
            let script = format!(
                "$s=(New-Object -ComObject WScript.Shell).CreateShortcut('{}');$s.TargetPath='{}';$s.Save()",
                shortcut.replace('\'', "''"),
                target.replace('\'', "''"),
            );
            let _ = Command::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .spawn();
        }
    }
}
