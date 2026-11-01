//! The node under a screen point, with heights: `SearchMapPoint` (0x2f5d550), used by
//! `IGfxEngine`'s screen→map queries.
//!
//! The original works in the screen coordinates of the current scroll position; this works in
//! zoom-1 world px (screen = world − view origin, then the zoom), with `W` = 24, `H` = 12:
//!
//! 1. First guess, ignoring heights: row `y = (wy + H/2)·2 / W` (camera mode 1 doubles the
//!    vertical distance), column from the row's horizontal offset.
//! 2. Up to five vertical corrections. Each step projects the guess with
//!    `ConvertMapIndexToScreenPosition` (the 5-output variant 0x2f5d0f0: the node's anchor plus how
//!    far its "cell" reaches up and down, half the row height minus the height difference to the
//!    neighbours above/below) and compares with the point:
//!    * `n = ⌊⌊dy / ((W/32)·1.65)⌋ / 7⌋ · 0.75` rows off (`dy` = point − anchor);
//!    * if the point is far outside the cell (`|n| > 0` and outside it vertically): move `n` rows
//!      and `n/2` columns;
//!    * else if it is more than `W/2 + 1` px off sideways (and `n ≤ 1`): move `round(dx / W)`
//!      columns;
//!    * else if it is still outside the cell vertically: move `n` rows (at least one, towards the
//!      point) and use up one of the five steps;
//!    * else done.
//! 3. Of the node and its six neighbours, take the one whose anchor (the 3-output variant
//!    0x2f5d3a0, i.e. where objects stand) is closest to the point (squared distance; the first of
//!    equal ones in the order self, x−1, x+1, y−1, (x−1, y−1), (x+1, y+1), y+1).

use s4_formats::gfx::landscape::{DEFAULT_TRIANGLE_SIZE, height_offset_fixed};

const W: i32 = 24;
const H: i32 = 12;

fn hoff(h: u8) -> i32 {
    height_offset_fixed(h, DEFAULT_TRIANGLE_SIZE)
}

/// `ConvertMapIndexToScreenPosition` (0x2f5d3a0) in world px: the object anchor of node (x, y).
fn anchor(height: &dyn Fn(i32, i32) -> u8, x: i32, y: i32) -> (i32, i32) {
    (W * x - (W / 2) * y, (((H * y) << 16) - hoff(height(x, y))) >> 16)
}

/// The 5-output variant (0x2f5d0f0): anchor (with the offset floored first) and the cell's
/// extent above and below it.
fn cell(height: &dyn Fn(i32, i32) -> u8, size: i32, x: i32, y: i32) -> (i32, i32, i32, i32) {
    let off = |x: i32, y: i32| hoff(height(x, y)) >> 16;
    let own = off(x, y);
    let sx = W * x - (W / 2) * y;
    let sy = H * y - own;
    let up = if x > 0 && y > 0 { (H - (own - off(x, y - 1).min(off(x - 1, y - 1)))) / 2 } else { H / 2 };
    let down = if x < size - 1 && y < size - 1 { (H - (own - off(x + 1, y + 1).max(off(x, y + 1)))) / 2 } else { H / 2 };
    (sx, sy, up, down)
}

