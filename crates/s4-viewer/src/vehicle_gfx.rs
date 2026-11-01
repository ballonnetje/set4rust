//! Vehicles as the original draws them (own RE of the shipped exe; decompiles in
//! `tools/re/viewer-2026-10-21/vehicles/` and `tools/re/viewer-2026-10-22/vehicles/`).
//!
//! **GetGfxInfo** (entity vtable +0x14): `CWheeler` 0x59a5c0 (catapults through 0x560c60), `CCart`
//! 0x55ba20, `CShip` 0x590590 (ferry 0x58ec70, transport ship 0x591f20, war ships 0x5924c0). Each
//! first runs the draw-time update 0x594db0 ([`update`]), then fills the gfx info:
//! * the image of file `race + 0x1e` (`Gfx\30..34`): job `+0x26` (0 when past the file's job
//!   count), direction `+0x40`, frame `+0x24` (0 when not below direction 2's frame count);
//!   a cart that carries nothing (0x55ca10 false) shows job `+0x26 + 1`;
//! * while turning (`+0x5d` non-zero) vtbl+0xb8 instead (0x594b50; carts 0x55bbd0): [`TURN`];
//! * team colour `0x149b5e8[player]`, renderer type 4 (land) / 2 (ships: the deferred list),
//!   direction `+0x40` and distance `+0x41` for the glide, drawn when `flags & 0x80100 == 0x100`;
//! * BIRTH (0x1000000, under construction): the clip `0xfffe − (+0x74)·(+0x70)` (gfx info
//!   +0x5c, which the vehicle blitter 0x670be0 passes on `>> 8` like `BlitBuilding`); a cart
//!   also shows job 0x2e (0 when the file has no more jobs), direction 2, frame 0. A ship is
//!   clipped while not READY (0x2000000);
//! * the icons (gfx info +0x16, +0x15, +0x14), drawn by the vehicle blitter right after the
//!   vehicle at its anchor: selected (0x200) 0x49 (ships 0x1b), "other" (0x400) 0x5a (ships
//!   0x59); a selected catapult adds ammo ([`ammo_icon`]) and health ([`health_icon`]), a
//!   selected ferry or war ship / an "other" transport ship the health from 0x101b468, a war
//!   ship the ammo from 0x101a6c4.
//!
//! A moving ship (`+0x5c & 8` clear) is drawn as job `+0x1c` frame 0 with job `+0x18` at frame
//! `+0x24` over it (0x462420); a standing one as job `+0x26` (`+0x1c` for `+0x18`), frame 0.

/// `CVehicle`'s turn table (.rdata 0x101b4a8, 0x14 bytes per entry, three dwords read) by the
/// 16-way direction `+0x5e & 0x7f`: (turn job, direction, frame). Turn job 0 is the vehicle
/// record's job `+0x1c`, else `+0x20 + turn job − 1` (0x594b50 / 0x55bbd0). Dump:
/// `tools/re/viewer-2026-10-22/vehicles/turn_table.py`.
pub const TURN: [(u8, u8, u8); 16] = [
    (0, 0, 0),
    (5, 2, 0),
    (0, 1, 0),
    (6, 2, 0),
    (6, 2, 1),
    (6, 2, 2),
    (0, 2, 0),
    (1, 2, 0),
    (0, 3, 0),
    (2, 2, 0),
    (0, 4, 0),
    (3, 2, 0),
    (3, 2, 1),
    (3, 2, 2),
    (0, 5, 0),
    (4, 2, 0),
];

/// The vehicle type of a cart (`VEHICLE_CART`).
pub const CART: u16 = 5;
/// Selection icons (`Gfx.gfx`): land vehicles selected / "other", ships selected / "other".
pub const SELECTED: u8 = 0x49;
pub const OTHER: u8 = 0x5a;
pub const SHIP_SELECTED: u8 = 0x1b;
pub const SHIP_OTHER: u8 = 0x59;
/// The cart under construction: job 0x2e, direction 2, frame 0 (0x55ba20).
pub const CART_BIRTH_JOB: usize = 0x2e;

