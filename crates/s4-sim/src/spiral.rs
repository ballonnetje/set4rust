//! `CSpiralOffsets` (MapObjects/CSpiralOffsets.cpp): the order in which spiral searches visit the
//! nodes around a centre. `m_sOffsets` is a table of `{i8 dx, i8 dy, u8 radius, u8 direction}`;
//! `DeltaX/DeltaY/Radius/Direction(i)` read entry `i`; `First(r)` / `Last(r)` come from a second
//! table (see [`last`]), and `CSpiralWalk(x, y, r)` visits entries `0..=Last(r)` that lie in the
//! world.
//!
//! The order is not a simple hex ring (entries are grouped by a rounded distance, ring 7 has 48
//! entries), so the table cannot be regenerated: it is read from the player's `S4_Main.exe` at
//! run time (.data, VA 0x1169968; 19825 entries up to radius 74; the accessor pattern
//! `table+0..+3` is referenced 83/86/10/4 times from .text). That this is `m_sOffsets` is inferred
//! (direction bytes, reference counts); a second table without direction bytes at .rdata
//! 0x101bdb8 has far fewer references.

use std::sync::LazyLock;

/// `m_sOffsets` and the `{first, last}` table of [`last`].
struct Tables {
    offsets: Vec<u8>,
    rings: Vec<u8>,
}

/// Read once. The game does not start without its install, so a missing one is a bug here.
static TABLES: LazyLock<Tables> = LazyLock::new(|| {
    let read = |va, len| s4_formats::exe::table(va, len).unwrap_or_else(|| panic!("the spiral tables: no S4_Main.exe in {}", s4_formats::game_dir().display()));
    Tables { offsets: read(0x1169968, LEN * 4), rings: read(0x117cf38, 76 * 4) }
});

/// Number of entries available.
pub const LEN: usize = 19825;
/// Largest radius in the table.
pub const MAX_RADIUS: u8 = 74;

/// `(dx, dy, radius, direction)` of entry `i` (`i < LEN`).
pub fn entry(i: usize) -> (i8, i8, u8, u8) {
    let b = &TABLES.offsets[i * 4..i * 4 + 4];
    (b[0] as i8, b[1] as i8, b[2], b[3])
}

/// `CSpiralOffsets::DeltaX`.
pub fn dx(i: usize) -> i32 {
    TABLES.offsets[i * 4] as i8 as i32
}

/// `CSpiralOffsets::DeltaY`.
pub fn dy(i: usize) -> i32 {
    TABLES.offsets[i * 4 + 1] as i8 as i32
}

/// `CSpiralOffsets::Radius`.
pub fn radius(i: usize) -> u8 {
    TABLES.offsets[i * 4 + 2]
}

/// `CSpiralOffsets::Last(radius)` (`[r*4 + 0x117cf3a]`); radii beyond the table give the
/// sentinel's `LEN - 1`. The rings are `{u16 first, u16 last}` per radius 0..=75 (release .rdata
/// VA 0x117cf38; entry 75 is the end sentinel `(19825, 19824)`). The release reads them directly:
/// `CSpiralWalk(x, y, r)` (ctor 0x431210) stops at `last[r]`, and `CFogging`'s entity sight
/// (0x5ae100) walks `0..=last[r]` and the rings `last[j-1]..=last[j]`. `last[r]` is *not* the
/// last entry with radius byte `<= r`: from radius 7 on, a ring's last few entries (radius byte
/// r, e.g. 169..=174 for 7) belong to the walk of `r + 1` (`last[7] = 168`, `first[8] = 175`).
pub fn last(radius: u8) -> usize {
    let r = (radius as usize).min(75);
    u16::from_le_bytes([TABLES.rings[r * 4 + 2], TABLES.rings[r * 4 + 3]]) as usize
}

/// `CSpiralOffsets::First(radius)` (`[r*4 + 0x117cf38]`, accessor 0x4330a0).
pub fn first(radius: u8) -> usize {
    let r = (radius as usize).min(75);
    u16::from_le_bytes([TABLES.rings[r * 4], TABLES.rings[r * 4 + 1]]) as usize
}

/// `CSpiralWalk`: in-world nodes around `(x, y)` up to `radius`, in the original's order.
pub fn walk(grid: crate::grid::Grid, x: i32, y: i32, radius: u8) -> impl Iterator<Item = (i32, i32)> {
    (0..=last(radius)).map(move |i| (x + dx(i), y + dy(i))).filter(move |&(px, py)| grid.in_world(px, py))
}

/// `Grid::Distance(dx, dy)` (globals.cpp): hex distance in the game's skewed coordinates —
/// `|dx| + |dy|` when the signs differ, else `max(|dx|, |dy|)`.
pub fn grid_distance(dx: i32, dy: i32) -> i32 {
    let (ax, ay) = (dx.abs(), dy.abs());
    if (dx ^ dy) < 0 { ax + ay } else { ax.max(ay) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_shape() {
        if !s4_formats::game_dir().join("S4_Main.exe").is_file() {
            return;
        }
        assert_eq!(entry(0), (0, 0, 0, 0));
        assert_eq!(entry(1), (1, 0, 1, 0));
        assert_eq!((first(7), last(7), first(8)), (127, 168, 175));
        assert_eq!((last(15), last(40), last(50)), (810, 5754, 9030));
        assert_eq!(radius(LEN - 1), 74);
        assert_eq!(last(75), LEN - 1);
        // Every walk ends on an entry of its radius; rings only grow.
        assert!((1..=MAX_RADIUS).all(|r| last(r) > last(r - 1) && radius(last(r)) == r && radius(first(r)) == r));
        assert_eq!((grid_distance(3, -2), grid_distance(3, 2), grid_distance(-1, -4)), (5, 3, 4));
    }
}
