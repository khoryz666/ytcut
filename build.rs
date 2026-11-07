//! Build the Svelte page that gets embedded in the binary.

use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/web/src");
    println!("cargo:rerun-if-changed=src/web/public");
    println!("cargo:rerun-if-changed=src/web/index.html");
    println!("cargo:rerun-if-changed=src/web/package-lock.json");
    println!("cargo:rerun-if-changed=src/web/vite.config.ts");

    let web = Path::new("src/web");
    if !web.join("node_modules").is_dir() {
        npm(&["ci"], web);
    }
    npm(&["run", "build"], web);
}

fn npm(args: &[&str], dir: &Path) {
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let status = Command::new(npm)
        .args(args)
        .current_dir(dir)
        .status()
        .unwrap_or_else(|e| panic!("could not run npm; enter `nix develop` first: {e}"));
    assert!(status.success(), "the web build failed");
}
