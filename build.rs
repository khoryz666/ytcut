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

    // Release builds carry their own yt-dlp and ffmpeg. Nix and the release
    // workflow point these at the raw binaries; compress them here so they take
    // less room in the executable, and name the cache folder after their bytes.
    if env::var_os("CARGO_FEATURE_BUNDLE").is_some() {
        let yt_dlp = env::var("YTCUT_YTDLP").expect("YTCUT_YTDLP must point at yt-dlp");
        let ffmpeg = env::var("YTCUT_FFMPEG").expect("YTCUT_FFMPEG must point at ffmpeg");
        let (yt_dlp_gz, yt_dlp_hash) = gzip(&yt_dlp);
        let (ffmpeg_gz, ffmpeg_hash) = gzip(&ffmpeg);
        println!("cargo:rustc-env=YTCUT_YTDLP={}", yt_dlp_gz.display());
        println!("cargo:rustc-env=YTCUT_FFMPEG={}", ffmpeg_gz.display());
        println!("cargo:rustc-env=YTCUT_TOOLS_ID={yt_dlp_hash:016x}{ffmpeg_hash:016x}");
    }
}

/// Compress a tool into OUT_DIR, and return the new path plus an FNV-1a hash of
/// its bytes so an upgraded tool gets a fresh cache folder.
fn gzip(source: &str) -> (PathBuf, u64) {
    use std::fs::File;
    use std::io::{BufReader, Read, Write};

    use flate2::write::GzEncoder;
    use flate2::Compression;

    let name = Path::new(source).file_name().expect("a file name");
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is set by Cargo"))
        .join(name)
        .with_extension("gz");
    let input = File::open(source).expect("the bundled tool exists");
    let mut encoder = GzEncoder::new(
        File::create(&out).expect("OUT_DIR is writable"),
        Compression::best(),
    );

    let mut reader = BufReader::new(input);
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let mut chunk = [0u8; 1 << 16];
    loop {
        let n = reader
            .read(&mut chunk)
            .expect("the bundled tool is readable");
        if n == 0 {
            break;
        }
        for byte in &chunk[..n] {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        encoder.write_all(&chunk[..n]).expect("OUT_DIR is writable");
    }
    encoder.finish().expect("the compressed tool is writable");
    (out, hash)
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
