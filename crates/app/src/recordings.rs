//! The recordings in the folder Rec writes to (issue #28's follow-up): each
//! take with its length, read from its WAV header, newest first, for Audra
//! settings' list. Written with the owner's local model (qwen3.8-flash-next:
//! the header walk and the listing), finished and tested here.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One recording in the folder.
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    pub path: PathBuf,
    /// The file name without its folder, e.g. "Audra 2026-11-10 17-40-12 AN1.wav".
    pub name: String,
    /// Length in seconds, from the WAV header (data bytes / (rate * channels * bytes per sample)); 0.0 if the header can't be read.
    pub seconds: f32,
    pub modified: SystemTime,
}

/// Parses a WAV file's header from its first bytes (at least 44): returns (channels, sample_rate, bits_per_sample, data_bytes).
/// Must handle the canonical 44-byte PCM header AND files with extra chunks (e.g. "LIST") before the "data" chunk: walk the RIFF chunks after "WAVE", read "fmt " for channels/rate/bits, stop at "data" and return its size. Return None if it is not RIFF/WAVE, or "fmt "/"data" are missing within the given bytes, or channels/rate/bits are 0.
pub fn parse_header(bytes: &[u8]) -> Option<(u16, u32, u16, u64)> {
    if bytes.len() < 12 {
        return None;
    }

    if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }

    let mut pos = 12usize;
    let mut fmt: Option<(u16, u32, u16)> = None;

    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = read_u32(bytes, pos + 4)? as u64;
        let chunk_data = pos + 8;

        if id == b"fmt " {
            if size < 16 || chunk_data + 16 > bytes.len() {
                return None;
            }

            let channels = read_u16(bytes, chunk_data + 2)?;
            let sample_rate = read_u32(bytes, chunk_data + 4)?;
            let bits_per_sample = read_u16(bytes, chunk_data + 14)?;

            if channels == 0 || sample_rate == 0 || bits_per_sample == 0 {
                return None;
            }

            fmt = Some((channels, sample_rate, bits_per_sample));
        } else if id == b"data" {
            let (channels, sample_rate, bits_per_sample) = fmt?;
            return Some((channels, sample_rate, bits_per_sample, size));
        }

        let advance = 8 + size + (size % 2);
        pos = pos.checked_add(usize::try_from(advance).ok()?)?;
    }

    None
}

/// The `.wav` files (case-insensitive extension) directly in `folder` (not subfolders), newest first by modified time, at most `limit` of them. Reads only the first 4096 bytes of each file for the header. A missing or unreadable folder gives an empty list. Files whose header can't be parsed are still listed with seconds = 0.0.
pub fn list(folder: &Path, limit: usize) -> Vec<Take> {
    if limit == 0 {
        return Vec::new();
    }

    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut takes = Vec::new();

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let extension_matches = path
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("wav"))
            .unwrap_or(false);

        if !extension_matches {
            continue;
        }

        let metadata = match path.metadata() {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        takes.push(Take {
            path,
            name,
            seconds: 0.0,
            modified,
        });
    }

    // Newest first. Two takes written in the same second (their names then
    // carry a number) keep that order, the higher number first (job 20).
    takes.sort_by(|a, b| {
        b.modified
            .cmp(&a.modified)
            .then_with(|| take_number(&b.name).cmp(&take_number(&a.name)))
            .then_with(|| a.name.cmp(&b.name))
    });

    takes.truncate(limit);

    // The length of the takes shown only: a folder of thousands must not
    // have every one of them opened (job 20).
    for take in &mut takes {
        take.seconds = seconds_of(&take.path);
    }

    takes
}

/// A take's length in seconds, from its WAV header; 0.0 when the header
/// can't be read.
fn seconds_of(path: &Path) -> f32 {
    match read_first_bytes(path, 4096) {
        Some(bytes) => match parse_header(&bytes) {
            Some((channels, sample_rate, bits_per_sample, data_bytes)) => {
                let bytes_per_sample = (bits_per_sample as u64 / 8).max(1);
                let denominator = sample_rate as u64 * channels as u64 * bytes_per_sample;

                if denominator == 0 {
                    0.0
                } else {
                    (data_bytes as f64 / denominator as f64) as f32
                }
            }
            None => 0.0,
        },
        None => 0.0,
    }
}

/// The number in a take's name: "… AN1.wav" is the first, "… AN1 (2).wav" the
/// second, and a name that says no number is the first (job 20).
fn take_number(name: &str) -> u32 {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let Some((_, number)) = stem.rsplit_once(" (") else {
        return 1;
    };
    let Some((number, close)) = number.split_once(')') else {
        return 1;
    };
    if !close.is_empty() {
        return 1;
    }
    number.parse().unwrap_or(1)
}

fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

