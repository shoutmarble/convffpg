# Bundled FFmpeg asset

This project embeds the following archive into the Rust executable at compile time:

- File: `ffmpeg-linux-x86_64.tar.xz`
- Source: `https://johnvansickle.com/ffmpeg/releases/ffmpeg-release-amd64-static.tar.xz`
- SHA-256: `abda8d77ce8309141f83ab8edf0596834087c52467f6badf376a6a2a4c87cf67`

The runtime extraction code unpacks this archive into the app's local data directory on first launch and then invokes the extracted `ffmpeg` binary for conversions.