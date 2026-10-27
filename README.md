# ytcut

Save songs from YouTube, keep the part you like, and play them on repeat.

ytcut runs on your own computer. It opens in its own browser window and saves songs to `~/Music`.

## Setup

This works on any Linux desktop: GNOME, KDE Plasma, Cinnamon, XFCE, MATE and others.

### 1. Install Nix

Nix gives ytcut everything it needs: yt-dlp, ffmpeg, Rust and TypeScript. Nothing else is installed on your system.

Skip this step if `nix --version` already works.

```sh
sh <(curl -L https://nixos.org/nix/install) --daemon
```

Close the terminal and open a new one.

### 2. Get a browser

ytcut opens in a Chromium-based browser. Any of these works:

- Google Chrome
- Chromium
- Brave
- Microsoft Edge
- Vivaldi

Install one from your distribution's software center if you have none. Flatpak and Snap browsers are not found. Use a normal package instead.

Without one of these browsers, ytcut opens in your default browser. Downloads may then fail when YouTube asks you to sign in.

### 3. Get ytcut

```sh
git clone <this repository> ~/ytcut
cd ~/ytcut
```

### 4. Add it to your desktop

```sh
nix run .#install
```

This adds **ytcut** to your app menu and puts a shortcut on your Desktop. The first run takes a few minutes while Nix downloads what ytcut needs.

If your desktop asks whether to trust or allow the shortcut, say yes. On GNOME, right-click the icon and choose **Allow Launching**.

### 5. Open it

Click **ytcut** in your app menu, or double-click it on the Desktop.

You can also run it from the ytcut folder:

```sh
nix run
```

## Download a song

1. Paste a YouTube link. The video's title and picture appear.
2. Type a file name, or leave it empty to use the video title.
3. To keep only part of the song, drag the two handles of the slider. Arrow keys move them by 1 second.
4. Click **Download**.

Songs are saved as MP4 in `~/Music`. Existing files are never overwritten.

## If YouTube asks you to sign in

Sometimes YouTube stops downloads with "Sign in to confirm you're not a bot".

1. Click **Sign in to YouTube** in the message.
2. Sign in to your Google account in the window that opens.
3. Try the download again.

You only do this once. The login stays in ytcut's own browser profile, not your normal one.

## Library

- Click a song to play it on repeat.
- Click ➕ to add a song to the playlist. Or tick several songs and click **Play selected**.
- Click ⋮ to rename or delete a song.
- Use the search box to find a song.

## Player

The player sits at the bottom of the window. It plays audio only. Music keeps playing when you switch pages.

| Button | What it does |
|---|---|
| ⏮ ⏸ ⏭ | Previous, pause or play, next |
| 🔂 | Repeat the current song |
| 🔀 | Shuffle |
| ☰ | Show the playlist. Drag songs to reorder, ✕ to remove |
| 🎬 | Show or hide the video |
| ✕ | Stop |

The playlist is not saved.

## Good to know

- ytcut stops by itself a few seconds after you close the last window. A running download finishes first.
- Only one copy of ytcut runs at a time.
- The window uses its own browser profile in `~/.local/share/ytcut`. It does not touch your normal browser.
- To pick a browser, set `YTCUT_BROWSER`, for example `YTCUT_BROWSER=brave nix run`.
- To use another port, set `PORT`.

## Troubleshooting

**Nothing happens when I click the shortcut.**
Run `nix run` in the ytcut folder to see the error.

**The Desktop shortcut opens as a text file.**
Your desktop has not trusted it yet. Right-click it and choose **Allow Launching** (GNOME) or **Trust** (Cinnamon, XFCE). Or use the app menu entry.

**"Sign in to confirm you're not a bot".**
See [If YouTube asks you to sign in](#if-youtube-asks-you-to-sign-in).

**A song won't play.**
Click ✕ and play it again. If it still fails, the file may be damaged. Download it again.

## Develop

```sh
direnv allow    # or: nix develop
cargo run -- ~/Music
```

| Path | What it holds |
|---|---|
| `src/main.rs` | Startup, single-instance lock, idle shutdown |
| `src/app.rs` | Shared state, and the check that only ytcut's own page may call the server |
| `src/routes.rs` | Which handler answers which URL |
| `src/http.rs` | Reading requests and sending replies, including video seeking |
| `src/library.rs` | The music folder: list, rename, delete, safe file names |
| `src/downloads.rs` | yt-dlp: look up, download, progress |
| `src/json.rs` | JSON output |
| `src/web/` | The page: HTML, CSS, logo |
| `src/web/ts/` | The page's TypeScript, one module per part of the page |
| `build.rs` | Compiles the TypeScript into the binary |

The server uses only the Rust standard library. The TypeScript has no packages.

Checks before a commit:

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

After changing the code, close the ytcut window and wait a few seconds. The next launch uses the new version.

Run `git add` on new files first. Nix only sees files that git tracks.

Video and audio files are ignored by git.