/// Up to `n` bytes from the start of the file at `path`.
fn read_first_bytes(path: &Path, n: usize) -> Option<Vec<u8>> {
    let mut buf = Vec::with_capacity(n);
    fs::File::open(path).ok()?.take(n as u64).read_to_end(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::time::Duration;

    fn canonical(channels: u16, rate: u32, bits: u16, data: u32) -> Vec<u8> {
        let align = channels * bits / 8;
        let mut h = Vec::new();
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&(36 + data).to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&channels.to_le_bytes());
        h.extend_from_slice(&rate.to_le_bytes());
        h.extend_from_slice(&(rate * u32::from(align)).to_le_bytes());
        h.extend_from_slice(&align.to_le_bytes());
        h.extend_from_slice(&bits.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&data.to_le_bytes());
        h
    }

    #[test]
    fn a_canonical_header_reads() {
        assert_eq!(parse_header(&canonical(2, 48_000, 24, 600)), Some((2, 48_000, 24, 600)));
        // Audra's own (recorder.rs) reads the same.
        assert_eq!(parse_header(&crate::recorder::header(1, 44_100, 3 * 44_100)), Some((1, 44_100, 24, 3 * 44_100)));
    }

    #[test]
    fn a_chunk_before_data_is_walked_over_with_its_pad_byte() {
        let mut h = canonical(1, 48_000, 16, 960);
        let data = h.split_off(36);
        h.extend_from_slice(b"LIST");
        h.extend_from_slice(&3u32.to_le_bytes());
        h.extend_from_slice(b"abc\0");
        h.extend_from_slice(&data);
        assert_eq!(parse_header(&h), Some((1, 48_000, 16, 960)));
    }

    #[test]
    fn not_a_wav_is_refused() {
        assert_eq!(parse_header(b"OggS\0\0\0\0\0\0\0\0\0\0"), None);
        assert_eq!(parse_header(&canonical(0, 48_000, 24, 6)), None);
        assert_eq!(parse_header(b"RIFF"), None);
    }

    #[test]
    fn the_folder_lists_its_wav_files_with_their_lengths() {
        let dir = std::env::temp_dir().join(format!("audra-takes-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let mut a = canonical(2, 48_000, 24, 48_000 * 6 * 2);
        a.resize(a.len() + 16, 0);
        fs::write(dir.join("a.wav"), &a).unwrap();
        fs::write(dir.join("B.WAV"), canonical(1, 48_000, 24, 48_000 * 3)).unwrap();
        fs::write(dir.join("notes.txt"), b"hi").unwrap();
        let takes = list(&dir, 10);
        let mut names: Vec<_> = takes.iter().map(|t| t.name.clone()).collect();
        names.sort();
        assert_eq!(names, vec!["B.WAV".to_string(), "a.wav".to_string()]);
        let secs = |n: &str| takes.iter().find(|t| t.name == n).unwrap().seconds;
        assert!((secs("a.wav") - 2.0).abs() < 1e-3);
        assert!((secs("B.WAV") - 1.0).abs() < 1e-3);
        assert_eq!(list(&dir, 1).len(), 1);
        assert!(list(&dir.join("missing"), 5).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_five_newest_are_the_five_listed() {
        // Job 20: the list shows five, so only those five are read.
        let dir = std::env::temp_dir().join(format!("audra-takes-seven-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Two seconds of sound: 48,000 frames, 3 bytes a sample.
        let mut header = canonical(1, 48_000, 24, 48_000 * 2 * 3);
        header.resize(header.len() + 8, 0);
        // Seven takes, a minute apart, so the times tell them apart.
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for k in 0..7 {
            let path = dir.join(format!("Audra take {k}.wav"));
            fs::write(&path, &header).unwrap();
            let at = base + Duration::from_secs(k * 60);
            File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
        }
        let takes = list(&dir, 5);
        let names: Vec<&str> = takes.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Audra take 6.wav", "Audra take 5.wav", "Audra take 4.wav", "Audra take 3.wav", "Audra take 2.wav"],
            "the five newest"
        );
        // The length is read for those five (2 s of sound in each).
        assert!((takes[0].seconds - 2.0).abs() < 1e-3, "{:?}", takes[0].seconds);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn two_takes_in_the_same_second_keep_their_order() {
        // Job 20: on a disk that keeps times to the second, the names carry
        // the order: the higher number is the newer take.
        let dir = std::env::temp_dir().join(format!("audra-takes-same-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Two seconds of sound: 48,000 frames, 3 bytes a sample.
        let mut header = canonical(1, 48_000, 24, 48_000 * 2 * 3);
        header.resize(header.len() + 8, 0);
        let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        for name in ["Audra AN1.wav", "Audra AN1 (2).wav", "Audra AN1 (3).wav"] {
            let path = dir.join(name);
            fs::write(&path, &header).unwrap();
            File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
        }
        let names: Vec<String> = list(&dir, 5).into_iter().map(|t| t.name).collect();
        assert_eq!(names, vec!["Audra AN1 (3).wav", "Audra AN1 (2).wav", "Audra AN1.wav"]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_takes_number_is_read_from_its_name() {
        assert_eq!(take_number("x.wav"), 1);
        assert_eq!(take_number("x (2).wav"), 2);
        assert_eq!(take_number("x (12).wav"), 12);
        assert_eq!(take_number("x (a).wav"), 1, "a name that says no number is the first");
        assert_eq!(take_number("Audra 2026-11-11 21-40-12 AN1 (3).wav"), 3);
    }
}
