#!/usr/bin/env python3
"""Download the pinned yt-dlp and ffmpeg, verify their SHA-256, and print
`KEY=value` lines for GitHub Actions (append them to $GITHUB_ENV).

    python3 scripts/fetch-tools.py --platform x86_64-linux --out tools

`--platform` is one of x86_64-linux, aarch64-linux, x86_64-windows.
"""

import argparse
import hashlib
import json
import pathlib
import tarfile
import urllib.request
import zipfile


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def download(url, destination):
    request = urllib.request.Request(url, headers={"User-Agent": "ytcut"})
    with urllib.request.urlopen(request) as response, open(destination, "wb") as out:
        while chunk := response.read(1 << 20):
            out.write(chunk)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--platform", required=True)
    parser.add_argument("--out", default="tools")
    args = parser.parse_args()

    tools = json.loads(pathlib.Path("tools.json").read_text())
    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    suffix = ".exe" if "windows" in args.platform else ""

    yt = tools["yt-dlp"]["assets"][args.platform]
    yt_path = out / f"yt-dlp{suffix}"
    print(f"downloading yt-dlp {yt['url']}")
    download(yt["url"], yt_path)
    if sha256(yt_path) != yt["sha256"]:
        raise SystemExit("yt-dlp SHA-256 mismatch")

    ff = tools["ffmpeg"]["assets"][args.platform]
    archive = out / "ffmpeg-archive"
    ff_path = out / f"ffmpeg{suffix}"
    print(f"downloading ffmpeg {ff['url']}")
    download(ff["url"], archive)
    if sha256(archive) != ff["sha256"]:
        raise SystemExit("ffmpeg SHA-256 mismatch")
    if ff["url"].endswith(".zip"):
        with zipfile.ZipFile(archive) as bundle:
            member = next(n for n in bundle.namelist() if n.endswith(f"/bin/ffmpeg{suffix}"))
            ff_path.write_bytes(bundle.read(member))
    else:
        with tarfile.open(archive) as bundle:
            member = next(n for n in bundle.getnames() if n.endswith("/bin/ffmpeg"))
            ff_path.write_bytes(bundle.extractfile(member).read())

    print(f"YTCUT_YTDLP={yt_path}")
    print(f"YTCUT_FFMPEG={ff_path}")


if __name__ == "__main__":
    main()
