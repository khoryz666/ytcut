#!/usr/bin/env python3
"""Refresh tools.json: the pinned yt-dlp and ffmpeg release files.

Run this twice a year (the Update workflow does it in January and July). It
downloads every asset, records its SHA-256, and keeps the URLs and hashes in
one file that both Nix and the Windows release build read.

    python3 scripts/update-tools.py
"""

import hashlib
import json
import pathlib
import sys
import tempfile
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "tools.json"

YT_DLP = "https://github.com/yt-dlp/yt-dlp/releases/download/{tag}/{asset}"
FFMPEG = "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/{asset}"

# The asset for each platform ytcut ships for.
YT_DLP_ASSETS = {
    "x86_64-linux": "yt-dlp_linux",
    "aarch64-linux": "yt-dlp_linux_aarch64",
    "x86_64-windows": "yt-dlp.exe",
}
FFMPEG_ASSETS = {
    "x86_64-linux": "ffmpeg-master-latest-linux64-gpl.tar.xz",
    "aarch64-linux": "ffmpeg-master-latest-linuxarm64-gpl.tar.xz",
    "x86_64-windows": "ffmpeg-master-latest-win64-gpl.zip",
}


def latest(repo):
    """The latest release tag and name of a GitHub repository."""
    url = f"https://api.github.com/repos/{repo}/releases/latest"
    request = urllib.request.Request(url, headers={"User-Agent": "ytcut-update"})
    with urllib.request.urlopen(request) as response:
        release = json.load(response)
    return release["tag_name"], release["name"]


def digest(url):
    """Download a URL and return its SHA-256, checking that it exists."""
    print(f"  {url}", flush=True)
    sha = hashlib.sha256()
    with tempfile.TemporaryFile() as sink:
        request = urllib.request.Request(url, headers={"User-Agent": "ytcut-update"})
        with urllib.request.urlopen(request) as response:
            while chunk := response.read(1 << 20):
                sha.update(chunk)
                sink.write(chunk)
    return sha.hexdigest()


def asset(url, **extra):
    return {"url": url, "sha256": digest(url), **extra}


def main():
    yt_dlp_tag, _ = latest("yt-dlp/yt-dlp")
    ffmpeg_tag, ffmpeg_name = latest("yt-dlp/FFmpeg-Builds")
    print(f"yt-dlp {yt_dlp_tag}")
    print(f"ffmpeg {ffmpeg_name}")

    tools = {
        "yt-dlp": {
            "version": yt_dlp_tag,
            "assets": {
                system: asset(YT_DLP.format(tag=yt_dlp_tag, asset=name))
                for system, name in YT_DLP_ASSETS.items()
            },
        },
        "ffmpeg": {
            # The release is built automatically, so the date is the version.
            "version": ffmpeg_tag if ffmpeg_tag != "latest" else ffmpeg_name,
            "assets": {
                system: asset(FFMPEG.format(asset=name))
                for system, name in FFMPEG_ASSETS.items()
            },
        },
    }
    OUT.write_text(json.dumps(tools, indent=2) + "\n")
    print(f"wrote {OUT.relative_to(ROOT)}")


if __name__ == "__main__":
    try:
        main()
    except Exception as error:  # noqa: BLE001 - report the URL that failed
        sys.exit(f"could not update tools.json: {error}")
