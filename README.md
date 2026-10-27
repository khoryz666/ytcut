# ytcut

Download YouTube videos and play them on repeat. It runs on your computer and opens in Chrome.

## Open

Double-click **ytcut** on the Desktop. It starts the app for `~/Music` and opens a Chrome window.

The window uses its own Chrome profile in `~/.local/share/ytcut/chrome`. It does not touch your normal Chrome.

The app keeps running after you close the window. Double-click again to reopen it.

From a terminal:

```sh
nix run .#open
```

## Download page

1. Paste a YouTube link. The title and length appear below it.
2. Type a file name. Leave it empty to use the video title.
3. To keep only part of the video, set Start and End, like `1:30`. Leave them empty for the whole video.
4. Click Download.

Videos are saved as MP4 in `~/Music`. Existing files are never overwritten.

## Library page

- Click a name to play it on repeat.
- To loop several songs, tick them and click **Play selected**.
- Use ⏮ ⏭ to skip, and **Shuffle** to play in random order.
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

After changing the code, stop the running app so the next launch uses the new version:

```sh
pkill -f ytcut-wrapped
```

Run `git add` on new files first. Nix only sees files that git tracks.

Video and audio files are ignored by git.