/// The node at world point (wx, wy), or `None` outside the map.
pub fn search_map_point(size: usize, height: &dyn Fn(i32, i32) -> u8, wx: i32, wy: i32) -> Option<(i32, i32)> {
    let size = size as i32;
    let mut y = (wy + H / 2) * 2 / W;
    let mut x = (wx + (W / 2) * y + W / 2).div_euclid(W);
    if x < 0 || y < 0 || x >= size || y >= size {
        return None;
    }
    let half = W / 2 + 1;
    let mut steps = 5;
    loop {
        let (sx, sy, up, down) = cell(height, size, x, y);
        let v11 = ((wy - sy) as f32 / ((W as f32 / 32.0) * 1.65)) as i32;
        let mut n = ((v11 / 7) as f32 * 0.75) as i32;
        let d = (wx - sx) as f32 / W as f32;
        let v19 = if wx - sx >= 0 { d + 0.5 } else { d - 0.5 } as i32;
        let inside = wy >= sy - up && wy < sy + down;
        let mut done = true;
        if n.abs() <= 0 || inside {
            if n <= 1 && v19 != 0 && (wx < sx - half || wx >= sx + half) {
                x += v19;
                done = false;
            } else if !(wy >= sy - up && wy <= sy + down) {
                steps -= 1;
                if steps >= 0 {
                    if n.abs() < 1 {
                        n = if v11 >= 0 { 1 } else { -1 };
                    }
                    y += n;
                    done = false;
                }
            }
        } else {
            y += n;
            x += n / 2;
            done = false;
        }
        if (x | y) < 0 || x >= size || y >= size {
            return None;
        }
        if done || steps < 0 {
            break;
        }
    }
    let dist = |(ax, ay): (i32, i32)| (ay - wy) * (ay - wy) + (ax - wx) * (ax - wx);
    let mut best = (dist(anchor(height, x, y)), 0, 0);
    let cands = [(-1, 0, x > 0), (1, 0, x < size - 1), (0, -1, y > 0), (-1, -1, x > 0 && y > 0), (1, 1, x < size - 1 && y < size - 1), (0, 1, y < size - 1)];
    for (dx, dy, ok) in cands {
        if ok {
            let dd = dist(anchor(height, x + dx, y + dy));
            if dd < best.0 {
                best = (dd, dx, dy);
            }
        }
    }
    Some((x + best.1, y + best.2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_map_picks_the_nearest_node() {
        let flat = |_: i32, _: i32| 0u8;
        for (x, y) in [(10, 10), (11, 10), (30, 31), (5, 40)] {
            let (wx, wy) = anchor(&flat, x, y);
            assert_eq!(search_map_point(64, &flat, wx, wy), Some((x, y)));
            assert_eq!(search_map_point(64, &flat, wx + 5, wy - 3), Some((x, y)));
        }
    }

    #[test]
    fn hill_is_followed() {
        // A hill of 40 units (~50 px) falling off over 15 nodes, steeper than most map terrain;
        // clicking a raised node's anchor must find it. (On slopes steeper than the row height the
        // original's heuristic can stop on a node in front, so does this port.)
        let hill = |x: i32, y: i32| {
            let d2 = ((x - 30).pow(2) + (y - 30).pow(2)) as f32;
            (40.0 * (-d2 / 60.0).exp()) as u8
        };
        for (x, y) in [(30, 30), (28, 31), (33, 27)] {
            let (wx, wy) = anchor(&hill, x, y);
            assert_eq!(search_map_point(64, &hill, wx, wy), Some((x, y)), "node {x},{y}");
        }
    }
}

#[cfg(test)]
mod real_map {
    use super::*;

    /// Picking the anchor of every node of a real map (skipped without the game data): the share
    /// that returns the node itself.
    #[test]
    fn anchors_of_a_real_map() {
        let path = s4_formats::game_dir().join("Map/Campaign/AO_maya1.map");
        let Ok(map) = s4_formats::map::MapFile::open(&path) else { return };
        let l = map.landscape().unwrap();
        let s = l.width as i32;
        let height = |x: i32, y: i32| l.nodes[(y.clamp(0, s - 1) * s + x.clamp(0, s - 1)) as usize].height;
        let (mut ok, mut n) = (0, 0);
        for y in 2..s - 2 {
            for x in 2..s - 2 {
                let (wx, wy) = anchor(&height, x, y);
                n += 1;
                ok += (search_map_point(s as usize, &height, wx, wy) == Some((x, y))) as u32;
            }
        }
        let share = ok as f64 / n as f64;
        eprintln!("AO_maya1: {ok}/{n} anchors pick their own node ({:.3}%)", share * 100.0);
        assert!(share > 0.99, "{share}");
    }
}
