//! Playing the game's sounds: the installed AIFF files and the sound
//! members inside the game program.

use std::collections::HashMap;
use std::path::PathBuf;

use macroquad::audio::{Sound, load_sound_from_bytes, play_sound_once, stop_sound};

/// Convert an AIFF file to an in-memory WAV, which macroquad can play.
/// Handles the uncompressed mono/stereo 8- and 16-bit files the game ships.
pub fn aiff_to_wav(aiff: &[u8]) -> Option<Vec<u8>> {
    if aiff.get(0..4)? != b"FORM" || aiff.get(8..12)? != b"AIFF" {
        return None;
    }
    let (mut channels, mut bits, mut rate, mut samples) = (0u16, 0u16, 0u32, None);
    let mut p = 12;
    while p + 8 <= aiff.len() {
        let id = &aiff[p..p + 4];
        let len = u32::from_be_bytes(aiff[p + 4..p + 8].try_into().ok()?) as usize;
        let body = aiff.get(p + 8..(p + 8 + len).min(aiff.len()))?;
        match id {
            b"COMM" => {
                channels = u16::from_be_bytes(body.get(0..2)?.try_into().ok()?);
                bits = u16::from_be_bytes(body.get(6..8)?.try_into().ok()?);
                rate = extended_to_u32(body.get(8..18)?);
            }
            b"SSND" => {
                let offset = u32::from_be_bytes(body.get(0..4)?.try_into().ok()?) as usize;
                samples = body.get(8 + offset..);
            }
            _ => {}
        }
        p += 8 + len + (len & 1); // chunks are padded to even sizes
    }
    let samples = samples?;
    if channels == 0 || rate == 0 {
        return None;
    }

    // AIFF is signed big-endian; WAV wants unsigned 8-bit or signed little-endian 16-bit.
    let data: Vec<u8> = match bits {
        8 => samples.iter().map(|&s| s.wrapping_add(0x80)).collect(),
        16 => samples.as_chunks::<2>().0.iter().flat_map(|c| [c[1], c[0]]).collect(),
        _ => return None,
    };
    Some(wav(channels, bits, rate, &data))
}

/// 8-bit unsigned mono samples (a Director sound member) as a WAV file.
pub fn pcm8_to_wav(rate: u32, samples: &[u8]) -> Vec<u8> {
    wav(1, 8, rate, samples)
}

pub fn wav(channels: u16, bits: u16, rate: u32, data: &[u8]) -> Vec<u8> {
    let block = channels * bits / 8;
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * block as u32).to_le_bytes());
    wav.extend_from_slice(&block.to_le_bytes());
    wav.extend_from_slice(&bits.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(data);
    wav
}

/// AIFF stores the sample rate as an 80-bit IEEE extended float.
fn extended_to_u32(b: &[u8]) -> u32 {
    let exponent = (u16::from_be_bytes([b[0], b[1]]) & 0x7FFF) as i32 - 16383;
    let mantissa = u64::from_be_bytes(b[2..10].try_into().unwrap());
    let value = mantissa as f64 / (1u64 << 63) as f64 * 2f64.powi(exponent);
    value.round() as u32
}

/// The sounds, loaded on first use: an installed sound by that name, else a
/// sound member (`cast`, name -> WAV). Like the original, one sound plays at
/// a time: starting a new one stops the last.
pub struct Sounds {
    folder: PathBuf,
    cast: HashMap<String, Vec<u8>>,
    loaded: HashMap<String, Option<Sound>>,
    playing: Option<String>,
}

impl Sounds {
    pub fn new(folder: PathBuf, cast: HashMap<String, Vec<u8>>) -> Self {
        Self { folder, cast, loaded: HashMap::new(), playing: None }
    }

    pub fn stop(&mut self) {
        if let Some(previous) = self.playing.take()
            && let Some(Some(s)) = self.loaded.get(&previous)
        {
            stop_sound(s);
        }
    }

    /// Play a movie's sound track (a WAV), cached under `key`.
    pub async fn play_wav(&mut self, key: &str, wav: &[u8]) {
        if !self.loaded.contains_key(key) {
            let sound = load_sound_from_bytes(wav).await.ok();
            self.loaded.insert(key.to_string(), sound);
        }
        self.stop();
        if let Some(Some(s)) = self.loaded.get(key) {
            play_sound_once(s);
            self.playing = Some(key.to_string());
        }
    }

    pub async fn play(&mut self, name: &str) {
        if !self.loaded.contains_key(name) {
            // The installed files' names differ in case from the scripts'
            // ("60 min" for "60 Min").
            let installed = std::fs::read_dir(&self.folder)
                .ok()
                .and_then(|dir| dir.flatten().find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(name)))
                .and_then(|e| std::fs::read(e.path()).ok())
                .and_then(|a| aiff_to_wav(&a));
            // Director matches member names without case ("Menu Sound").
            let cast = self.cast.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, wav)| wav.clone());
            let sound = match installed.or(cast) {
                Some(wav) => load_sound_from_bytes(&wav).await.ok(),
                None => None,
            };
            self.loaded.insert(name.to_string(), sound);
        }
        if let Some(previous) = self.playing.take()
            && let Some(Some(s)) = self.loaded.get(&previous)
        {
            stop_sound(s);
        }
        if let Some(Some(s)) = self.loaded.get(name) {
            play_sound_once(s);
            self.playing = Some(name.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_22khz_rate() {
        // 22254.545 Hz, as stored by Mac sound files.
        let ext = [0x40, 0x0D, 0xAD, 0xDD, 0x17, 0x3E, 0xAB, 0x36, 0x7A, 0x0F];
        assert_eq!(extended_to_u32(&ext), 22255);
    }

    #[test]
    fn converts_8bit_mono() {
        let mut a = b"FORM\0\0\0\0AIFFCOMM\0\0\0\x12\0\x01\0\0\0\x02\0\x08".to_vec();
        a.extend_from_slice(&[0x40, 0x0D, 0xAD, 0xDD, 0x17, 0x3E, 0xAB, 0x36, 0x7A, 0x0F]);
        a.extend_from_slice(b"SSND\0\0\0\x0a\0\0\0\0\0\0\0\0\x00\x7f");
        let wav = aiff_to_wav(&a).unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[44..], &[0x80, 0xFF]);
    }
}
