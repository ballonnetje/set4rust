//! Sum of sprite bounding-box areas per gfx file (atlas budgeting).
use s4_formats::gfx::{self, GfxFile};
fn main() -> anyhow::Result<()> {
    let dir = s4_formats::game_dir().join("Gfx");
    for n in gfx::list_files(&dir)? {
        let f = GfxFile::open(&dir, n)?;
        let mut area = 0u64;
        for i in 0..f.image_count() {
            let h = f.header(i)?;
            area += h.width as u64 * h.height as u64;
        }
        println!("{n:>2} imgs {:>6} area {:>10} px = {:>7.1} MB RGBA, {:>6.1} MB R8, team {}", f.image_count(), area, area as f64 * 4.0 / 1e6, area as f64 / 1e6, f.has_team_colour());
    }
    Ok(())
}
