# ytcut

Save songs from YouTube, keep the part you like, and play them on repeat.

ytcut runs on your own computer. It opens in its own window and saves songs to your Music folder.

## Install

Download the file for your system from the [latest release](https://github.com/khoryz666/ytcut/releases/latest), unpack it, and run it:

- **Linux (x86-64):** `ytcut-x86_64-unknown-linux-musl.tar.gz`
- **Windows (x86-64):** `ytcut-x86_64-pc-windows-msvc.zip`

The first run needs internet. It downloads yt-dlp, ffmpeg and Deno, about 200 MB on Linux, and can take a few minutes. They are checked against the checksums their projects publish, and kept in `~/.local/share/ytcut` (Linux) or `%LOCALAPPDATA%\ytcut` (Windows).

These tools are glibc binaries. Alpine (musl) can't run them. On NixOS, set `programs.nix-ld.enable = true;` in your configuration so the dynamic linker is available.

## Updates

There is nothing to do. On each start, yt-dlp and ytcut update themselves in the background. A new ytcut takes effect the next time you open it.

## Use it

1. Paste a YouTube link. The video's title and picture appear.
2. Type a file name, or leave it empty to use the video title.
3. To keep only part of the song, drag the **Start** and **End** sliders.
4. Click **Download**.

Existing files are never overwritten. A part saved without a name gets its times in the file name, like `Song (10-40).mp4`.

In the library, click a song to play it, ➕ to add it to the playlist, ✎ to rename it and 🗑 to delete it. In the playlist, use ↑ and ↓ to reorder and ✕ to remove.

| Player button | What it does |
|---|---|
| ⏮ ⏸ ⏭ | Previous, pause or play, next |
| 🔂 | Repeat the current song |
| 🔀 | Shuffle |
| 🎬 | Show or hide the video |
| ✕ | Stop and clear the playlist |

## If YouTube asks you to sign in

Click **Sign in to YouTube** in the message, sign in to Google in the window that opens, and try again. The login stays in ytcut's own browser profile, not your normal one.

This needs a Chromium-based browser: Chrome, Chromium, Brave, Edge or Vivaldi. Without one, ytcut opens in your default browser and cannot sign in. Flatpak and Snap browsers are not found.

## Good to know

- ytcut stops by itself a few seconds after you close its window. A running download finishes first.
- Only one copy runs at a time. Opening it again opens another window.
- To use another folder, run `ytcut FOLDER`.
- To pick a browser, set `YTCUT_BROWSER`. To use another port, set `PORT`.

## Develop

You need [Rust](https://rustup.rs) (stable) and [Node.js](https://nodejs.org) 22+ with npm. `nix develop` provides both if you use Nix; otherwise install them directly.

To run the app the way a user would:

```sh
cargo run    # builds the page, then opens ytcut in a browser window
```

`cargo run` builds and type-checks the page first, installing its npm packages the first time. The first download fetches yt-dlp, ffmpeg and Deno, just like a first install.

To work on the page with live reload, run the server and Vite side by side:

```sh
cargo run -- --no-window ~/Music    # terminal 1: the server, no window
npm --prefix src/web run dev        # terminal 2: Vite, then open http://localhost:5173
```

`~/Music` is the folder videos are saved to; pass any folder. Vite proxies the API and media paths to the server on port 8765.

Run these before a commit:

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

Debug builds do not update themselves.

| Path               | What it holds                                                              |
| ------------------ | -------------------------------------------------------------------------- |
| `src/main.rs`      | Startup, single-instance lock, idle shutdown                               |
| `src/app.rs`       | Shared state, and the check that only ytcut's own page may call the server |
| `src/api.rs`       | Routes, the embedded page, and video streaming                             |
| `src/browser.rs`   | Finds the browser and opens the window                                     |
| `src/library.rs`   | The video folder: list, rename, delete, safe file names                    |
| `src/downloads.rs` | yt-dlp: look up, download, progress                                        |
| `src/tools.rs`     | Downloads the tools, and updates yt-dlp and ytcut                          |
| `src/web/`         | Svelte 5 page, Pico CSS, and the Vite config                               |

## Release

Set the new version in `Cargo.toml`, then push a tag with the same version, like `v0.2.0`. [`release.yml`](.github/workflows/release.yml) builds both binaries and publishes them, and running copies pick them up on their next start.
