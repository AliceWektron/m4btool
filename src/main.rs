use regex::Regex;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{Write, BufWriter};
use std::path::Path;
use std::process::Command;
use tempfile::{NamedTempFile, Builder};
use walkdir::WalkDir;

/// Represents a token parsed from a chapter title.
/// A token may either be bracketed (e.g. "[Intro]") or not.
/// The flag `is_bracketed` helps distinguish between tokens that should be treated differently.
#[derive(Debug)]
struct TitleToken {
    is_bracketed: bool,
    text: String,
}

/// Standardizes different types of bracket characters in the input string
/// by replacing them with the common bracket characters "[" and "]".
///
/// # Arguments
///
/// * `input` - A string slice that potentially contains various bracket styles.
///
/// # Returns
///
/// A `String` with all bracket types standardized to square brackets.
fn standardize_brackets(input: &str) -> String {
    input.replace("（", "[")
         .replace("）", "]")
         .replace("(", "[")
         .replace(")", "]")
         .replace("【", "[")
         .replace("】", "]")
}

/// Splits a chapter title into tokens using regular expressions.
/// Tokens can either be bracketed segments (like "[Intro]" or "(Overview)")
/// or non-bracketed text segments. This function leverages `standardize_brackets`
/// to ensure consistent processing.
///
/// # Arguments
///
/// * `title` - The chapter title as a string slice.
///
/// # Returns
///
/// A vector of `TitleToken` instances representing the parsed tokens.
fn split_title_tokens(title: &str) -> Vec<TitleToken> {
    let standardized_title = standardize_brackets(title);
    // Regex pattern captures either bracketed expressions or continuous non-numeric and non-bracketed text.
    let token_pattern = Regex::new(r"(\(.*?\)|\[.*?\])|([^0-9\s\-:：\(\)\[\]]+)").unwrap();
    let mut tokens = Vec::new();

    for capture in token_pattern.captures_iter(&standardized_title) {
        if let Some(bracketed) = capture.get(1) {
            tokens.push(TitleToken { is_bracketed: true, text: bracketed.as_str().to_string() });
        } else if let Some(non_bracketed) = capture.get(2) {
            tokens.push(TitleToken { is_bracketed: false, text: non_bracketed.as_str().to_string() });
        }
    }
    tokens
}

/// Builds a frequency map of non-bracketed tokens across multiple chapter titles.
/// This is used later to decide if a token should be removed based on its occurrence frequency.
///
/// # Arguments
///
/// * `titles` - A slice of chapter title strings.
///
/// # Returns
///
/// A `HashMap` where each key is a non-bracketed token and the value is the occurrence count.
fn build_token_frequency(titles: &[String]) -> HashMap<String, usize> {
    let mut token_frequency = HashMap::new();
    for title in titles {
        for token in split_title_tokens(title) {
            // Only count non-bracketed tokens to avoid removing significant descriptive parts.
            if !token.is_bracketed {
                *token_frequency.entry(token.text).or_insert(0) += 1;
            }
        }
    }
    token_frequency
}

/// Cleans up a chapter title dynamically by removing common tokens that exceed a given frequency threshold.
/// This helps in removing redundant words from the beginning of titles (e.g., repeated "Chapter" labels).
///
/// # Arguments
///
/// * `title` - The original chapter title as a string slice.
/// * `token_frequency` - A frequency map of tokens obtained from `build_token_frequency`.
/// * `total_titles` - Total number of chapter titles processed.
/// * `threshold` - A fractional threshold (e.g., 0.8) that determines token removal based on frequency.
///
/// # Returns
///
/// A cleaned-up title string with the common tokens removed.
fn dynamic_clean_title(title: &str, token_frequency: &HashMap<String, usize>, total_titles: usize, threshold: f64) -> String {
    let tokens = split_title_tokens(title);
    let mut cleaned_tokens = Vec::new();
    let mut in_removal_phase = true;

    for token in tokens {
        // In the removal phase, skip tokens that are overly common.
        if in_removal_phase && !token.is_bracketed {
            let frequency = token_frequency.get(&token.text).copied().unwrap_or(0);
            if (frequency as f64) / (total_titles as f64) >= threshold {
                continue;
            } else {
                // Token is not too common, so end removal phase and keep it.
                in_removal_phase = false;
                cleaned_tokens.push(token.text);
            }
        } else {
            // Once the removal phase is over, keep all tokens (especially bracketed ones).
            if token.is_bracketed {
                in_removal_phase = false;
            }
            cleaned_tokens.push(token.text);
        }
    }
    cleaned_tokens.join("").trim().to_string()
}

