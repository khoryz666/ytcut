//! Build the Svelte page, or use the build Nix already prepared.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=src/web/src");
    println!("cargo:rerun-if-changed=src/web/index.html");
    println!("cargo:rerun-if-changed=src/web/package.json");
    println!("cargo:rerun-if-changed=src/web/vite.config.ts");

    let dist = match env::var_os("YTCUT_WEB_DIST") {
        Some(dir) => PathBuf::from(dir),
        None => build_web(),
    };
    assert!(
        dist.join("index.html").is_file(),
        "the web build is missing: {}",
        dist.display()
    );
    println!("cargo:rustc-env=YTCUT_WEB_DIST={}", dist.display());
}

/// Build the page locally. Nix sets YTCUT_WEB_DIST, so this only runs in a dev checkout.
fn build_web() -> PathBuf {
    let web = Path::new("src/web");
    if !web.join("node_modules").is_dir() {
        run(Command::new("npm").args(["ci", "--prefix"]).arg(web));
    }
    run(Command::new("npm")
        .args(["run", "build", "--prefix"])
        .arg(web));
    web.join("dist")
}

fn run(command: &mut Command) {
    let status = command.status().unwrap_or_else(|e| {
        panic!("could not run npm; enter `nix develop` first: {e}");
    });
    assert!(status.success(), "the web build failed");
}
