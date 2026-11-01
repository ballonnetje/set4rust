//! IVF, the container of the converted videos (one AV1 temporal unit per frame): a 32-byte file
//! header (`DKIF`, version, header size, fourcc, width, height, time base denominator and
//! numerator, frame count), then per frame a 12-byte header (size u32, pts u64) and the data.

use anyhow::{Context, bail};

pub struct Ivf {
    data: Vec<u8>,
    pub fourcc: [u8; 4],
    pub width: u32,
    pub height: u32,
    /// Seconds per pts unit, as (numerator, denominator).
    pub time_base: (u32, u32),
    /// Per frame: pts, data start, data length.
    frames: Vec<(u64, usize, usize)>,
}

impl Ivf {
    pub fn open(path: &std::path::Path) -> anyhow::Result<Ivf> {
        Ivf::parse(std::fs::read(path).with_context(|| path.display().to_string())?)
    }

    pub fn parse(data: Vec<u8>) -> anyhow::Result<Ivf> {
        let u16_at = |o: usize| u16::from_le_bytes([data[o], data[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes(data[o..o + 4].try_into().unwrap());
        if data.len() < 32 || &data[..4] != b"DKIF" {
            bail!("not an IVF file");
        }
        let header = u16_at(6) as usize;
        let fourcc = data[8..12].try_into().unwrap();
        let (width, height) = (u16_at(12) as u32, u16_at(14) as u32);
        let (den, num) = (u32_at(16), u32_at(20));
        if den == 0 || num == 0 {
            bail!("IVF time base {num}/{den}");
        }
        let mut frames = Vec::new();
        let mut o = header.max(32);
        while o + 12 <= data.len() {
            let size = u32_at(o) as usize;
            let pts = u64::from_le_bytes(data[o + 4..o + 12].try_into().unwrap());
            if o + 12 + size > data.len() {
                break; // a cut-off last frame
            }
            frames.push((pts, o + 12, size));
            o += 12 + size;
        }
        Ok(Ivf { data, fourcc, width, height, time_base: (num, den), frames })
    }

    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    /// Frame `i`: its time in seconds and its data.
    pub fn frame(&self, i: usize) -> (f64, &[u8]) {
        let (pts, at, size) = self.frames[i];
        (self.seconds(pts), &self.data[at..at + size])
    }

    fn seconds(&self, pts: u64) -> f64 {
        pts as f64 * self.time_base.0 as f64 / self.time_base.1 as f64
    }

    /// The end of the last frame in seconds (its pts plus one frame's step).
    pub fn duration(&self) -> f64 {
        match self.frames.as_slice() {
            [] => 0.0,
            [.., a, b] => self.seconds(b.0) + self.seconds(b.0 - a.0.min(b.0)),
            [a] => self.seconds(a.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_frames() {
        let mut d = Vec::new();
        d.extend_from_slice(b"DKIF");
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&32u16.to_le_bytes());
        d.extend_from_slice(b"AV01");
        d.extend_from_slice(&640u16.to_le_bytes());
        d.extend_from_slice(&480u16.to_le_bytes());
        d.extend_from_slice(&25u32.to_le_bytes());
        d.extend_from_slice(&1u32.to_le_bytes());
        d.extend_from_slice(&2u32.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        for (pts, body) in [(0u64, &b"abc"[..]), (1, b"de")] {
            d.extend_from_slice(&(body.len() as u32).to_le_bytes());
            d.extend_from_slice(&pts.to_le_bytes());
            d.extend_from_slice(body);
        }
        d.extend_from_slice(&[9, 0, 0, 0]); // a cut-off header
        let ivf = Ivf::parse(d).unwrap();
        assert_eq!((ivf.width, ivf.height, ivf.len(), &ivf.fourcc), (640, 480, 2, b"AV01"));
        assert_eq!(ivf.frame(1), (0.04, &b"de"[..]));
        assert!((ivf.duration() - 0.08).abs() < 1e-9);
    }
}
