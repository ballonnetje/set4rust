//! `CVehicleInfoMgr` (0x597530): the vehicle table at .data 0x12dbce8, one 0x5c-byte record per
//! `[race · 6 + type]`. The exe's defaults (the list at .rdata 0x101b5e8: `{type, race,
//! record}`, 20 entries, copied in with the "defined" flag +0x58) are overwritten by
//! `VehicleInfo.xml` (0x5975b0): Boards +0x00, Ironbars +0x04, WalkSteps +0x2c, Hitpoints / 2
//! +0x30, Armor +0x34, Damage +0x38, ReadyToFireDelay +0x4c, MaxAmmo +0x54.

use s4_formats::gamedata::GameData;

/// Races per table (`[race · 6 + type]`, races 0..=4, types 0..=5).
pub const RACES: usize = 5;
pub const TYPES: usize = 6;

/// One record (the 23 dwords, names where their use is known).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VehicleInfo {
    /// +0x00 / +0x04: building costs (the build record's +0x6c / +0x6e, 0x594750).
    pub boards: i32,
    pub ironbars: i32,
    /// +0x08: obj type (2 ship, 4 land vehicle).
    pub obj_type: i32,
    /// +0x0c: the vehicle type.
    pub kind: i32,
    pub x10: i32,
    /// +0x14: the walker kind (3 `CWalkingCatapult`, 4 `CWalkingShip`).
    pub walker: i32,
    /// +0x18 / +0x1c / +0x20: gfx jobs (+0x18 the move job, `CVehicle` 0x595150).
    pub job: i32,
    pub job2: i32,
    pub job3: i32,
    pub x24: i32,
    /// +0x28: entity flags set when finished (0x594830).
    pub flags: u32,
    /// +0x2c `WalkSteps`.
    pub walk_steps: i32,
    /// +0x30 hit points (`Hitpoints` / 2), +0x34 armor, +0x38 damage.
    pub hitpoints: i32,
    pub armor: i32,
    pub damage: i32,
    pub x3c: i32,
    pub x40: i32,
    pub x44: i32,
    pub x48: i32,
    /// +0x4c `ReadyToFireDelay`.
    pub fire_delay: i32,
    pub x50: i32,
    /// +0x54 `MaxAmmo`.
    pub max_ammo: i32,
    /// +0x58: a default record exists.
    pub defined: bool,
}

impl VehicleInfo {
    const fn from(r: [i32; 23]) -> VehicleInfo {
        VehicleInfo {
            boards: r[0],
            ironbars: r[1],
            obj_type: r[2],
            kind: r[3],
            x10: r[4],
            walker: r[5],
            job: r[6],
            job2: r[7],
            job3: r[8],
            x24: r[9],
            flags: r[10] as u32,
            walk_steps: r[11],
            hitpoints: r[12],
            armor: r[13],
            damage: r[14],
            x3c: r[15],
            x40: r[16],
            x44: r[17],
            x48: r[18],
            fire_delay: r[19],
            x50: r[20],
            max_ammo: r[21],
            defined: true,
        }
    }
}

