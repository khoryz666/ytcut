# ytcut

Download YouTube videos and play them on repeat. It runs on your computer and opens in Chrome.

## Open

Double-click **ytcut** on the Desktop. It starts the app for `~/Music` and opens a Chrome window.

The window uses its own Chrome profile in `~/.local/share/ytcut/chrome`. It does not touch your normal Chrome.

The app stops a few seconds after you close the last window. If a download is still running, it finishes first.

Only one copy of the app can run at a time.

From a terminal:

```sh
nix run .#open
```

## Download page

1. Paste a YouTube link. The title appears below it.
2. Type a file name. Leave it empty to use the video title.
3. To keep only part of the video, drag the two handles of the slider. Arrow keys move them by 1 second. Leave them at the ends for the whole video.
4. Click Download.

Videos are saved as MP4 in `~/Music`. Existing files are never overwritten.

## If YouTube asks you to sign in

Sometimes YouTube blocks downloads with "Sign in to confirm you're not a bot".

1. Click **Sign in to YouTube** in the error message.
2. Sign in to your Google account in the window that opens.
3. Try the download again.

You only need to do this once. The login stays in ytcut's own Chrome profile, not your normal Chrome.

## Library page

- Click a name to play it on repeat.
- Click ➕ to add a song to the playlist. Or tick several songs and click **Play selected**.
- The playlist shows below the player. Click a song to play it, drag to reorder, and ✕ to remove. It is not saved.
- Use ⏮ ⏭ to skip, 🔂 to repeat the current song, and 🔀 to shuffle.
- Use the search box to find a video.
- **Rename** and **Delete** are on each row. Delete cannot be undone.

Music keeps playing when you switch to the Download page.

## Desktop shortcut

To make the shortcut again:

```sh
cat > ~/Desktop/ytcut.desktop <<'EOF'
[Desktop Entry]
Type=Application
Name=ytcut
Exec=/nix/var/nix/profiles/default/bin/nix run "/home/hongye/Music#open"
Icon=multimedia-video-player
Terminal=false
StartupWMClass=ytcut
EOF
chmod +x ~/Desktop/ytcut.desktop
gio set ~/Desktop/ytcut.desktop metadata::trusted true
```

## Develop

```sh
direnv allow    # or: nix develop
cargo run -- ~/Music
```

The server uses only the Rust standard library. The page is plain HTML and JavaScript inside the binary.

After changing the code, close the app window and wait a few seconds. The next launch uses the new version.

Run `git add` on new files first. Nix only sees files that git tracks.

Video and audio files are ignored by git.