/// Catapult health icons (0x101a3fc), ammo icons (0x101a41c), war ship ammo (0x101a6c4), ship
/// health (0x101b468). Dump: `tools/re/viewer-2026-10-22/vehicles/icon_tables.py`.
const CATAPULT_HEALTH: [u8; 8] = [65, 66, 67, 68, 69, 70, 71, 72];
const CATAPULT_AMMO: [u8; 5] = [60, 61, 62, 63, 64];
const WARSHIP_AMMO: [u8; 5] = [76, 77, 78, 79, 80];
const SHIP_HEALTH: [u8; 8] = [81, 82, 83, 84, 85, 86, 87, 88];

/// What 0x594db0 changes: +0x5d turn steps left, +0x5e the 16-way direction (bit 7 clockwise),
/// +0x24 frame, +0x60 half frames.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Anim {
    pub turn: u8,
    pub dir16: u8,
    pub frame: u8,
    pub half: u8,
}

/// `CVehicle` 0x594db0 for `elapsed` ticks: the turn takes a 1/16 per tick first; what is left
/// plays an animation (task 0x10) or, walking / standing (6 / 0x11) unless the last step was
/// none (`+0x5c & 8`), the walk cycle (flag 0x40000: at half speed through `+0x60`). `frames` is
/// `+0x61`, the direction-2 frame count of the job taken (0x595150; the ctor 0x593f60).
pub fn update(a: &mut Anim, task: u8, step: u8, flags: u32, frames: u8, mut elapsed: u32) {
    if elapsed == 0 {
        return;
    }
    if a.turn != 0 {
        let k = elapsed.min(a.turn as u32);
        a.turn -= k as u8;
        let d = if a.dir16 & 0x80 != 0 { k as i32 } else { -(k as i32) };
        a.dir16 = (((a.dir16 & 0x7f) as i32 + d) & 0xf) as u8 | (a.dir16 & 0x80);
        elapsed -= k;
        if elapsed == 0 {
            return;
        }
    }
    let n = frames as u32;
    if n == 0 {
        return;
    }
    match task {
        0x10 => a.frame = ((a.frame as u32 + elapsed) % n) as u8,
        6 | 0x11 if step & 8 == 0 => {
            if flags & 0x4_0000 != 0 {
                a.half = ((a.half as u32 + elapsed) % (n * 2)) as u8;
                a.frame = a.half >> 1;
            } else {
                a.frame = ((a.frame as u32 + elapsed) % n) as u8;
            }
        }
        _ => {}
    }
}

/// Whether 0x594db0 moves the distance (`DecDistance(elapsed · 256 / +0x62)`) for this state.
pub fn moves(task: u8, step: u8) -> bool {
    matches!(task, 6 | 0x11) && step & 8 == 0
}

/// The turn sprite's (job, direction, frame) for the 16-way direction `dir16` (0x594b50 /
/// 0x55bbd0): `jobs` = the record's +0x1c / +0x20; an empty cart takes the job + 8 (turn job 0:
/// + 1). The job and frame are bounded by the caller (job count, direction 2's frames).
pub fn turn_image(dir16: u8, jobs: (u16, u16), empty_cart: bool) -> (u16, u8, u8) {
    let (tj, dir, frame) = TURN[(dir16 & 0x7f) as usize & 0xf];
    let job = if tj == 0 { jobs.0 } else { jobs.1.wrapping_add(tj as u16 - 1) };
    let job = match (empty_cart, tj) {
        (false, _) => job,
        (true, 0) => job.wrapping_add(1),
        (true, _) => job.wrapping_add(8),
    };
    (job, dir, frame)
}

