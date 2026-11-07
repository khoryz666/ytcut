# ytcut

Save songs from YouTube, keep the part you like, and play them on repeat.

ytcut runs on your own computer. It opens in its own browser window and saves songs to `~/Music` (Linux) or your `Music` folder (Windows).

## Download

Each release is a single self-contained binary that carries its own yt-dlp and ffmpeg, so it needs no other program installed:

- **Linux** — `ytcut-linux-x86_64` (statically linked, runs on any distribution)
- **Windows** — `ytcut-windows-x86_64.zip` (64-bit, no Visual C++ runtime needed)

Download the one for your system, make it runnable if needed, and start it:

```sh
# Linux
chmod +x ytcut
./ytcut ~/Music
```

```powershell
# Windows (PowerShell)
.\ytcut.exe "$env:USERPROFILE\Music"
```

Then open <http://127.0.0.1:8765> in any browser. The first start unpacks yt-dlp and ffmpeg into `~/.cache/ytcut` (Linux) or `%LOCALAPPDATA%\ytcut` (Windows).

## Build from source (Nix)

This works on any Linux desktop: GNOME, KDE Plasma, Cinnamon, XFCE, MATE and others. Nix gives ytcut everything it needs to build: yt-dlp, ffmpeg, Rust, Node and the web build tools.

### 1. Install Nix

Skip this step if `nix --version` already works.

```sh
sh <(curl -L https://nixos.org/nix/install) --daemon
```

Close the terminal and open a new one.

### 2. Get a browser

ytcut opens in a Chromium-based browser. Any of these works: Google Chrome, Chromium, Brave, Microsoft Edge, Vivaldi. Install one from your distribution's software center if you have none. Flatpak and Snap browsers are not found.

Without one of these browsers, ytcut opens in your default browser. Downloads may then fail when YouTube asks you to sign in.

### 3. Get ytcut

```sh
git clone https://github.com/khoryz666/ytcut.git ~/ytcut
cd ~/ytcut
```

### 4. Add a Desktop shortcut

```sh
nix run .#install
```

This creates one file, `~/Desktop/ytcut.desktop`. To remove the shortcut, delete that file. If your desktop asks whether to trust it, say yes (on GNOME, right-click the icon and choose **Allow Launching**).

### 5. Open it

Double-click **ytcut** on the Desktop, or run it from the folder:

```sh
nix run
```

## Download a song

1. Paste a YouTube link. The video's title and picture appear.
2. Type a file name, or leave it empty to use the video title.
3. To keep only part of the song, drag the **Start** and **End** sliders.
4. Click **Download**.

Songs are saved as MP4 in `~/Music`. Existing files are never overwritten.

## Library and playlist

Add, Library and Playlist are all on one page. On a wide window they sit side by side; on a phone they stack.

- **Play all** plays your whole library, or just the current search results.
- Click a song to play it. If a playlist is playing, the song joins it right after the current one.
- Click ➕ to add a song to the end of the playlist.
- Click ✎ to rename a song, 🗑 to delete it.
- Click ↑ or ↓ to move a song in the playlist, ✕ to remove it.
- Use the search box to find a song.

## Player

The player sits at the bottom of the window. It plays audio only, so your graphics card is not used. Music keeps playing while you use the page.

| Button | What it does |
|---|---|
| ⏮ ⏸ ⏭ | Previous, pause or play, next |
| 🔂 | Repeat the current song |
| 🔀 | Shuffle |
| 🎬 | Show or hide the video |
| ✕ | Stop and clear the playlist |

## If YouTube asks you to sign in

Sometimes YouTube stops downloads with "Sign in to confirm you're not a bot".

1. Click **Sign in to YouTube** in the message.
2. Sign in to your Google account in the window that opens.
3. Try the download again.

You only do this once. The login stays in ytcut's own browser profile, not your normal one.

## Good to know

- ytcut stops by itself a few seconds after you close the last window. A running download finishes first.
- Only one copy of ytcut runs at a time.
- The window uses its own browser profile in `~/.local/share/ytcut`. It does not touch your normal browser.
- To pick a browser, set `YTCUT_BROWSER`, for example `YTCUT_BROWSER=brave nix run`.
- To use another port, set `PORT`.

## Troubleshooting

**Nothing happens when I click the shortcut.**
Run `nix run` in the ytcut folder to see the error.

**I moved the ytcut folder.**
The shortcut points to the old place. Run `nix run .#install` again in the new folder.

**The Desktop shortcut opens as a text file.**
Your desktop has not trusted it yet. Right-click it and choose **Allow Launching** (GNOME) or **Trust** (Cinnamon, XFCE).

**"Sign in to confirm you're not a bot".**
See [If YouTube asks you to sign in](#if-youtube-asks-you-to-sign-in).

**A song won't play.**
ytcut retries a song twice by itself before it says "Can't play this file". If you see that message, the file may be damaged. Download it again.

## Develop

```sh
direnv allow    # or: nix develop
cargo run -- ~/Music
```

`cargo run` builds the Svelte page first. For a faster page loop, run the API and Vite together:

```sh
nix develop
cargo run -- ~/Music            # terminal 1
npm --prefix src/web run dev    # terminal 2, then open http://localhost:5173
```

| Path | What it holds |
|---|---|
| `src/main.rs` | Startup, single-instance lock, idle shutdown |
| `src/app.rs` | Shared state, and the check that only ytcut's own page may call the server |
| `src/api.rs` | Axum routes, the embedded page, and video streaming |
| `src/library.rs` | The music folder: list, rename, delete, safe file names |
| `src/downloads.rs` | yt-dlp: look up, download, progress |
| `src/tools.rs` | Unpacks the embedded yt-dlp and ffmpeg on first run |
| `build.rs` | Builds the page, and compresses the bundled tools into the binary |
| `tools.json` | The pinned yt-dlp and ffmpeg release files, one per platform |
| `scripts/` | Refreshes `tools.json` and fetches tools for the Windows build |
| `src/web/` | Svelte 5 page, Pico CSS, and the Vite config |

The backend uses axum, tokio, serde and tower-http. The page uses Svelte 5, Vite and Pico CSS. The `bundle` Cargo feature embeds yt-dlp and ffmpeg; without it, a dev build finds them on `PATH`.

Checks before a commit:

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
npm --prefix src/web run check
```

After changing the code, close the ytcut window and wait a few seconds. The next launch uses the new version. Run `git add` on new files first — Nix only sees files that git tracks. Video and audio files are ignored by git.

## Releases

Pushing a `v*` tag runs [`.github/workflows/release.yml`](.github/workflows/release.yml), which builds the Linux binary with Nix and the Windows binary with cargo, then attaches both to the release. Running the workflow by hand only uploads build artifacts.

## Updates

[`.github/workflows/update.yml`](.github/workflows/update.yml) runs twice a year, in January and July. It updates `flake.lock`, the pinned yt-dlp and ffmpeg files in `tools.json`, `Cargo.lock` and `package-lock.json`, then pushes only if `nix build` still succeeds. Dependabot keeps the GitHub Actions themselves current and merges those PRs once CI passes.
