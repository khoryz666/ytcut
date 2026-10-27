//! Compile the page's TypeScript into OUT_DIR/js, which src/routes.rs embeds.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo::rerun-if-changed=src/web/ts");
    println!("cargo::rerun-if-changed=src/web/tsconfig.json");
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("js");
    let status = Command::new("tsc")
        .args(["--project", "src/web/tsconfig.json", "--outDir"])
        .arg(&out)
        .status()
        .unwrap_or_else(|e| {
            panic!("could not run tsc (TypeScript); run inside `nix develop`: {e}")
        });
    assert!(status.success(), "TypeScript failed to compile");
}
