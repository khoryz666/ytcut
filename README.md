# ytcut

Download songs from YouTube as audio, keep the part you like, and play them from your terminal.

`ytcut` is a terminal music player. It runs in your terminal (no browser, no GUI), saves audio to your Music folder, and keeps itself and its tools up to date.

## Install

Download the file for your system from the [latest release](https://github.com/khoryz666/ytcut/releases/latest), unpack it, and run it:

- **Linux (x86-64):** `ytcut-x86_64-unknown-linux-gnu.tar.gz`
- **Windows (x86-64):** `ytcut-x86_64-pc-windows-msvc.zip`

Or with a package manager:

```sh
# cargo
cargo install --git https://github.com/khoryz666/ytcut

# Nix
nix profile install github:khoryz666/ytcut
```

The first run needs internet. It downloads yt-dlp (about 200 MB on Linux) — and ffmpeg and Deno only if you don't already have them — checks them against the checksums their projects publish, and keeps them in `~/.local/share/ytcut` (Linux) or `%LOCALAPPDATA%\ytcut` (Windows). These are glibc binaries, so Alpine (musl) can't run them; on NixOS set `programs.nix-ld.enable = true;`.

Linux needs ALSA (`libasound2`) and PulseAudio/PipeWire installed — both are present on nearly every desktop. On Windows, sound uses the system output (WASAPI) automatically; the interface looks best in Windows Terminal.

## Use it

Run `ytcut` with no arguments to open the terminal interface. Your songs are read from your Music folder automatically; change the folder later with `/settings` or `/dir <path>`.

| Key | Action |
|---|---|
| `↑`/`↓` or `j`/`k` | move selection |
| `Tab` | switch pane (Library ↔ Queue) |
| `Enter` | play the selected song |
| `space` / `n` / `p` | play/pause, next, previous |
| `s` / `r` | shuffle, repeat-one |
| `/` | command palette |
| `?` | full help |
| `d` | delete the selected song |
| `o` | open YouTube in your browser (sign in) |
| `q` / `Esc` | back / quit |

The command palette (`/`) covers everything else: `/add <url>`, `/background` (alias `/bg`), `/stop`, `/search <query>`, `/refresh`, `/settings`, `/dir <path>`, `/update`, `/help`, `/quit`.

To keep only part of a song, type the **Start** and **End** times (like `1:30`) in the Add dialog (`/add`) before downloading. Songs are saved as audio (`.m4a`); existing files are never overwritten.

### Quick commands (no interface)

```
ytcut add  <URL> [--name N] [--start 1:30] [--end 2:00]   # download audio
ytcut play [FILES...] [--shuffle] [--repeat] [--background]   # play files or the library; --background detaches
ytcut list [DIR]                                              # list the library
ytcut info <URL>                                              # title + duration
ytcut rm <NAME>                                               # delete
ytcut stop                                                    # stop the background player
ytcut update [VERSION]                                        # update yt-dlp + self
```

The library folder is set in `~/.config/ytcut/config.toml` (Linux) / `%APPDATA%\ytcut\config.toml` (Windows), or with `/settings` in the interface, or per-command with `--out DIR`. Sound follows your system output through PulseAudio/PipeWire (Linux) or WASAPI (Windows), so a connected Bluetooth earphone or the default speakers just work; the settings screen also covers the sign-in browser, auto-update and volume.

## Updates

Nothing to do. On start, ytcut checks for a new version and installs patch releases silently; for minor/major releases it tells you to run `ytcut update`. yt-dlp updates itself before downloads. Set `YTCUT_DISABLE_AUTOUPDATE=1` (or `autoupdate = "off"` in config.toml) to turn this off.

Rust dependencies update themselves monthly: [Dependabot](.github/dependabot.yml) opens pull requests for the crates and GitHub Actions, CI verifies them, and passing updates are merged automatically. The pinned Nix toolchain is refreshed the same way by [`flake-update.yml`](.github/workflows/flake-update.yml). ffmpeg and Deno are not part of that — ytcut uses the copies already on your system, and any it downloads are refreshed every six months. yt-dlp keeps itself current because YouTube breaks old versions.

## If YouTube asks you to sign in

Press `o` in the interface to open YouTube in your normal browser, sign in there, then try again. ytcut reads that browser's cookies (`--cookies-from-browser`), so it needs a Chromium-based browser — Chrome, Chromium, Edge, Brave or Vivaldi. Pick one with `/settings`, or set `YTCUT_BROWSER`.

## Uninstall

Delete the `ytcut` binary and, if you want, the tools and settings:

- **Linux:** `~/.local/share/ytcut/` and `~/.config/ytcut/`
- **Windows:** `%LOCALAPPDATA%\ytcut\` and `%APPDATA%\ytcut\`

Nothing else is written.

## Develop

You need [Rust](https://rustup.rs) (stable) and the ALSA dev headers (`libasound2-dev` on Debian/Ubuntu). `nix develop` provides the full toolchain and those headers.

```sh
cargo run -- ~/Music    # run the interface against a folder
cargo run -- add 'https://www.youtube.com/watch?v=...'
```

Before a commit:

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

| Path | What it holds |
|---|---|
| `src/main.rs` | CLI subcommands, single-instance lock |
| `src/ui/` | the ratatui interface — `mod.rs` state & keys, `render.rs` drawing, plus `theme.rs`, `palette.rs`, `widgets.rs`, `text.rs` helpers |
| `src/player.rs` | rodio playback: queue, seek, shuffle, repeat, shared key handling |
| `src/downloads.rs` | yt-dlp: lookup, audio download, progress |
| `src/library.rs` | list / delete / safe names |
| `src/tools.rs` | download + verify yt-dlp, ffmpeg, deno; update yt-dlp |
| `src/update.rs` | self-update (patch auto, minor/major notify) |
| `src/config.rs` | settings in `config.toml` |

## Release

Set the version in `Cargo.toml` and `flake.nix`, then push a tag with the same version, like `v0.3.0`. [`release.yml`](.github/workflows/release.yml) builds both binaries and publishes them; running copies pick them up on their next start.

Dependencies update themselves monthly via [Dependabot](.github/dependabot.yml): it opens pull requests for the Rust crates and GitHub Actions, [CI](.github/workflows/ci.yml) checks them, and passing updates are merged automatically. [`auto-release.yml`](.github/workflows/auto-release.yml) then tags the changes and ships a new release each month, so no manual step is needed. To ship a feature yourself, bump the version in `Cargo.toml` and `flake.nix` and push a tag.
