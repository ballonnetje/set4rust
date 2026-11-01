//! Probe GUI images whose RLE stream does not end at the next image.
use s4_formats::gfx::GfxFile;
fn main() -> anyhow::Result<()> {
    let n: u32 = std::env::args().nth(1).map(|s| s.parse().unwrap()).unwrap_or(0);
    let f = GfxFile::open_default(n)?;
    let mut starts: Vec<usize> = (0..f.image_count()).map(|i| f.header(i).unwrap().data_offset - 12).collect();
    starts.sort(); starts.dedup();
    for i in 0..f.image_count() {
        let ix = f.decode_indexed(i)?;
        let h = ix.header;
        let hs = h.data_offset - 12;
        let next = starts.iter().find(|&&s| s > hs).copied();
        let gap = next.map(|s| s - h.data_offset);
        let ok = gap == Some(ix.stream_len);
        if !ok || !ix.terminated {
            println!("img {i}: {}x{} left {} top {} flags {:#x} id {} stream {} gap {:?} term {} raw? {}", h.width, h.height, h.left, h.top, h.flags, h.cache_id, ix.stream_len, gap, ix.terminated, gap == Some(h.width as usize * h.height as usize));
        }
    }
    Ok(())
}
