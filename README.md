# ytcut

Download YouTube videos from a web page. You can cut a part and pick a file name.

## Run

```sh
nix run . -- [FOLDER]
```

Open http://127.0.0.1:8765.

Videos are saved as MP4 in `FOLDER`. If no folder is given, the current folder is used. To change the port, set `PORT`.

## Use

1. Paste the link.
2. Type a file name. Leave it empty to use the video title.
3. To cut, set Start and End, like `90` or `1:30`. Leave one empty to start at the beginning or stop at the end.
4. Click Download.

Cuts are exact, but they take longer. Existing files are never overwritten.

## Develop

```sh
direnv allow    # or: nix develop
cargo run -- ~/Music
```

Run `git add` on new files first. Nix only sees files that git tracks.

Video and audio files are ignored by git.
