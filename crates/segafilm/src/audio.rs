//! Sega CD PCM audio.
//!
//! Samples are 8-bit sign-magnitude, the native format of the Sega CD's
//! RF5C164 PCM chip: bit 7 is the sign (set = negative), bits 0-6 the
//! magnitude. So 0x01 = +1, 0x81 = -1, and both 0x00 and 0x80 are silence.

/// Playback rate: the PCM chip's 32,552 Hz clock with a frequency delta of
/// 0x400 (half speed), as the Sega CD movie player programs it.
pub const SAMPLE_RATE: u32 = 16_276;

/// Convert sign-magnitude samples to unsigned 8-bit PCM (0x80 = silence),
/// the format WAV files use.
pub fn to_unsigned(samples: &[u8]) -> Vec<u8> {
    samples
        .iter()
        .map(|&b| {
            let magnitude = b & 0x7F;
            if b & 0x80 != 0 { 0x80 - magnitude } else { 0x80 + magnitude }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_magnitude() {
        assert_eq!(to_unsigned(&[0x00, 0x80, 0x01, 0x81, 0x7F, 0xFF]), [0x80, 0x80, 0x81, 0x7F, 0xFF, 0x01]);
    }
}