/// Retrieves the duration of an audio file in milliseconds by using `ffprobe`.
/// This function invokes `ffprobe` as a subprocess and parses the output to obtain the duration.
///
/// # Arguments
///
/// * `file_path` - The file path to the audio file as a string slice.
///
/// # Returns
///
/// An `Option<u64>` representing the duration in milliseconds, or `None` if the duration cannot be determined.
fn get_duration_ms(file_path: &str) -> Option<u64> {
    let output = Command::new("ffprobe")
        .args(&[
            "-v", "error",
            "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
            file_path,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!("ffprobe error for {}: {}", file_path, String::from_utf8_lossy(&output.stderr));
        return None;
    }
    let duration_str = String::from_utf8_lossy(&output.stdout);
    let duration_sec: f64 = duration_str.trim().parse().ok()?;
    Some((duration_sec * 1000.0).round() as u64)
}

/// Extracts audio stream information from a file using `ffprobe`.
/// It retrieves details such as the codec name and bitrate of the first audio stream.
///
/// # Arguments
///
/// * `file_path` - The file path to the audio file.
///
/// # Returns
///
/// An `Option` containing a tuple:
/// - A `String` representing the codec name.
/// - An `Option<u64>` representing the bitrate in bits per second (if available).
fn get_audio_info(file_path: &str) -> Option<(String, Option<u64>)> {
    let output = Command::new("ffprobe")
        .args(&[
            "-v", "error",
            "-select_streams", "a:0",
            "-show_entries", "stream=codec_name,bit_rate",
            "-of", "default=noprint_wrappers=1:nokey=1",
            file_path,
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        eprintln!("ffprobe error for {}: {}", file_path, String::from_utf8_lossy(&output.stderr));
        return None;
    }
    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut lines = output_str.lines();
    let codec = lines.next()?.to_string();
    let bit_rate = lines.next().and_then(|s| s.trim().parse::<u64>().ok());
    Some((codec, bit_rate))
}

/// Re-encodes an audio file to AAC using the `libfdk_aac` codec at a constant bitrate
/// that matches the source file's bitrate (or defaults to 128k if unavailable).
/// The output is written to a temporary file.
///
/// # Arguments
///
/// * `file_path` - The file path of the source audio file.
///
/// # Returns
///
/// An `Option<NamedTempFile>` containing the temporary file with the re-encoded audio,
/// or `None` if the process fails.
fn reencode_audio(file_path: &str) -> Option<NamedTempFile> {
    // Create a temporary file for the re-encoded output with a .m4a extension.
    let tmpfile = Builder::new().suffix(".m4a").tempfile().ok()?;
    let tmpfile_path = tmpfile.path().to_str().unwrap().to_string();

    // Retrieve the source file's bitrate; default to 128k if not available.
    let bitrate_str = if let Some((_, Some(bit_rate))) = get_audio_info(file_path) {
        // Convert bits per second to kilobits per second.
        format!("{}k", bit_rate / 1000)
    } else {
        "128k".to_string() // fallback if bitrate information isn't available
    };

    // Execute ffmpeg to re-encode the audio stream using libfdk_aac at the desired bitrate.
    let status = Command::new("ffmpeg")
        .args(&[
            "-i", file_path,
            "-vn",
            "-map", "0:a",
            "-c:a", "libfdk_aac",
            "-b:a", &bitrate_str,
            "-y", &tmpfile_path,
        ])
        .status()
        .ok()?;
    if status.success() {
        Some(tmpfile)
    } else {
        eprintln!("Error reencoding file: {}", file_path);
        None
    }
}

/// Main entry point of the audiobook creation tool.
///
/// This function:
/// 1. Validates command-line arguments and input directory.
/// 2. Searches for supported audio files (mp3, m4a, flac) within the input directory.
/// 3. Processes chapter titles to clean them up using dynamic token frequency analysis.
/// 4. Re-encodes each audio file to ensure consistent audio quality and bitrate.
/// 5. Constructs a concat list and metadata file (including chapters and durations).
/// 6. Optionally incorporates a cover image if present.
/// 7. Invokes ffmpeg to merge all processed audio files into a single audiobook file.
///
/// # Behavior
///
/// On success, the final audiobook is saved as `output.m4b` in the input directory.
/// On failure, relevant error messages are printed to stderr.
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <input_directory>", args[0]);
        return;
    }
    let input_directory = &args[1];
    if !Path::new(input_directory).is_dir() {
        eprintln!("Error: '{}' is not a valid directory", input_directory);
        return;
    }

    // Define the output audiobook path.
    let audiobook_output_path = format!("{}/output.m4b", input_directory);
    if Path::new(&audiobook_output_path).exists() {
        if let Err(err) = fs::remove_file(&audiobook_output_path) {
            eprintln!("Error removing existing file '{}': {}", audiobook_output_path, err);
            return;
        }
    }

    // Collect supported audio files from the input directory and sort them by filename.
    let mut audio_file_entries: Vec<_> = WalkDir::new(input_directory)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry.file_type().is_file() &&
            entry.path().extension().map(|ext| {
                let ext_lc = ext.to_string_lossy().to_lowercase();
                ext_lc == "mp3" || ext_lc == "m4a" || ext_lc == "flac"
            }).unwrap_or(false)
        })
        .collect();
    audio_file_entries.sort_by_key(|entry| entry.file_name().to_os_string());

    if audio_file_entries.is_empty() {
        eprintln!("No supported audio files found in '{}'", input_directory);
        return;
    }

    // Build chapter titles and token frequency map for dynamic title cleaning.
    let chapter_titles: Vec<String> = audio_file_entries.iter()
        .filter_map(|entry| entry.path().file_stem().map(|stem| stem.to_string_lossy().to_string()))
        .collect();
    let token_frequency_map = build_token_frequency(&chapter_titles);
    let total_chapters = chapter_titles.len();

    let mut reencoded_tempfiles: Vec<NamedTempFile> = Vec::new();
    let mut final_files: Vec<(String, String)> = Vec::new();

    // Re-encode all audio files to ensure a consistent audio format.
    for entry in audio_file_entries {
        let file_path = entry.path().to_str().unwrap().to_string();
        let original_title = entry.path().file_stem().unwrap().to_string_lossy().to_string();
        let mut final_file_path = file_path.clone();

        if let Some(tmpfile) = reencode_audio(&file_path) {
            final_file_path = tmpfile.path().to_str().unwrap().to_string();
            reencoded_tempfiles.push(tmpfile);
        } else {
            eprintln!("Re-encoding failed for {}. Using original file.", file_path);
        }
        final_files.push((final_file_path, original_title));
    }

    // Create a temporary file listing all files for ffmpeg concatenation.
    let mut concat_file = NamedTempFile::new().expect("Could not create temporary file for concat list");
    for (file_path, _) in &final_files {
        writeln!(concat_file, "file '{}'", file_path).expect("Error writing to concat list file");
    }
    let concat_file_path = concat_file.into_temp_path();

    // Generate metadata file with chapter markers, durations, and cleaned titles.
    let metadata_temp_file = NamedTempFile::new().expect("Could not create temporary file for metadata");
    {
        let mut metadata_writer = BufWriter::new(&metadata_temp_file);
        writeln!(metadata_writer, ";FFMETADATA1").expect("Error writing metadata header");

        let mut current_chapter_start_ms = 0u64;
        for (file_path, original_title) in &final_files {
            let cleaned_title = dynamic_clean_title(&original_title, &token_frequency_map, total_chapters, 0.8);
            if let Some(duration_ms) = get_duration_ms(file_path) {
                let chapter_end_ms = current_chapter_start_ms + duration_ms;
                writeln!(metadata_writer, "[CHAPTER]").expect("Error writing chapter marker");
                writeln!(metadata_writer, "TIMEBASE=1/1000").expect("Error writing timebase");
                writeln!(metadata_writer, "START={}", current_chapter_start_ms).expect("Error writing chapter start");
                writeln!(metadata_writer, "END={}", chapter_end_ms).expect("Error writing chapter end");
                writeln!(metadata_writer, "title={}", cleaned_title).expect("Error writing chapter title");
                current_chapter_start_ms = chapter_end_ms;
            } else {
                eprintln!("Warning: Could not retrieve duration for file '{}'", file_path);
            }
        }
        metadata_writer.flush().expect("Error flushing metadata writer");
    }
    let metadata_file_path = metadata_temp_file.into_temp_path();

    // Attempt to locate a cover image with a supported extension.
    let cover_image_extensions = ["jpg", "jpeg", "png", "webp"];
    let cover_image_path = cover_image_extensions.iter()
        .map(|ext| format!("{}/cover.{}", input_directory, ext))
        .find(|path| Path::new(path).exists());

    // Build the ffmpeg command with appropriate arguments based on whether a cover image is present.
    let mut ffmpeg_cmd = Command::new("ffmpeg");
    ffmpeg_cmd
        .arg("-f")
        .arg("concat")
        .arg("-safe")
        .arg("0")
        .arg("-i")
        .arg(concat_file_path.to_str().unwrap());

    if let Some(ref cover_path) = cover_image_path {
        ffmpeg_cmd
            .arg("-i")
            .arg(cover_path)
            .arg("-i")
            .arg(metadata_file_path.to_str().unwrap())
            .arg("-map")
            .arg("0:a")
            .arg("-map")
            .arg("1")
            .arg("-map_metadata")
            .arg("2");
    } else {
        ffmpeg_cmd
            .arg("-i")
            .arg(metadata_file_path.to_str().unwrap())
            .arg("-map")
            .arg("0:a")
            .arg("-map_metadata")
            .arg("1");
    }

    ffmpeg_cmd.arg("-c:a").arg("copy");

    if cover_image_path.is_some() {
        ffmpeg_cmd.arg("-c:v")
                  .arg("mjpeg")
                  .arg("-disposition:v:0")
                  .arg("attached_pic");
    }

    ffmpeg_cmd
        .arg("-metadata")
        .arg("title=Audiobook")
        .arg(&audiobook_output_path);

    println!("Executing ffmpeg command: {:?}", ffmpeg_cmd);

    // Execute the constructed ffmpeg command and log the result.
    match ffmpeg_cmd.output() {
        Ok(output) => {
            if !output.status.success() {
                eprintln!("FFmpeg execution failed: {}", String::from_utf8_lossy(&output.stderr));
            } else {
                println!("Success: Audiobook created at '{}'", audiobook_output_path);
            }
        },
        Err(err) => {
            eprintln!("Error executing ffmpeg command: {}", err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::tempdir;
    use walkdir::WalkDir;

    /// Tests that `split_title_tokens` correctly identifies both bracketed and non-bracketed tokens.
    #[test]
    fn test_split_title_tokens() {
        let title = "Chapter 1 [Intro] (Overview)";
        let tokens = split_title_tokens(title);
        assert!(!tokens.is_empty());
        let bracketed: Vec<_> = tokens.iter().filter(|t| t.is_bracketed).collect();
        assert!(!bracketed.is_empty());
    }

    /// Tests that `dynamic_clean_title` properly cleans a title by removing common tokens.
    #[test]
    fn test_dynamic_clean_title() {
        let titles = vec![
            "Chapter 1 [Intro]".to_string(),
            "Chapter 2 [Intro]".to_string(),
            "Chapter 3 [Intro]".to_string(),
        ];
        let freq = build_token_frequency(&titles);
        let cleaned = dynamic_clean_title("Chapter 1 [Intro]", &freq, titles.len(), 0.8);
        assert!(!cleaned.is_empty());
    }

    /// Tests the collection of audio files from a directory, ensuring only supported files are picked up.
    #[test]
    fn test_collect_audio_files() {
        let dir = tempdir().unwrap();
        let file_names = ["test.mp3", "audio.m4a", "sound.flac", "ignore.txt"];
        for name in &file_names {
            let file_path = dir.path().join(name);
            File::create(&file_path).unwrap();
        }
        let audio_files: Vec<_> = WalkDir::new(dir.path())
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_type().is_file() &&
                entry.path().extension().map(|ext| {
                    let ext_lc = ext.to_string_lossy().to_lowercase();
                    ext_lc == "mp3" || ext_lc == "m4a" || ext_lc == "flac"
                }).unwrap_or(false)
            })
            .collect();
        assert_eq!(audio_files.len(), 3);
    }
}
