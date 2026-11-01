//! Zip writer: stored entries (PNG / OGG / MP3 are compressed already), UTF-8 names, no zip64.

use std::io::Write;

use anyhow::{Result, bail};

const fn crc_table() -> [u32; 256] {
    let mut t = [0u32; 256];
    let mut n = 0;
    while n < 256 {
        let mut c = n as u32;
        let mut k = 0;
        while k < 8 {
            c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        t[n] = c;
        n += 1;
    }
    t
}

static CRC_TABLE: [u32; 256] = crc_table();

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = 0xffff_ffffu32;
    for &b in bytes {
        c = CRC_TABLE[((c ^ b as u32) & 255) as usize] ^ (c >> 8);
    }
    c ^ 0xffff_ffff
}

/// DOS time and date fields.
pub fn dos_time(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> (u16, u16) {
    (((hour << 11) | (minute << 5) | (second >> 1)) as u16, ((((year - 1980) as u32) << 9) | (month << 5) | day) as u16)
}

pub struct ZipWriter<W: Write> {
    out: W,
    central: Vec<u8>,
    pub offset: u64,
    pub count: usize,
    time: u16,
    date: u16,
}

impl<W: Write> ZipWriter<W> {
    pub fn new(out: W, (time, date): (u16, u16)) -> ZipWriter<W> {
        ZipWriter { out, central: Vec::new(), offset: 0, count: 0, time, date }
    }

    pub fn add(&mut self, name: &str, data: &[u8]) -> Result<()> {
        if self.count >= 0xffff || self.offset + data.len() as u64 >= 0xffff_ffff {
            bail!("the export is too large for one zip (65535 files / 4 GB): export a group at a time");
        }
        let nm = name.as_bytes();
        let crc = crc32(data);
        let n = data.len() as u32;
        let (t, d) = (self.time, self.date);
        let mut h = Vec::with_capacity(30 + nm.len());
        h.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        h.extend_from_slice(&20u16.to_le_bytes());
        h.extend_from_slice(&0x0800u16.to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(&t.to_le_bytes());
        h.extend_from_slice(&d.to_le_bytes());
        h.extend_from_slice(&crc.to_le_bytes());
        h.extend_from_slice(&n.to_le_bytes());
        h.extend_from_slice(&n.to_le_bytes());
        h.extend_from_slice(&(nm.len() as u16).to_le_bytes());
        h.extend_from_slice(&0u16.to_le_bytes());
        h.extend_from_slice(nm);
        self.out.write_all(&h)?;
        self.out.write_all(data)?;
        let c = &mut self.central;
        c.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&20u16.to_le_bytes());
        c.extend_from_slice(&0x0800u16.to_le_bytes());
        c.extend_from_slice(&0u16.to_le_bytes());
        c.extend_from_slice(&t.to_le_bytes());
        c.extend_from_slice(&d.to_le_bytes());
        c.extend_from_slice(&crc.to_le_bytes());
        c.extend_from_slice(&n.to_le_bytes());
        c.extend_from_slice(&n.to_le_bytes());
        c.extend_from_slice(&(nm.len() as u16).to_le_bytes());
        c.extend_from_slice(&[0u8; 12]);
        c.extend_from_slice(&(self.offset as u32).to_le_bytes());
        c.extend_from_slice(nm);
        self.offset += 30 + nm.len() as u64 + data.len() as u64;
        self.count += 1;
        Ok(())
    }

    /// Writes the central directory; the total size of the file.
    pub fn finish(mut self) -> Result<u64> {
        self.out.write_all(&self.central)?;
        let mut e = Vec::with_capacity(22);
        e.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        e.extend_from_slice(&[0u8; 4]);
        e.extend_from_slice(&(self.count as u16).to_le_bytes());
        e.extend_from_slice(&(self.count as u16).to_le_bytes());
        e.extend_from_slice(&(self.central.len() as u32).to_le_bytes());
        e.extend_from_slice(&(self.offset as u32).to_le_bytes());
        e.extend_from_slice(&[0u8; 2]);
        self.out.write_all(&e)?;
        self.out.flush()?;
        Ok(self.offset + self.central.len() as u64 + 22)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u16at(b: &[u8], o: usize) -> u16 {
        u16::from_le_bytes([b[o], b[o + 1]])
    }

    fn u32at(b: &[u8], o: usize) -> u32 {
        u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
    }

    #[test]
    fn crc_of_the_check_string() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn dos_fields() {
        assert_eq!(dos_time(2026, 11, 1, 12, 30, 45), ((12 << 11) | (30 << 5) | 22, (46 << 9) | (11 << 5) | 1));
    }

    #[test]
    fn the_archive_can_be_walked_from_its_end() {
        let mut out = Vec::new();
        let mut z = ZipWriter::new(&mut out, (1, 2));
        z.add("a.txt", b"hello").unwrap();
        z.add("dir/b\u{e9}.bin", &[1, 2, 3]).unwrap();
        let total = z.finish().unwrap();
        assert_eq!(total as usize, out.len());
        let e = out.len() - 22;
        assert_eq!(u32at(&out, e), 0x0605_4b50);
        assert_eq!(u16at(&out, e + 10), 2);
        let (cd_size, cd_at) = (u32at(&out, e + 12) as usize, u32at(&out, e + 16) as usize);
        assert_eq!(cd_at + cd_size, e);
        // First central record -> first local header -> data.
        assert_eq!(u32at(&out, cd_at), 0x0201_4b50);
        assert_eq!(u16at(&out, cd_at + 8), 0x0800, "UTF-8 names");
        assert_eq!(u16at(&out, cd_at + 10), 0, "stored");
        let n1 = u16at(&out, cd_at + 28) as usize;
        assert_eq!(&out[cd_at + 46..cd_at + 46 + n1], b"a.txt");
        assert_eq!(u32at(&out, cd_at + 42), 0);
        assert_eq!(u32at(&out, 0), 0x0403_4b50);
        assert_eq!(&out[30..35], b"a.txt");
        assert_eq!(&out[35..40], b"hello");
        assert_eq!(u32at(&out, 14), crc32(b"hello"));
        // Second record points behind the first entry.
        let c2 = cd_at + 46 + n1;
        let off2 = u32at(&out, c2 + 42) as usize;
        assert_eq!(off2, 40);
        assert_eq!(u16at(&out, off2 + 26) as usize, "dir/b\u{e9}.bin".len());
    }

    #[test]
    fn too_many_entries_are_refused() {
        let mut z = ZipWriter::new(Vec::new(), (0, 0));
        z.count = 0xffff;
        let e = z.add("x", b"").unwrap_err().to_string();
        assert!(e.contains("65535 files / 4 GB"), "{e}");
    }
}