/// The defaults list at .rdata 0x101b5e8 (type, race, the record's 23 dwords).
const DEFAULTS: [(usize, usize, [i32; 23]); 20] = {
    const WAR: [i32; 15] = [5, 1, 2, 1, 10, 4, 8, 1, 2, 6, 8192, 6, 250, 2, 50];
    const fn war(x: [i32; 8]) -> [i32; 23] {
        let mut r = [0; 23];
        let mut i = 0;
        while i < 15 {
            r[i] = WAR[i];
            i += 1;
        }
        while i < 23 {
            r[i] = x[i - 15];
            i += 1;
        }
        r
    }
    const FERRY: [i32; 23] = [5, 1, 2, 2, 11, 4, 17, 10, 11, 6, 8192, 6, 75, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const TRANSPORT: [i32; 23] = [5, 1, 2, 3, 11, 4, 26, 19, 20, 6, 16384, 8, 75, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const CART: [i32; 23] = [5, 1, 4, 5, 9, 3, 37, 37, 39, 6, 16384, 8, 75, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    const fn cat(x: [i32; 8]) -> [i32; 23] {
        let base = [5, 1, 4, 4, 8, 3, 28, 28, 29, 0, 270336, 8, 125, 2, 50];
        let mut r = [0; 23];
        let mut i = 0;
        while i < 15 {
            r[i] = base[i];
            i += 1;
        }
        while i < 23 {
            r[i] = x[i - 15];
            i += 1;
        }
        r
    }
    [
        (1, 0, war([2, 85, 0, 2, 10, 2, 40, 0])),
        (1, 1, war([3, 85, 0, 2, 10, 0, 0, 0])),
        (1, 2, war([4, 85, 0, 2, 10, 17, 40, 0])),
        (1, 4, war([10, 85, 0, 2, 10, 40, 40, 0])),
        (2, 0, FERRY),
        (2, 1, FERRY),
        (2, 2, FERRY),
        (2, 4, FERRY),
        (3, 0, TRANSPORT),
        (3, 1, TRANSPORT),
        (3, 2, TRANSPORT),
        (3, 4, TRANSPORT),
        (4, 0, cat([2, 85, 92, 5, 28, 2, 40, 0])),
        (4, 1, cat([6, 0, 0, 9, 28, 0, 0, 0])),
        (4, 2, cat([4, 85, 92, 13, 28, 17, 40, 0])),
        (4, 4, cat([10, 85, 92, 13, 28, 40, 40, 0])),
        (5, 0, CART),
        (5, 1, CART),
        (5, 2, CART),
        (5, 4, CART),
    ]
};

/// The table, `[race · 6 + type]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VehicleRules {
    pub info: Vec<VehicleInfo>,
    /// The vehicles' gfx (`Gfx\<30 + race>.gfx`): per `[race · 6 + type]` the type's first job
    /// (its `.sil` entry, set by the ctor 0x593f60 as +0x26), and per race the frame count of
    /// direction 2 of every job (`sub_61FE0(race + 0x1e, job, 2)`, +0x61).
    pub first_job: Vec<u16>,
    pub frames: Vec<Vec<u8>>,
    /// `Gfx.gfx` direction-0 frame counts per job: the effects' durations (`CEffects` 0x4e2190;
    /// the gfx files 4 and 6 agree on the ones the vehicles use).
    pub effect_frames: Vec<u8>,
}

impl Default for VehicleRules {
    /// The exe's defaults only.
    fn default() -> Self {
        let mut info = vec![VehicleInfo::default(); RACES * TYPES];
        for (t, r, rec) in DEFAULTS {
            info[r * TYPES + t] = VehicleInfo::from(rec);
        }
        VehicleRules { info, first_job: vec![0; RACES * TYPES], frames: vec![Vec::new(); RACES], effect_frames: Vec::new() }
    }
}

impl VehicleRules {
    /// The defaults, then `VehicleInfo.xml`.
    pub fn new(data: &GameData) -> VehicleRules {
        let mut v = VehicleRules::default();
        for x in &data.vehicles {
            let (r, t) = (x.race as usize, x.vehicle as usize);
            if r >= RACES || t >= TYPES {
                continue;
            }
            let i = &mut v.info[r * TYPES + t];
            if let Some(b) = x.boards {
                i.boards = b;
            }
            if let Some(b) = x.ironbars {
                i.ironbars = b;
            }
            if let Some(h) = x.hitpoints {
                i.hitpoints = h / 2;
            }
            if let Some(a) = x.armor {
                i.armor = a;
            }
            if let Some(d) = x.damage {
                i.damage = d;
            }
            if let Some(d) = x.ready_to_fire_delay {
                i.fire_delay = d;
            }
            if let Some(m) = x.max_ammo {
                i.max_ammo = m;
            }
            if let Some(w) = x.walk_steps {
                i.walk_steps = w;
            }
        }
        v
    }

    /// Read the first jobs and frame counts from the vehicle gfx files (a missing file, as the
    /// Dark Tribe's 33.gfx, leaves zeros).
    pub fn load_gfx(&mut self) {
        if let Ok(g) = s4_formats::gfx::GfxFile::open_default(6) {
            self.effect_frames = (0..g.job_count()).map(|j| g.job_frames(j, 0).map_or(0, |r| r.len().min(255) as u8)).collect();
        }
        for race in 0..RACES {
            let Ok(g) = s4_formats::gfx::GfxFile::open_default(30 + race as u32) else { continue };
            for t in 0..TYPES {
                let job = g.job_groups().get(t).copied().unwrap_or(0) as usize;
                self.first_job[race * TYPES + t] = if g.job_dirs(job).is_some() { job as u16 } else { 0 };
            }
            self.frames[race] = (0..g.job_count()).map(|j| g.job_frames(j, 2).map_or(0, |r| r.len().min(255) as u8)).collect();
        }
    }

    /// `CVehicle` ctor 0x593f60: the type's first gfx job.
    pub fn first_job(&self, race: u8, kind: u8) -> u16 {
        self.first_job.get(race as usize * TYPES + kind as usize).copied().unwrap_or(0)
    }

    /// `sub_61FE0(race + 0x1e, job, 2)`: the frames of direction 2 of `job`.
    pub fn frame_count(&self, race: u8, job: u16) -> u8 {
        self.frames.get(race as usize).and_then(|f| f.get(job as usize)).copied().unwrap_or(0)
    }

    /// The record of `race` / `kind` (zero for a missing one, as the original's table).
    pub fn get(&self, race: u8, kind: u8) -> VehicleInfo {
        self.info.get(race as usize * TYPES + kind as usize).copied().unwrap_or_default()
    }
}
