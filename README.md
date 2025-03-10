# M4BTOOL

M4BTOOL is a command-line tool written in Rust that concatenates and re-encodes multiple audio files (such as MP3, M4A, and FLAC) into a single audiobook file in M4B format. It automatically cleans chapter titles by removing redundant tokens, adds chapter metadata with accurate durations, and supports embedding a cover image.

## Features

- **Audio Concatenation:** Merge multiple audio files into one seamless audiobook.
- **Dynamic Title Cleaning:** Automatically remove common or redundant tokens from chapter titles.
- **Re-encoding:** Standardizes audio quality by re-encoding files to a consistent bitrate using `ffmpeg` and `ffprobe`.
- **Chapter Metadata:** Generates chapter markers with start and end times for easy navigation.
- **Cover Image Support:** Embeds a cover image if one is available (supported formats: JPG, JPEG, PNG, WEBP).
- **Cross-platform:** Built with Rust and tested for robust performance.

## Prerequisites

- **Rust:** Ensure you have the latest version of [Rust](https://rustup.rs/) installed.
- **FFmpeg & FFprobe:** These tools are required for audio processing. Install them via your package manager or from the [FFmpeg website](https://ffmpeg.org/download.html).
- **libfdk_aac:** For optimal AAC encoding, make sure your `ffmpeg` build includes support for `libfdk_aac`.
