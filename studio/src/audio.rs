//! Listening to originals and replacements, and turning audio files into 16-bit PCM WAV.

use std::fs::File;
use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

/// One track at a time, on the default output device, opened at the first play.
#[derive(Default)]
pub struct Audio {
    sink: Option<MixerDeviceSink>,
    player: Option<Player>,
    track: Option<Track>,
}

struct Track {
    path: PathBuf,
    duration: Option<Duration>,
}

impl Audio {
    /// Starts a file, or pauses / resumes it when it is the one playing.
    pub fn toggle(&mut self, path: &Path, volume: f32) -> Result<()> {
        if let (Some(t), Some(p)) = (&self.track, &self.player)
            && t.path == path
            && !p.empty()
        {
            if p.is_paused() { p.play() } else { p.pause() }
            return Ok(());
        }
        self.start(path, volume)
    }

    fn start(&mut self, path: &Path, volume: f32) -> Result<()> {
        self.stop();
        if self.sink.is_none() {
            let mut sink = DeviceSinkBuilder::open_default_sink().context("no audio output")?;
            sink.log_on_drop(false);
            self.sink = Some(sink);
        }
        let file = File::open(path).with_context(|| path.display().to_string())?;
        let len = file.metadata().with_context(|| path.display().to_string())?.len();
        let mut builder = Decoder::builder().with_data(BufReader::new(file)).with_byte_len(len).with_seekable(true).with_coarse_seek(true);
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            builder = builder.with_hint(ext);
        }
        let source = builder.build().with_context(|| format!("{}: cannot decode", path.display()))?;
        let duration = source.total_duration();
        let sink = self.sink.as_ref().context("no audio output")?;
        let player = Player::connect_new(sink.mixer());
        player.set_volume(volume);
        player.append(source);
        self.player = Some(player);
        self.track = Some(Track { path: path.to_path_buf(), duration });
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(p) = self.player.take() {
            p.stop();
        }
        self.track = None;
    }

    pub fn set_volume(&self, v: f32) {
        if let Some(p) = &self.player {
            p.set_volume(v);
        }
    }

    /// Whether `path` is sounding now.
    pub fn is_playing(&self, path: &Path) -> bool {
        self.is_current(path) && self.player.as_ref().is_some_and(|p| !p.is_paused() && !p.empty())
    }

    /// Whether `path` is loaded, playing or paused.
    pub fn is_current(&self, path: &Path) -> bool {
        self.track.as_ref().is_some_and(|t| t.path == path) && self.player.as_ref().is_some_and(|p| !p.empty())
    }

    pub fn any_playing(&self) -> bool {
        self.track.is_some() && self.player.as_ref().is_some_and(|p| !p.is_paused() && !p.empty())
    }

    /// Position and, where the file tells, length of `path`.
    pub fn position(&self, path: &Path) -> Option<(Duration, Option<Duration>)> {
        if !self.is_current(path) {
            return None;
        }
        Some((self.player.as_ref()?.get_pos(), self.track.as_ref()?.duration))
    }

    /// Jumps to a position; `Err`: this file or decoder cannot.
    pub fn seek(&mut self, to: Duration) -> Result<(), String> {
        match &self.player {
            Some(p) => p.try_seek(to).map_err(|e| e.to_string()),
            None => Ok(()),
        }
    }
}

/// Interleaved samples as a 16-bit PCM WAV file.
pub fn encode_wav(samples: &[f32], channels: u16, rate: u32) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for &s in samples {
        let v = s.clamp(-1.0, 1.0);
        let i = if v < 0.0 { v * 32768.0 } else { v * 32767.0 } as i16;
        out.extend_from_slice(&i.to_le_bytes());
    }
    out
}

/// An audio file as 16-bit PCM WAV, at the file's own rate and channel count.
pub fn to_wav(bytes: Vec<u8>, name: &str) -> Result<Vec<u8>> {
    let dec = Decoder::new(Cursor::new(bytes)).with_context(|| format!("{name}: not an audio file the studio can read"))?;
    let (channels, rate) = (dec.channels().get(), dec.sample_rate().get());
    let samples: Vec<f32> = dec.collect();
    Ok(encode_wav(&samples, channels, rate))
}

/// `m:ss.d`.
pub fn clock(d: Duration) -> String {
    let t = d.as_secs_f64();
    format!("{}:{:04.1}", (t / 60.0).floor() as u64, t % 60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_and_sample_rounding() {
        let w = encode_wav(&[0.0, 1.0, -1.0, 2.0], 2, 22050);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(u16::from_le_bytes(w[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 22050);
        assert_eq!(u32::from_le_bytes(w[28..32].try_into().unwrap()), 88200);
        assert_eq!(u16::from_le_bytes(w[32..34].try_into().unwrap()), 4);
        assert_eq!(u16::from_le_bytes(w[34..36].try_into().unwrap()), 16);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
        let s: Vec<i16> = w[44..].as_chunks::<2>().0.iter().map(|c| i16::from_le_bytes(*c)).collect();
        assert_eq!(s, vec![0, 32767, -32768, 32767]);
    }

    #[test]
    fn wav_decodes_and_is_read_back() {
        let samples: Vec<f32> = (0..2205).map(|i| (i as f32 * 0.05).sin() * 0.5).collect();
        let wav = encode_wav(&samples, 1, 22050);
        let again = to_wav(wav.clone(), "a.wav").unwrap();
        assert_eq!(&again[..12], &wav[..12]);
        assert_eq!(u32::from_le_bytes(again[24..28].try_into().unwrap()), 22050);
        assert_eq!(again.len(), wav.len());
        assert!(to_wav(b"nope".to_vec(), "a.bin").is_err());
    }

    #[test]
    fn clock_text() {
        assert_eq!(clock(Duration::from_secs_f64(75.25)), "1:15.2");
        assert_eq!(clock(Duration::ZERO), "0:00.0");
    }
}