/// `7 − health · 7 / max` (unsigned) into a table of 8, clamped (0x560c10); none without a max.
fn health_index(health: u8, max: i32) -> Option<usize> {
    (max > 0).then(|| (7 - (health as u32 * 7 / max as u32) as i32).clamp(0, 7) as usize)
}

/// A catapult's health icon (+0x14, 0x560c10): 65..=72 (65 = full).
pub fn health_icon(health: u8, max: i32) -> u8 {
    health_index(health, max).map_or(0, |i| CATAPULT_HEALTH[i])
}

/// A ship's health icon (+0x14, 0x58ec70 / 0x591f20 / 0x5924c0): 81..=88.
pub fn ship_health_icon(health: u8, max: i32) -> u8 {
    health_index(health, max).map_or(0, |i| SHIP_HEALTH[i])
}

/// The ammo icon (+0x15) of a catapult (0x560b80) or war ship (0x592430): `4 − ammo · 4 / max`
/// clamped into the table. Vikings (race 1) shoot mana: ammo = `min(mana, cost · 40)` of
/// `cost · 40`, `cost` = `VikingWarmachineShotCost` (0x11abdc8, at least 1; written by 0x421100);
/// the others carry `+0x63` of `MaxAmmo` (record +0x54).
pub fn ammo_icon(warship: bool, race: u8, mana: i32, shot_cost: i32, cargo: u8, max_ammo: i32) -> u8 {
    let level = if race == 1 {
        let full = shot_cost.max(1) as u32 * 0x28;
        (mana.max(0) as u32).min(full) * 4 / full
    } else {
        if max_ammo <= 0 {
            return 0;
        }
        cargo as u32 * 4 / max_ammo as u32
    };
    let i = (4 - level as i32).clamp(0, 4) as usize;
    if warship { WARSHIP_AMMO[i] } else { CATAPULT_AMMO[i] }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_turns_first_then_animates() {
        // Turning clockwise from 2 by 3, then walking a 12-frame cycle.
        let mut a = Anim { turn: 3, dir16: 0x80 | 3, frame: 5, half: 0 };
        update(&mut a, 6, 0, 0x100, 12, 2);
        assert_eq!(a, Anim { turn: 1, dir16: 0x85, frame: 5, half: 0 });
        update(&mut a, 6, 0, 0x100, 12, 10);
        assert_eq!(a, Anim { turn: 0, dir16: 0x86, frame: 2, half: 0 });
        // Counter-clockwise wraps below 0; a blocked step (+0x5c & 8) keeps the frame.
        let mut b = Anim { turn: 2, dir16: 1, frame: 4, half: 0 };
        update(&mut b, 6, 0x88, 0, 12, 5);
        assert_eq!(b, Anim { turn: 0, dir16: 15, frame: 4, half: 0 });
        // Half speed (flag 0x40000) through +0x60.
        let mut c = Anim { half: 23, frame: 11, ..Anim::default() };
        update(&mut c, 0x11, 0, 0x4_0000, 12, 3);
        assert_eq!((c.half, c.frame), (2, 1));
    }

    #[test]
    fn turn_sprites_and_icons() {
        assert_eq!(turn_image(0x85, (40, 50), false), (55, 2, 2));
        assert_eq!(turn_image(6, (40, 50), false), (40, 2, 0));
        assert_eq!(turn_image(0x81, (40, 50), true), (62, 2, 0));
        assert_eq!(turn_image(8, (40, 50), true), (41, 3, 0));
        assert_eq!((health_icon(30, 30), health_icon(1, 30), health_icon(0, 0)), (65, 72, 0));
        assert_eq!(ship_health_icon(15, 30), 85);
        // A Roman catapult with 3 of 8 shots, a Viking one with 300 mana at cost 15 (600 = full).
        assert_eq!(ammo_icon(false, 0, 0, 15, 3, 8), 63);
        assert_eq!(ammo_icon(false, 1, 300, 15, 0, 8), 62);
        assert_eq!(ammo_icon(true, 1, 9999, 15, 0, 8), 76);
    }
}
