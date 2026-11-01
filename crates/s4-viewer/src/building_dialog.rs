//! Building selection dialogs: the dialogs 14..=24 and 43 a selected building shows, filled the
//! way the shipped exe fills them.
//!
//! # Pipeline (release addresses)
//! * Selecting a building (`CInputProcessor::RequestDialog` 0x4ee730, entity class 8) calls the
//!   role's vtable +0x58 `(building, refresh = 0)`. The role writes its *info object* (one global
//!   per code, below) and sends GUI event 0x25a; a refresh (`refresh = 1`, sent by the roles from
//!   their logic while the building has flag 0x200 "selected") sends 0x25c.
//! * 0x25a → `CreateUITab` (0x4841e0) closes the open context dialog and opens the dialog of the
//!   info object's code, running `ResetMainMenuForContext` (0x481110) first except for code 10.
//!   0x25c → the update switch 0x488e00 refills the open one.
//! * Codes, info objects and updates: 0 house (0x11aaf1c, D15, 0x4709c0), 1 site (0x11ab10c, D14,
//!   0x46daa0), 2 worker (0x11aacc0, D18, 0x49d2c0), 3 production (0x11aab2c, D19, 0x49f8e0),
//!   4 military (0x11aad0c, D16, 0x49a210), 5 trade (0x11aacd8, D17, 0x49c330), 6 storage
//!   (0x11aaac0, D23, 0x4ac390), 7 barrack (0x11ab2a4, D22, 0x46c230), 8 small temple (0x11aaa1c,
//!   D20, 0x4a5e40), 9 workshop (0x11aabf8, D24, 0x4a5100), 10 crush confirmation (0x11aa8b0, D43,
//!   0x4b0a40).
//! * Role → code (vtable +0x58 of each role class, from the RTTI vtables): simple 0x54f4b0 (type
//!   27 → 0, else 2), work-up and animal ranch 0x55a6d0 → 3, production 0x54d7b0 → 3, gather
//!   0x546f20 → 2, mine 0x54c060 → 3, farm 0x5467f0 → 2, lookout tower 0x547570 → 0, military and
//!   castle 0x54a6a0 → 4, barrack 0x5352f0 → 7, storage 0x5508d0 → 6, trading 0x5533f0 → 5,
//!   residence 0x54df70 → 0, small temple 0x54fe40 → 8, big temple 0x535880 → 0, eye catcher
//!   0x5460c0 → 0, workshop 0x559930 → 9, building site 0x543d10 → 1; dark temple, mushroom farm
//!   and manakopter hall have empty ones (no dialog).
//!
//! # Info objects
//! They are globals that persist between selections, and a role writes only some of their bytes,
//! so a dialog can show a byte an earlier building left (e.g. the farm role never writes +0xe;
//! D18's "+n" visibility reads the *house* object's +0xf, 0x49d35a; the eye catcher and residence
//! roles write the site object's +0xb / +0xc). [`Infos`] keeps them byte-for-byte for that reason.
//! Common header: +4 code (dword), +8 type, +9 race, +0xa worker in house, +0xb stop button
//! visible, +0xc building flag 0x1000 ("works"; the stop button shows pressed when clear), +0xd
//! work-area button, +0xe productivity % (0xff = none), +0xf / +0x10 the player's buildings of
//! the type under construction / finished (`CBuildingMgr` table 0x14ef620), +0x11 the worker type.
//!
//! # Buttons
//! The procedures map a click to a GUI event (see [`proc_event`]), which `CInputProcessor`
//! (handler table 0x4e96d0) turns into a net command: 0x193 work-area mode (0x4e9c50; the map
//! click then sends 0x1389 = [`s4_sim::Command::SetWorkingArea`], 0x4ead10), 0x194 crush request
//! (0x4eaab0 → 0x536850: the role's vtable +0x28 "may crush" and code 10), 0x195 the confirmation
//! answer (0x4eaad0: yes → 0x138a [`s4_sim::Command::CrushBuilding`] and deselect, no → the
//! building's dialog again), 0x196 stop/continue (0x4eabb0 → 0x138b
//! [`s4_sim::Command::SwitchBuilding`]), 0x197 site priority (0x4eac60 → 0x138c
//! [`s4_sim::Command::PrioritizeBuildingSite`]), 0x198 next building of the type (0x4e9e70),
//! D16's soldier buttons 0x1bb / 0x1bc (0x4eae00 / 0x4eaeb0 → 0x13a7 / 0x13a8
//! [`s4_sim::Command::FillTower`] / [`s4_sim::Command::EmptyTower`]), D17's trade target 0x199
//! (state 7, the map click 0x4eaf90 → 0x138d [`s4_sim::Command::SetTradeTarget`]) and D24's
//! cancel 0x1bf (0x4eeaa0 → 0x13a6 [`s4_sim::Command::CancelProduction`]). The sidebar buttons:
//! see [`sidebar_fill`].
//! While an update runs, the procedures ignore the radio buttons' code 3 (guards 0x15aa50d,
//! 0x15aabc1, 0x15aabc2), so a fill never sends a command; [`GuiState`] fills here send none.
//!
//! Verified against the exe: every update here is checked by `tests/bdlg_vectors.rs` against
//! vectors made by running the original update in an emulator
//! (`tools/re/viewer-2026-10-19/bdlg/vectors.py`).

use std::collections::HashMap;
use std::sync::OnceLock;

use s4_sim::{EntityData, ObjType, World};

use crate::guistate::GuiState;
use crate::population::offered;

/// The dialog of each info code: 0..=10 the building dialogs (`CreateUITab`, event 0x25a), 0xb..=0x11
/// the sidebars (opener 0x481260, event 0x25f; surface 3 at 198, 250): 0xb weapon smith D46, 0xc
/// tool smith D47, 0xd / 0xe trading D54 / D53, 0xf storage D50, 0x10 barracks D48, 0x11 workshop
/// D51.
pub const DIALOGS: [usize; 18] = [15, 14, 18, 19, 16, 17, 23, 22, 20, 24, 43, 46, 47, 54, 53, 50, 48, 51];
/// Info object sizes (their vtable +4 `GetSize`; 0x10: vtable 0x102f7d4 → 0x4e6640).
pub const SIZES: [usize; 18] = [20, 36, 24, 52, 60, 52, 40, 32, 28, 40, 12, 60, 16, 60, 52, 96, 24, 20];

/// The info objects by code (byte images of the globals at 0x11aa8b0..0x11ab4b8, from +0; +0..+3
/// is the vtable pointer and stays 0 here).
#[derive(Clone, Debug, PartialEq)]
pub struct Infos {
    pub obj: [[u8; 96]; 18],
    /// `IsLastTower` (0x11aa8b8, set by the crush request 0x536850).
    pub last_tower: bool,
    /// The local player's race: D16's procedure init (0x49b090) stores it at 0x15aabb8 for its
    /// picture, name and soldier images, D50's (0x468270) at 0x15aa4f4 for its good → link table.
    pub gui_race: u8,
    /// `CBuildingSiteRole +0x1c4`, the progress divisor 0x543c60 computes on its first call (the
    /// first time the site's dialog is shown), by building id.
    pub site_divisor: HashMap<u16, f32>,
    /// The vehicle dialogs' info object (0x11aa918, 0x104 bytes; codes 0x15 / 0x16 / 0x18, D41
    /// [`crate::vehicle_dialog`]).
    pub vehicle: Vec<u8>,
}

impl Default for Infos {
    fn default() -> Self {
        Infos { obj: [[0; 96]; 18], last_tower: false, gui_race: 0, site_divisor: HashMap::new(), vehicle: vec![0; VEHICLE_SIZE] }
    }
}

/// The vehicle dialogs' info code of a cart, a transport ship, a donkey, and the object's size.
pub const CART_CODE: usize = 0x15;
pub const TRANSPORT_CODE: usize = 0x16;
pub const DONKEY_CODE: usize = 0x18;
/// The donkey's settler type (`RequestDialog` 0x4ee730: type 0x3c → role vtbl+0x84).
pub const DONKEY: u16 = 0x3c;
const VEHICLE_SIZE: usize = 0x104;

/// The dialog of info `code`: [`DIALOGS`], and dialog 41 for the vehicle codes 0x15 / 0x16 /
/// 0x18, 42 for 0x17 (`CreateUITab` 0x4841e0).
pub fn dialog_of(code: usize) -> usize {
    match code {
        0x15 | 0x16 | 0x18 => crate::vehicle_dialog::DIALOG,
        0x17 => 42,
        c => DIALOGS.get(c).copied().unwrap_or(0),
    }
}

/// `CCart` vtbl+0x94 (0x55bdb0): the vehicle info object for a cart. A trade cart: +0xf8 = 0,
/// settlers 0 / 0 and its six slots into rows 0..6; a foundation cart: the settlers, the room
/// flag and one row per goods entry (rows beyond keep what they held). Code 0x15.
fn fill_cart(infos: &mut Infos, c: &CartFacts) {
    let o = &mut infos.vehicle;
    o[0xf8] = (!c.foundation || c.can_settle) as u8;
    o[0xf9] = c.foundation as u8;
    o[0xfa] = c.can_morph as u8;
    let n = if c.foundation { c.rows.len().min(15) } else { 6 };
    for r in 0..n {
        let row = c.rows.get(r).copied().unwrap_or_default();
        for (k, v) in row.iter().enumerate() {
            let at = 8 + 12 * r + 4 * k;
            if c.foundation || k < 2 {
                o[at..at + 4].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    if !c.foundation {
        o[0xf8] = 0;
    }
    o[0xfc..0x100].copy_from_slice(&c.settlers_in.to_le_bytes());
    o[0x100..0x104].copy_from_slice(&c.settlers_needed.to_le_bytes());
    o[4..8].copy_from_slice(&(CART_CODE as u32).to_le_bytes());
}

/// `CTransportShip` vtbl+0x94 (0x591dc0): +0xf9 = 0, the eight slots at +8 + 12·i (good) /
/// +0xc + 12·i (amount), code 0x16; the rest of the object keeps what an earlier fill left.
fn fill_transport(infos: &mut Infos, slots: &[[i32; 2]; 8]) {
    let o = &mut infos.vehicle;
    o[0xf9] = 0;
    for (i, s) in slots.iter().enumerate() {
        o[8 + 12 * i..12 + 12 * i].copy_from_slice(&s[0].to_le_bytes());
        o[12 + 12 * i..16 + 12 * i].copy_from_slice(&s[1].to_le_bytes());
    }
    o[4..8].copy_from_slice(&(TRANSPORT_CODE as u32).to_le_bytes());
}

/// `CDonkeyRole` 0x577240 (vtbl+0x84): the object cleared after +4 (`GetSize` 0x104), code 0x18,
/// the two slots at +8 / +0xc and +0x14 / +0x18.
fn fill_donkey(infos: &mut Infos, slots: &[(u8, i32); 2]) {
    let o = &mut infos.vehicle;
    o[4..].fill(0);
    o[4..8].copy_from_slice(&(DONKEY_CODE as u32).to_le_bytes());
    for (i, &(good, amount)) in slots.iter().enumerate() {
        o[8 + 12 * i..12 + 12 * i].copy_from_slice(&(good as i32).to_le_bytes());
        o[12 + 12 * i..16 + 12 * i].copy_from_slice(&amount.to_le_bytes());
    }
}

impl Infos {
    fn b(&self, code: usize, off: usize) -> u8 {
        self.obj[code][off]
    }

    fn i(&self, code: usize, off: usize) -> i8 {
        self.obj[code][off] as i8
    }

    fn dword(&self, code: usize, off: usize) -> u32 {
        u32::from_le_bytes(self.obj[code][off..off + 4].try_into().unwrap_or_default())
    }

    fn set(&mut self, code: usize, off: usize, v: u8) {
        self.obj[code][off] = v;
    }

    /// The code of the last fill (the dword at +4 of each object is only its own constant).
    fn code_dword(&mut self, code: usize) {
        self.obj[code][4..8].copy_from_slice(&(code as u32).to_le_bytes());
    }
}

// ------------------------------------------------------------------------------------------------
// What the roles read of the world

/// The facts a role's vtable +0x58 reads of its building, taken from the sim world (on the sim
/// thread; the client turns them into the info object with [`fill`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub id: u16,
    /// `+0xc` type (generic: ports/shipyards as 32/31), `+0x20` race (low nibble) / player.
    pub kind: u8,
    pub race: u8,
    pub player: u8,
    /// `+0x14` entity flags (0x1000 works, 0x2000000 READY).
    pub flags: u32,
    /// buildingInfo +0x1dd house kind (role class); 0 for a construction site.
    pub house_kind: u8,
    /// buildingInfo +0x1de inhabitant type, +0x1df tool, +0x1ec working-area radius, +0x1f0 beds.
    pub inhabitant: u8,
    pub tool: u8,
    pub working_radius: i32,
    pub beds: i32,
    /// Role +0x1d `m_bWorkerInHouse`.
    pub worker_in_house: bool,
    /// The player's buildings of the type: under construction (0x14ef620), finished (0x14ef624).
    pub counts: (i32, i32),
    /// The construction site, if it is one.
    pub site: Option<SiteFacts>,
    /// Farm / gather +0x17c, work-up / ranch / mine +0x17e: the product pile (good, amount).
    pub product: Option<(u8, u8)>,
    /// Production / work-up / ranch / mine input piles (+0x18c / +0x180 / +0x188), and production's
    /// product piles (+0x180), as (good, amount).
    pub inputs: Vec<(u8, u8)>,
    pub products: Vec<(u8, u8)>,
    /// Residence +0x17c: settlers living there (`s4_sim::buildings::residence::residents`).
    pub residents: u8,
    /// The owner's mana (0x149b7fc, stored XOR 0x27356334; `s4_sim::magic::mana`): the small
    /// temple's info +0x18.
    pub mana: i32,
    /// Mine productivity (0x54c330: the sum of the 15 signed result bytes at +0x1a0, / 15).
    pub mine_productivity: u8,
    /// `+0x18` position, `+0x48` door and `+0x4c` working-area centre (release offsets; 5001
    /// writes +0x4c, 0x4f62f0).
    pub pos: (u16, u16),
    pub door: (u16, u16),
    pub work_area: (u16, u16),
    /// The role's vtable +0x28 (the crush request asks it): every role says yes but the military
    /// ones (0x549300): no enemy soldier (warrior type 2..=5) within 10 nodes (0x5b0a20).
    pub may_crush: bool,
    /// 0x53efd0's count for the player: READY towers (types 46, 47, 48, 50, 51) with role +0x1d.
    pub occupied_towers: usize,
    /// Military role +0x1a4: the soldiers in their slots, in slot order, as (settler type,
    /// health %, the slot is a top one: buildingInfo slot `top`, the info's +0xe9 byte).
    pub soldiers: Vec<(u8, u8, bool)>,
    /// Military role +0x180 / +0x181 (top / bottom; the sim's `bow_wish` / `sword_wish`): the
    /// slots first shown as 0xfe.
    pub marked: (u8, u8),
    /// Military role +0x17c + +0x17e (top) and +0x17d + +0x17f (bottom): the slots filled up to
    /// with 0xff after the soldiers.
    pub limit: (u8, u8),
    /// Storage role +0x1ac: its piles as (good, amount).
    pub stored: Vec<(u8, u8)>,
    /// Storage role +0x180..: the goods the player chose to store (by good, 43).
    pub store: Vec<bool>,
    /// Tool / weapon smith (production role +0x198, its product goods): each product good with
    /// the door eco-sector's order count (`CEcoSector` +0x348 tools / +0x364 weapons, by the
    /// same index) and, for weapons, the percentage (+0x37c).
    pub orders: Vec<(u8, u8, u8)>,
    /// `CEcoSector` +0x360 weapon auto-production (100 = on).
    pub weapon_auto: u8,
    /// The barracks' sidebar (0x535150), from the door's eco-sector: its soldier orders
    /// (`+0x33c..+0x345`, the sim's `soldier_orders`) and its carriers over the minimum
    /// (`+0x26 − +0x20`, at least 0).
    pub soldier_orders: [i8; 10],
    pub free_carriers: i32,
    /// `CTradingBuildingRole`: the trade piles (+0x260) as (vt+0x3c good, vt+0x28 amount), and
    /// per good the amount still to send (+0x1ab) and the flags (+0x1d6).
    pub trade_piles: Vec<(u8, u8)>,
    pub trade_send: Vec<i8>,
    pub trade_flags: Vec<u8>,
    /// The owner's `CInternationalTrader` entry (the role's handle +0x2a4): "trades with player
    /// p" (+0x14 + p) and the building's record states per good (0 / 4 / 8), and `PlayerCount`.
    pub trader: bool,
    pub trade_with: [bool; 9],
    pub foreign: Vec<u8>,
    pub player_count: u8,
    /// `CWorkshopBuildingRole` (house kind 5): what its dialog fill (0x559930) and sidebar fill
    /// (0x5597b0) read.
    pub workshop: Option<WorkshopFacts>,
    /// A cart (land vehicle type 5) instead of a building: what `CCart` vtbl+0x94 (0x55bdb0) reads.
    pub cart: Option<CartFacts>,
    /// A transport ship (ship type 3): its eight cargo slots {good, amount} (`CTransportShip`
    /// +0xb0.., vtbl+0x94 0x591dc0; s4_sim::vehicles::transport::dialog).
    pub transport: Option<[[i32; 2]; 8]>,
    /// A donkey (settler type 0x3c): its two slots {good, amount} (`CDonkeyRole` +0x50.., vtbl+0x84
    /// 0x577240; s4_sim::jobs::donkey::dialog).
    pub donkey: Option<[(u8, i32); 2]>,
}

/// What the workshop's fills read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkshopFacts {
    /// +0x190: the order counts of the vehicle types 1..=5 (100 = endless).
    pub orders: [u8; 5],
    /// +0x17c: the type being made.
    pub kind: u8,
    /// The vehicle under construction (+0x17e): its +0x70 progress and +0x72 end.
    pub vehicle: Option<(u16, u16)>,
    /// `CVehicleInfoMgr` (0x12dbce8) +0 / +4 low bytes for the race, by vehicle type 0..=5.
    pub costs: [(u8, u8); 6],
}

/// What the cart's fill 0x55bdb0 reads (`s4_sim::vehicles::cart`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CartFacts {
    pub can_settle: bool,
    pub foundation: bool,
    pub can_morph: bool,
    pub settlers_in: u32,
    pub settlers_needed: u32,
    /// The rows from 0x11aa920: a trade cart's six slots {good, amount, _}; a foundation cart's
    /// goods entries' +4, +8, +0xc (`CartGood` good, arrived, ordered).
    pub rows: Vec<[i32; 3]>,
}

/// What `CBuildingSiteRole` +0x58 (0x543d10) reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SiteFacts {
    /// +0x193/+0x195/+0x197 boards, stones, gold needed; +0x194/+0x196/+0x198 used.
    pub needed: [u8; 3],
    pub used: [u8; 3],
    /// The material piles +0x1b0/+0x1b2/+0x1b4 as (good, amount); `None` = no pile.
    pub piles: [Option<(u8, u8)>; 3],
    /// +0x188 leveling effort left, +0x19c work done, +0x1a0 work to do.
    pub dig_effort: i32,
    pub work_done: i32,
    pub work: i32,
    /// +0x1b8 priority.
    pub priority: bool,
    /// The door's eco-sector's goods on offer (`CEcoSector` 0x4d4280: the offered piles' amounts)
    /// of boards, stones, gold and the tool, and its settlers of the inhabitant type (+0x24).
    pub stock: [i32; 4],
    pub workers: i32,
}

/// The site materials (`CBuildingSiteRole` 0x543d10): boards 7, stones 32, gold 14.
const SITE_GOODS: [u8; 3] = [7, 32, 14];

/// Load the game data [`Facts::from_world`] reads once (~50 ms of XML), so the first building
/// selected does not wait for it: the game calls this while it starts.
pub fn warm() {
    info_extra(0, 0);
}

/// buildingInfo bytes the sim's rules do not carry: (inhabitant, tool, working radius) by (race,
/// type), from the game data (field offsets +0x1de / +0x1df / +0x1ec checked in the XML loader
/// 0x53989a..0x539a7a).
fn info_extra(race: u8, kind: u8) -> (u8, u8, i32) {
    type Extra = HashMap<(u8, u8), (u8, u8, i32)>;
    static T: OnceLock<Extra> = OnceLock::new();
    let t = T.get_or_init(|| {
        let Ok(data) = s4_formats::gamedata::GameData::load_default() else { return HashMap::new() };
        data.buildings
            .iter()
            .map(|b| {
                let v = (b.inhabitant.map_or(0, |s| s as u8), b.tool.map_or(0, |g| g as u8), b.working_area_radius);
                ((b.race as u8, b.building as u8), v)
            })
            .collect()
    });
    t.get(&(race, kind)).copied().unwrap_or_default()
}

fn pile_of(w: &World, id: u16) -> Option<(u8, u8)> {
    (id != 0).then(|| s4_sim::pile::get(w, id).map_or((0, 0), |p| (p.good, p.amount)))
}


/// 0x53efd0 (the count, before its "at least n" test): the player's READY towers of types 46,
/// 47, 48, 50 and 51 whose role has +0x1d (`HaveInhabitant`).
fn occupied_towers(w: &World, player: u8) -> usize {
    w.entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == player && matches!(e.kind, 46 | 47 | 48 | 50 | 51))
        .filter(|&(id, e)| e.flags & s4_sim::entity::flag::READY != 0 && s4_sim::buildings::have_inhabitant(w, id))
        .count()
}

/// 0x5b0a20(radius, x, y, player): a settler of an enemy of the player (`0x12dcd54` enemy mask;
/// `World::alliances`) with warrior type 2..=5 on a node within `radius`.
fn enemy_soldier_near(w: &World, player: u8, (x, y): (i32, i32), radius: i32) -> bool {
    let enemies = w.alliances.enemy_bits.get(player as usize).copied().unwrap_or(0);
    w.entities.iter().any(|(_, e)| {
        e.obj_type == ObjType::Settler
            && !e.is_dead()
            && e.flags & s4_sim::entity::flag::VISIBLE != 0
            && (1..=8).contains(&e.owner)
            && enemies & 1 << (e.owner - 1) != 0
            && (2..=5).contains(&(e.flags & 0xf))
            && s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= radius
    })
}

impl Facts {
    /// The facts of building `id` (`None` if it is not a building).
    pub fn from_world(w: &World, id: u16) -> Option<Facts> {
        use s4_sim::buildings::RoleKind;
        let e = w.entities.by_index(id as u32)?;
        if e.obj_type == ObjType::LandVehicle && e.kind == s4_sim::vehicles::kind::CART {
            return Self::cart_from_world(w, id);
        }
        let base = || Facts { id, kind: e.kind as u8, race: e.race, player: e.owner, flags: e.flags, pos: (e.x() as u16, e.y() as u16), ..Default::default() };
        if e.obj_type == ObjType::Ship && e.kind == s4_sim::vehicles::kind::TRANSPORT_SHIP {
            return s4_sim::vehicles::transport::dialog(w, id).map(|t| Facts { transport: Some(t), ..base() });
        }
        if e.obj_type == ObjType::Settler && e.kind == DONKEY {
            return s4_sim::jobs::donkey::dialog(w, id).map(|d| Facts { donkey: Some(d), ..base() });
        }
        let EntityData::Building(d) = &e.data else { return None };
        if e.obj_type != ObjType::Building {
            return None;
        }
        let rule = w.rules.building(e.race, d.type_ex);
        let (inhabitant, tool, working_radius) = info_extra(e.race, d.type_ex);
        let mut f = Facts {
            id,
            pos: (e.x() as u16, e.y() as u16),
            door: (d.door.x() as u16, d.door.y() as u16),
            work_area: (d.working_area.x() as u16, d.working_area.y() as u16),
            may_crush: true,
            occupied_towers: occupied_towers(w, e.owner),
            kind: e.kind as u8,
            race: e.race,
            player: e.owner,
            flags: e.flags,
            house_kind: rule.house_kind,
            inhabitant,
            tool,
            working_radius,
            beds: rule.beds,
            counts: (
                w.buildings_amount(e.owner as i32, e.kind as i32, s4_sim::query::UNDER_CONSTRUCTION),
                w.buildings_amount(e.owner as i32, e.kind as i32, s4_sim::query::READY),
            ),
            ..Default::default()
        };
        if let Some(s) = w.sites.roles.get(&id) {
            f.house_kind = 0;
            let door = w.grid.checked_index(d.door.x(), d.door.y());
            let eco = door.map_or(0, |n| w.tiling.eco_sector_of(n));
            let stock = [SITE_GOODS[0], SITE_GOODS[1], SITE_GOODS[2], tool].map(|g| if g == 0 { 0 } else { offered(w, eco, g) });
            let workers = w.economy.get(eco).map_or(0, |e| e.settlers.get(inhabitant as usize).copied().unwrap_or(0) as i32);
            f.site = Some(SiteFacts {
                needed: [s.boards, s.stones, s.gold],
                used: [s.boards_used, s.stones_used, s.gold_used],
                piles: s.piles.map(|p| pile_of(w, p)),
                dig_effort: s.dig_effort,
                work_done: s.work_done,
                work: s.work,
                priority: s.prio.on,
                stock,
                workers,
            });
            return Some(f);
        }
        if matches!(f.house_kind, hk::MILITARY | hk::CASTLE) {
            f.may_crush = !enemy_soldier_near(w, e.owner, (e.x(), e.y()), 10);
            if let Some(m) = s4_sim::buildings::military::state(w, id) {
                f.marked = (m.bow_wish, m.sword_wish);
                f.limit = (m.bow_in.wrapping_add(m.bow_coming), m.sword_in.wrapping_add(m.sword_coming));
                let tops: Vec<bool> = rule.soldier_slots.iter().map(|s| s.top).collect();
                for (k, &s) in m.slots.iter().enumerate() {
                    let Some(se) = w.entities.by_index(s as u32).filter(|_| s != 0) else { continue };
                    let EntityData::Settler(sd) = &se.data else { continue };
                    // 0x54a6a0: 100, or hp · 100 / max when below the type's max (table 0x1546932).
                    let max = w.rules.settler_health(se.race, se.kind) as i32;
                    let hp = sd.health as i32;
                    let pct = if max != 0 && hp < max { hp * 100 / max } else { 100 };
                    f.soldiers.push((se.kind as u8, pct as u8, tops.get(k).copied().unwrap_or(false)));
                }
            }
        }
        f.residents = s4_sim::buildings::residence::residents(w, id).unwrap_or(0);
        let Some(role) = s4_sim::buildings::role(w, id) else { return Some(f) };
        f.worker_in_house = role.base.worker_in_house;
        let piles = |v: &[u16]| v.iter().filter_map(|&p| pile_of(w, p)).collect::<Vec<_>>();
        match &role.kind {
            RoleKind::Farm(s) => f.product = pile_of(w, s.pile),
            // The small temple's offering pile (+0x17c) and the owner's mana.
            RoleKind::SmallTemple(s) => {
                f.product = pile_of(w, s.pile);
                f.mana = s4_sim::magic::mana(w, f.player);
            }
            RoleKind::Gather(s) => f.product = pile_of(w, s.pile),
            // The animal ranch is a work-up role in the sim (`CAnimalRanchRole` shares 0x55a6d0).
            RoleKind::WorkUp(s) => {
                f.inputs = piles(&s.delivery);
                f.product = pile_of(w, s.product);
            }
            RoleKind::Mine(s) => {
                f.inputs = piles(&s.delivery);
                f.product = pile_of(w, s.product);
                f.mine_productivity = (s.history.iter().map(|&b| b as i8 as i32).sum::<i32>() / 15) as u8;
            }
            RoleKind::Production(s) => {
                f.inputs = piles(&s.delivery);
                f.products = piles(&s.products);
                // The smiths' sidebars read the door eco-sector's order lists (0x54d480 /
                // 0x54d660).
                if matches!(f.kind, 21 | 22) {
                    let eco = w.grid.checked_index(d.door.x(), d.door.y()).map_or(0, |n| w.tiling.eco_sector_of(n));
                    if let Some(e) = w.economy.get(eco) {
                        // The tool smith's fill 0x54d480 reads only the counts (+0x348); the
                        // weapon smith's 0x54d660 also the percentages (+0x37c): tools have none.
                        let (counts, pct) = if f.kind == 21 { (&e.tool_orders, &[][..]) } else { (&e.weapon_orders, &e.weapon_percent[..]) };
                        f.orders = s.goods.iter().enumerate().map(|(k, &g)| (g, counts.get(k).copied().unwrap_or(0), pct.get(k).copied().unwrap_or(0))).collect();
                        f.weapon_auto = e.weapon_auto;
                    }
                }
            }
            RoleKind::Trading(s) => {
                f.trade_piles = s.piles.iter().map(|&p| pile_of(w, p).unwrap_or((0, 0))).collect();
                f.trade_send = s.send.to_vec();
                f.trade_flags = s.flags.to_vec();
                if let Some(t) = s4_sim::buildings::trader::trader_of(w, id) {
                    f.trader = true;
                    f.trade_with = t.trade_with;
                    f.foreign = t.record(id, e.kind).map_or(vec![0; 43], |r| r.goods.iter().map(|g| g.1).collect());
                }
                f.player_count = w.players.iter().enumerate().filter(|(_, p)| p.is_some()).map(|(i, _)| i as u8).max().unwrap_or(0);
            }
            RoleKind::Storage(s) => {
                f.stored = s.piles.iter().map(|&p| pile_of(w, p).unwrap_or((0, 0))).collect();
                f.store = s.store.to_vec();
            }
            RoleKind::Workshop(s) => {
                // 0x559930 / 0x5597b0: the input piles (+0x184), the orders, the vehicle under
                // construction and its type; the costs of the local race's vehicles ([inf] the
                // building's race: the local player's for his own building).
                f.inputs = piles(&s.piles);
                let vehicle = (s.vehicle != 0).then(|| s4_sim::vehicles::get(w, s.vehicle)).flatten().map(|v| (v.progress[0], v.progress[1]));
                let costs = std::array::from_fn(|t| {
                    let i = w.rules.vehicles.get(e.race, t as u8);
                    (i.boards as u8, i.ironbars as u8)
                });
                f.workshop = Some(WorkshopFacts { orders: s.orders, kind: s.kind, vehicle, costs });
            }
            RoleKind::Barrack(s) => {
                // 0x5352f0: every pile of +0x180 as (vt+0x3c good, vt+0x28 amount).
                f.inputs = piles(&s.piles);
                let eco = w.grid.checked_index(d.door.x(), d.door.y()).map_or(0, |n| w.tiling.eco_sector_of(n));
                if let Some(e) = w.economy.get(eco) {
                    f.soldier_orders = e.soldier_orders;
                    f.free_carriers = (e.settlers[1] as i32 - e.min_carrier as i32).max(0);
                }
            }
            _ => {}
        }
        Some(f)
    }

    /// A cart's facts (`CCart` vtbl+0x94 0x55bdb0; `s4_sim::vehicles::cart`).
    fn cart_from_world(w: &World, id: u16) -> Option<Facts> {
        use s4_sim::vehicles::cart;
        let e = w.entities.by_index(id as u32)?;
        let c = cart::dialog(w, id)?;
        let data = cart::data(w, id)?;
        // The fill copies a foundation cart's goods entries at +4, +8, +0xc (see `CartFacts`).
        let rows = if c.foundation { data.goods.iter().map(|g| [g.good, g.arrived, g.ordered]).collect() } else { c.rows.clone() };
        Some(Facts {
            id,
            kind: e.kind as u8,
            race: e.race,
            player: e.owner,
            flags: e.flags,
            pos: (e.x() as u16, e.y() as u16),
            cart: Some(CartFacts { can_settle: c.can_settle, foundation: c.foundation, can_morph: c.can_morph, settlers_in: c.settlers_in, settlers_needed: c.settlers_needed, rows }),
            ..Default::default()
        })
    }
}

// ------------------------------------------------------------------------------------------------
// The roles' vtable +0x58

/// House kinds (buildingInfo +0x1dd).
mod hk {
    pub const SIMPLE: u8 = 1;
    pub const WORKUP: u8 = 2;
    pub const ANIMALRANCH: u8 = 3;
    pub const PRODUCTION: u8 = 4;
    pub const WORKSHOP: u8 = 5;
    pub const GATHER: u8 = 6;
    pub const MINE: u8 = 7;
    pub const FARM: u8 = 8;
    pub const LOOKOUTTOWER: u8 = 9;
    pub const MILITARY: u8 = 10;
    pub const BARRACK: u8 = 11;
    pub const STORAGE: u8 = 12;
    pub const TRADE: u8 = 13;
    pub const RESIDENCE: u8 = 14;
    pub const CASTLE: u8 = 15;
    pub const SMALLTEMPLE: u8 = 16;
    pub const BIGTEMPLE: u8 = 17;
    pub const EYECATCHER: u8 = 19;
}

/// The role's vtable +0x58: write the building's info object, return its code (`None`: the role
/// has no dialog). `first` = the selection (`refresh = 0`), not a refresh; the site's progress
/// divisor is cached on its first fill.
pub fn fill(infos: &mut Infos, f: &Facts) -> Option<usize> {
    let race_player = |infos: &mut Infos, code: usize| {
        infos.set(code, 8, f.kind);
        infos.set(code, 9, f.race & 0xf);
    };
    let counts = |infos: &mut Infos, code: usize| {
        infos.set(code, 0xf, f.counts.0 as u8);
        infos.set(code, 0x10, f.counts.1 as u8);
    };
    let works = (f.flags >> 12 & 1) as u8;
    if let Some(c) = &f.cart {
        fill_cart(infos, c);
        return Some(CART_CODE);
    }
    if let Some(t) = &f.transport {
        fill_transport(infos, t);
        return Some(TRANSPORT_CODE);
    }
    if let Some(d) = &f.donkey {
        fill_donkey(infos, d);
        return Some(DONKEY_CODE);
    }
    if let Some(s) = &f.site {
        // CBuildingSiteRole 0x543d10.
        let c = 1;
        infos.code_dword(c);
        race_player(infos, c);
        infos.set(c, 0xb, 1);
        infos.set(c, 0xc, works);
        infos.set(c, 0x20, s.priority as u8);
        if f.tool != 0 {
            infos.set(c, 0x12, if s.stock[3] < 1 { (f.tool as i8).wrapping_neg() as u8 } else { f.tool });
        }
        if f.inhabitant != 0 {
            infos.set(c, 0x11, if s.workers == 0 { (f.inhabitant as i8).wrapping_neg() as u8 } else { f.inhabitant });
        }
        infos.set(c, 0xd, (f.working_radius > 0) as u8);
        counts(infos, c);
        // 0x543c60: 100 − (work left + leveling left) / divisor, the divisor (work + leveling) /
        // 100 taken once (1.0 if that is 0), clamped to 0..=100.
        let div = *infos.site_divisor.entry(f.id).or_insert_with(|| {
            let d = (s.work + s.dig_effort) as f32 / 100.0;
            if d == 0.0 { 1.0 } else { d }
        });
        let p = (100.0 - ((s.work - s.work_done) + s.dig_effort) as f32 / div) as i32;
        infos.set(c, 0xe, p.clamp(0, 100) as u8);
        for (k, (&pile, &good)) in s.piles.iter().zip(&SITE_GOODS).enumerate() {
            let (good_off, amount_off) = (0x14 + 2 * k, 0x15 + 2 * k);
            match pile {
                None => infos.set(c, good_off, 0),
                Some((_, amount)) => {
                    let mut missing = if s.needed[k] == 0 { 0 } else { (s.needed[k] as i32 - s.used[k] as i32) - amount as i32 };
                    if s.stock[k] < missing {
                        missing = -missing;
                    }
                    infos.set(c, amount_off, missing as u8);
                    infos.set(c, good_off, if missing != 0 { good } else { 0 });
                }
            }
        }
        for (k, pile) in s.piles.iter().enumerate() {
            let (good, amount) = pile.unwrap_or((0, 0));
            infos.set(c, 0x1a + 2 * k, good);
            infos.set(c, 0x1b + 2 * k, amount);
        }
        return Some(c);
    }
    let house = |infos: &mut Infos| {
        infos.obj[0][4..8].fill(0);
        race_player(infos, 0);
    };
    match f.house_kind {
        hk::SIMPLE if f.kind == 27 => {
            // CSimpleBuildingRole 0x54f4b0, type 27.
            house(infos);
            infos.set(0, 0xb, 1);
            infos.set(0, 0xc, works);
            infos.set(0, 0xd, (f.working_radius > 0) as u8);
            counts(infos, 0);
            infos.set(0, 0xa, f.worker_in_house as u8);
            if f.worker_in_house {
                infos.set(0, 0x11, f.inhabitant);
            }
            Some(0)
        }
        hk::SIMPLE | hk::GATHER | hk::FARM => {
            // 0x54f4b0 (other types), CGatherBuildingRole 0x546f20, CFarmBuildingRole 0x5467f0.
            let c = 2;
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 1);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, if f.house_kind == hk::SIMPLE { (f.working_radius > 0) as u8 } else { 1 });
            counts(infos, c);
            if f.house_kind == hk::GATHER {
                infos.set(c, 0xe, 0xff);
            }
            infos.set(c, 0xa, f.worker_in_house as u8);
            if f.worker_in_house {
                infos.set(c, 0x11, f.inhabitant);
            }
            let (good, amount) = if f.house_kind == hk::SIMPLE { (0, 0) } else { f.product.unwrap_or((0, 0)) };
            infos.set(c, 0x14, good);
            infos.set(c, 0x15, amount);
            Some(c)
        }
        hk::WORKUP | hk::ANIMALRANCH | hk::PRODUCTION | hk::MINE => {
            // 0x55a6d0 (work-up, animal ranch), CProductionBuildingRole 0x54d7b0 and CAnimalRanch
            // clear the object first (vtable +4 size); CMineRole 0x54c060 clears the pile bytes.
            let c = 3;
            if f.house_kind == hk::MINE {
                for o in 0x14..0x32 {
                    infos.set(c, o, 0);
                }
            } else {
                infos.obj[c][4..SIZES[c]].fill(0);
            }
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 1);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, 0);
            counts(infos, c);
            infos.set(c, 0xa, f.worker_in_house as u8);
            if f.worker_in_house {
                infos.set(c, 0x11, f.inhabitant);
            }
            match f.house_kind {
                hk::MINE => infos.set(c, 0xe, f.mine_productivity),
                hk::PRODUCTION => {}
                _ => infos.set(c, 0xe, 0xff),
            }
            for (k, &(good, amount)) in f.inputs.iter().take(5).enumerate() {
                infos.set(c, 0x14 + 2 * k, good);
                infos.set(c, 0x15 + 2 * k, amount);
            }
            let outputs: Vec<(u8, u8)> = if f.house_kind == hk::PRODUCTION { f.products.clone() } else { f.product.into_iter().collect() };
            for (k, &(good, amount)) in outputs.iter().take(10).enumerate() {
                infos.set(c, 0x1e + 2 * k, good);
                infos.set(c, 0x1f + 2 * k, amount);
            }
            Some(c)
        }
        hk::LOOKOUTTOWER => {
            // CLookoutTowerRole 0x547570.
            house(infos);
            infos.set(0, 0xb, 0);
            infos.set(0, 0xd, 0);
            counts(infos, 0);
            infos.set(0, 0xa, f.worker_in_house as u8);
            if f.worker_in_house {
                infos.set(0, 0x11, f.inhabitant);
            }
            Some(0)
        }
        hk::RESIDENCE => {
            // CResidenceBuildingRole 0x54df70 (it also writes the site object's +0xc).
            house(infos);
            infos.set(1, 0xc, works);
            infos.set(0, 0xa, 0);
            infos.set(0, 0xb, 0);
            infos.set(0, 0xd, 0);
            infos.set(0, 0xe, 0xff);
            infos.set(0, 0x13, (f.beds - f.residents as i32).max(0) as u8);
            counts(infos, 0);
            Some(0)
        }
        hk::BIGTEMPLE => {
            // CBigTempleRole 0x535880.
            house(infos);
            infos.set(0, 0xb, 0);
            infos.set(0, 0xc, 1);
            infos.set(0, 0xd, 0);
            counts(infos, 0);
            infos.set(0, 0xa, f.worker_in_house as u8);
            Some(0)
        }
        hk::EYECATCHER => {
            // CEyeCatcherRole 0x5460c0 (it writes the site object's +0xb / +0xc).
            house(infos);
            infos.set(1, 0xb, 0);
            infos.set(1, 0xc, works);
            infos.set(0, 0xd, 0);
            infos.set(0, 0xe, 0xff);
            infos.set(0, 0xa, 0);
            counts(infos, 0);
            Some(0)
        }
        hk::MILITARY | hk::CASTLE => {
            // CCastleRole / CMilitaryBuildingRole 0x54a6a0: cleared, then the soldier slots: top
            // (+0x14) and bottom (+0x28), 10 (type, health %) each: 0xfe for the first +0x180 /
            // +0x181, the soldiers in slot order over them, then 0xff up to in + coming.
            let c = 4;
            infos.obj[c][4..SIZES[c]].fill(0);
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 0);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, 0);
            counts(infos, c);
            infos.set(c, 0xa, f.worker_in_house as u8);
            for (top, base, marked, limit) in [(true, 0x14, f.marked.0, f.limit.0), (false, 0x28, f.marked.1, f.limit.1)] {
                for n in 0..(marked as usize).min(10) {
                    infos.set(c, base + 2 * n, 0xfe);
                    infos.set(c, base + 2 * n + 1, 0);
                }
                let mut n = 0;
                for &(kind, health, _) in f.soldiers.iter().filter(|s| s.2 == top) {
                    if n < 10 {
                        infos.set(c, base + 2 * n, kind);
                        infos.set(c, base + 2 * n + 1, health);
                    }
                    n += 1;
                }
                while n < (limit as usize).min(10) {
                    infos.set(c, base + 2 * n, 0xff);
                    infos.set(c, base + 2 * n + 1, 0);
                    n += 1;
                }
            }
            Some(c)
        }
        hk::TRADE => {
            // CTradingBuildingRole 0x5533f0 (ports 32, marketplaces 33): cleared, the header,
            // then per trade pile i (+0x260) with goods (vt+0x28) its good (vt+0x3c) at +0x14 + 2i
            // and amount at +0x15 + 2i; an empty pile leaves its slot 0.
            let c = 5;
            infos.obj[c][4..SIZES[c]].fill(0);
            infos.code_dword(c);
            infos.set(c, 9, f.race & 0xf);
            if matches!(f.kind, 32 | 33) {
                infos.set(c, 8, f.kind);
                infos.set(c, 0xb, 1);
                infos.set(c, 0xc, works);
                infos.set(c, 0xd, 0);
                counts(infos, c);
                if f.kind == 32 {
                    infos.set(c, 0xa, f.worker_in_house as u8);
                    if f.worker_in_house {
                        infos.set(c, 0x11, f.inhabitant);
                    }
                } else {
                    infos.set(c, 0x11, 0);
                }
            }
            // The original has no bound (a building has 8 trade piles; slot 16 would run into the
            // military object at 0x11aad0c).
            for (i, &(good, amount)) in f.trade_piles.iter().enumerate().take((SIZES[c] - 0x14) / 2) {
                if amount != 0 {
                    infos.set(c, 0x14 + 2 * i, good);
                    infos.set(c, 0x15 + 2 * i, amount);
                }
            }
            Some(c)
        }
        hk::STORAGE => {
            // CStorageBuildingRole 0x5508d0: the header, then its piles (an empty one as 0, 0).
            let c = 6;
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 1);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, 0);
            infos.set(c, 0xa, 0);
            infos.set(c, 0x11, 0);
            counts(infos, c);
            for (k, &(good, amount)) in f.stored.iter().take(10).enumerate() {
                let (g, a) = if amount == 0 { (0, 0) } else { (good, amount) };
                infos.set(c, 0x14 + 2 * k, g);
                infos.set(c, 0x15 + 2 * k, a);
            }
            Some(c)
        }
        hk::BARRACK => {
            // CBarrackRole 0x5352f0: the header and its input piles (+0x180, s4-sim `barrack::State::piles`).
            let c = 7;
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 1);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, 0);
            counts(infos, c);
            infos.set(c, 0xa, f.worker_in_house as u8);
            if f.worker_in_house {
                infos.set(c, 0x11, f.inhabitant);
            }
            // Every pile, unbounded in the original; the 32-byte object holds six.
            for (k, &(good, amount)) in f.inputs.iter().take(6).enumerate() {
                infos.set(c, 0x14 + 2 * k, good);
                infos.set(c, 0x15 + 2 * k, amount);
            }
            Some(c)
        }
        hk::SMALLTEMPLE => {
            // CSmallTempleRole 0x54fe40: the header, the offering pile (+0x17c): its good
            // (vt+0x3c) at +0x14 and amount (vt+0x28) at +0x15, the player's mana (0x149b7fc,
            // stored XOR 0x27356334) as the dword +0x18; +0x16 / +0x17 are not written.
            let c = 8;
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 1);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, (f.working_radius > 0) as u8);
            counts(infos, c);
            infos.set(c, 0xa, f.worker_in_house as u8);
            if let Some((good, amount)) = f.product {
                infos.set(c, 0x14, good);
                infos.set(c, 0x15, amount);
            }
            infos.obj[c][0x18..0x1c].copy_from_slice(&f.mana.to_le_bytes());
            if f.worker_in_house {
                infos.set(c, 0x11, f.inhabitant);
            }
            Some(c)
        }
        hk::WORKSHOP => {
            // CWorkshopBuildingRole 0x559930: cleared, the header, its input piles (+0x184), and
            // the vehicle under construction (+0x17e): its progress into +0x20, the type +0x17c
            // into +0x24.
            let c = 9;
            infos.obj[c][4..SIZES[c]].fill(0);
            infos.code_dword(c);
            race_player(infos, c);
            infos.set(c, 0xb, 1);
            infos.set(c, 0xc, works);
            infos.set(c, 0xd, 0);
            counts(infos, c);
            infos.set(c, 0xa, f.worker_in_house as u8);
            if f.worker_in_house {
                infos.set(c, 0x11, f.inhabitant);
            }
            for (k, &(good, amount)) in f.inputs.iter().take(5).enumerate() {
                infos.set(c, 0x14 + 2 * k, good);
                infos.set(c, 0x15 + 2 * k, amount);
            }
            if let Some(w) = &f.workshop
                && let Some((p, end)) = w.vehicle
            {
                infos.obj[c][0x20..0x24].copy_from_slice(&(workshop_progress(p, end) as u32).to_le_bytes());
                infos.obj[c][0x24..0x28].copy_from_slice(&(w.kind as u32).to_le_bytes());
            }
            Some(c)
        }
        _ => None,
    }
}

/// The workshop dialog's progress (0x559930, SSE single precision): `r = (p + end) / 100` (1.0
/// when 0; the release stores it in the vehicle's +0x7c), then `trunc(100 − (end − p) / r)` with
/// `end − p` as a signed int, at most 100 and below 0 → 0.
pub fn workshop_progress(p: u16, end: u16) -> i32 {
    let r = (p as u32 + end as u32) as i32 as f32 / 100.0;
    let r = if r == 0.0 { 1.0 } else { r };
    let v = (100.0f32 - (end as u32).wrapping_sub(p as u32) as i32 as f32 / r) as i32;
    v.clamp(0, 100)
}

/// The crush request (0x536850): the role's vtable +0x28 allows it (every role but the military
/// ones, 0x549300: no enemy soldier within 10 nodes) → code 10; `IsLastTower` = an occupied (+0x1d)
/// READY tower (types 46, 47, 48, 50, 51) when the player has fewer than two of them (0x53efd0).
/// `None`: refused (the original then calls `AddMessage` with string 2551, which its warning
/// classification filters out: see s4-game `bdlg.rs`).
pub fn crush_request(infos: &mut Infos, f: &Facts) -> Option<usize> {
    if !f.may_crush {
        return None;
    }
    infos.code_dword(10);
    infos.last_tower = matches!(f.kind, 46 | 47 | 48 | 50 | 51) && f.flags & 0x200_0000 != 0 && f.worker_in_house && f.occupied_towers < 2;
    Some(10)
}

/// The working area shown in work-area mode (0x53d2e0, from 0x4e9c50): rings of markers around
/// the centre (`+0x4c`) at `r·k / n` for k = 1..=n (n = 4 rings with helper objects 10, 11, 13,
/// 15 when the buildingInfo radius r (+0x1ec) is above 5, else 3 with 10, 12, 15; a ring index
/// already drawn is skipped), each ring's spiral nodes inside the map and in the door's
/// eco-sector, as (x, y, helper object) like the build markers.
pub fn work_area_markers(pop: &crate::population::Population, f: &Facts) -> Vec<(u16, u16, u8)> {
    let r = f.working_radius;
    if r <= 0 {
        return Vec::new();
    }
    let size = pop.size as i32;
    let eco = |x: i32, y: i32| pop.economy.eco_of.get((y * size + x) as usize).copied().unwrap_or(0);
    let door_eco = eco(f.door.0 as i32, f.door.1 as i32);
    let (icons, n): (&[u8], i32) = if r > 5 { (&[10, 11, 13, 15], 4) } else { (&[10, 12, 15], 3) };
    let (cx, cy) = (f.work_area.0 as i32, f.work_area.1 as i32);
    let mut out = Vec::new();
    let mut prev = -1;
    for (k, &icon) in icons.iter().enumerate() {
        let ring = r * (k as i32 + 1) / n;
        if ring <= prev {
            continue;
        }
        prev = ring;
        let ring = ring.min(255) as u8;
        for i in s4_sim::spiral::first(ring)..=s4_sim::spiral::last(ring) {
            let (x, y) = (cx + s4_sim::spiral::dx(i), cy + s4_sim::spiral::dy(i));
            if (0..size).contains(&x) && (0..size).contains(&y) && eco(x, y) == door_eco {
                out.push((x as u16, y as u16, icon));
            }
        }
    }
    out
}

/// The work-area click (0x4ead10 → 0x53d450): the first node of the spiral around the click
/// (rings 0..=4, inside the map) that has an eco-sector, if it is closer than 60 to the building
/// and in the eco-sector of its door. [inf] When no node of the rings has one, the original goes
/// on with the last node tried; here the click is refused.
pub fn work_area_target(pop: &crate::population::Population, f: &Facts, (x, y): (i32, i32)) -> Option<(u16, u16)> {
    let size = pop.size as i32;
    let eco = |x: i32, y: i32| pop.economy.eco_of.get((y * size + x) as usize).copied().unwrap_or(0);
    let grid = s4_sim::grid::Grid::new(pop.size as u32)?;
    let (px, py) = s4_sim::spiral::walk(grid, x, y, 4).find(|&(px, py)| eco(px, py) != 0)?;
    let d = s4_sim::spiral::grid_distance(px - f.pos.0 as i32, py - f.pos.1 as i32);
    (d < 60 && eco(px, py) == eco(f.door.0 as i32, f.door.1 as i32)).then_some((px as u16, py as u16))
}

// ------------------------------------------------------------------------------------------------
// Updates (0x488e00)

/// Building name string ids (release 0x49eef0) by \[type\]\[race\] (race 0 Roman, 1 Viking, 2 Maya,
/// 3 Dark Tribe, 4 Trojan); 0xff00fc = none ("").
const NAME_DEFAULT: u32 = 0xff00fc;

/// The helpers' race switches: Roman, Viking, Maya and Trojan by id, anything else takes the
/// `default:` branch, which is the Dark Tribe column of the tables (it differs from the Roman one
/// only for type 18's picture).
fn race_column(race: i8) -> usize {
    match race {
        0 | 1 | 2 | 4 => race as usize,
        _ => 3,
    }
}

fn name_id(race: i8, kind: i8) -> u32 {
    let r = race_column(race);
    let t = kind as i32;
    if !(1..=83).contains(&t) {
        return NAME_DEFAULT;
    }
    NAME[t as usize][r]
}

fn picture(race: i8, kind: i8, pressed: bool) -> u16 {
    let r = race_column(race);
    let t = if (1..=83).contains(&(kind as i32)) { kind as usize } else { 0 };
    if pressed { PICTURE1[t][r] } else { PICTURE0[t][r] }
}

/// 0x49f720: the good's icon (default 429).
pub fn good_icon(good: i32) -> u16 {
    if (0..44).contains(&good) { GOOD_ICON[good as usize] } else { 429 }
}

/// `wsprintfA("%u", (int)(char)v)`.
fn u(v: i8) -> String {
    (v as i32 as u32).to_string()
}

/// What an update does to the GUI.
struct Ui<'a> {
    st: &'a mut GuiState,
    d: usize,
    s: &'a dyn Fn(u32) -> String,
}

impl Ui<'_> {
    fn text(&mut self, link: u16, t: String) {
        self.st.texts.insert((self.d, link), t);
    }
    fn image(&mut self, link: u16, img: u16) {
        self.st.images.insert((self.d, link), (img, 0));
    }
    fn effect(&mut self, link: u16, bit: u8, on: bool) {
        if let Some(i) = self.st.index_of(self.d, link)
            && let Some(e) = self.st.effects.get_mut(&(self.d, i))
        {
            *e = if on { *e | bit } else { *e & !bit };
        }
    }
    fn visible(&mut self, link: u16, v: bool) {
        self.effect(link, 8, !v);
    }
    fn enable(&mut self, link: u16, v: bool) {
        self.effect(link, 4, !v);
    }
    /// `SelectControl` on a radio (each building radio has a group of its own).
    fn select(&mut self, link: u16, v: bool) {
        self.effect(link, 1, v);
    }
    fn slider(&mut self, link: u16, v: i8) {
        self.st.bars.insert((self.d, link), v as u8);
    }
    fn font(&mut self, link: u16, f: u8) {
        self.st.fonts.insert((self.d, link), f);
    }
    fn name(&mut self, link: u16, race: i8, kind: i8) {
        let id = name_id(race, kind);
        let t = if id == NAME_DEFAULT { String::new() } else { (self.s)(id) };
        self.text(link, t);
    }
    /// The count / "+n" pair: "+%u" shown only for `plus_if > 0`.
    fn counts(&mut self, links: (u16, u16), count: i8, plus: i8, plus_if: i8) {
        self.text(links.0, u(count));
        if plus_if > 0 {
            self.text(links.1, format!("+{}", u(plus)));
            self.visible(links.1, true);
        } else {
            self.visible(links.1, false);
        }
    }
    /// A (good, amount) slot: the amount's absolute value and the good's icon (inline: image +8,
    /// pressed +0xe = 0), both hidden without a good (`good <= 0`); font 1 on the text, or 2 for
    /// a negative amount, on the text or (D20, D23, D24: the originals' slip) on the icon.
    fn good_slot(&mut self, (text, icon): (u16, u16), good: i8, amount: i8, font_on: FontOn) {
        if good > 0 {
            self.text(text, (amount as i32).unsigned_abs().to_string());
            self.image(icon, good_icon(good as i32));
            self.visible(text, true);
            self.visible(icon, true);
            match font_on {
                FontOn::Text => self.font(text, if amount < 0 { 2 } else { 1 }),
                FontOn::NegativeOnIcon if amount < 0 => self.font(icon, 2),
                FontOn::NegativeOnIcon => self.font(text, 1),
            }
        } else {
            self.visible(text, false);
            self.visible(icon, false);
        }
    }

    /// The header every building dialog has: picture, name, the count and "+n" (sites).
    fn header(&mut self, (pic, title, count, plus): (u16, u16, u16, u16), race: i8, kind: i8, counts: (i8, i8), pressed_picture: bool) {
        self.image(pic, picture(race, kind, pressed_picture));
        self.name(title, race, kind);
        self.counts((count, plus), counts.1, counts.0, counts.0);
    }
}

#[derive(Clone, Copy)]
enum FontOn {
    Text,
    NegativeOnIcon,
}

/// Fill dialog `DIALOGS[code]` from its info object (the update switch 0x488e00). `strings` gives
/// a string id's text.
pub fn update(code: usize, infos: &Infos, st: &mut GuiState, strings: &dyn Fn(u32) -> String) {
    if dialog_of(code) == crate::vehicle_dialog::DIALOG {
        crate::vehicle_dialog::update(&infos.vehicle, infos.gui_race, st, strings);
        return;
    }
    let d = DIALOGS[code];
    let mut ui = Ui { st, d, s: strings };
    let i = |o: usize| infos.i(code, o);
    match code {
        0 => {
            // 0x4709c0.
            ui.image(470, picture(i(9), i(8), false));
            ui.name(467, i(9), i(8));
            ui.counts((468, 469), i(0x10), i(0xf), i(0xf));
            ui.visible(471, infos.b(0, 0xa) != 0);
            if matches!(infos.b(0, 8), 40..=42) {
                ui.text(472, infos.b(0, 0x13).to_string());
            }
        }
        1 => {
            // 0x46daa0.
            ui.image(420, picture(i(9), i(8), true));
            ui.name(408, i(9), i(8));
            ui.counts((409, 410), i(0x10), i(0xf), i(0xf));
            for (k, links) in [(416, 413), (415, 412), (414, 411)].into_iter().enumerate() {
                ui.good_slot(links, i(0x14 + 2 * k), i(0x15 + 2 * k), FontOn::Text);
            }
            for (k, links) in [(425, 422), (426, 423), (427, 424)].into_iter().enumerate() {
                ui.good_slot(links, i(0x1a + 2 * k), i(0x1b + 2 * k), FontOn::Text);
            }
            ui.slider(418, i(0xe));
            ui.visible(405, infos.b(1, 0xd) != 0);
            ui.select(407, infos.b(1, 0xc) == 0);
            ui.select(417, infos.b(1, 0x20) != 0);
        }
        2 => {
            // 0x49d2c0: the "+n" shows by the *house* object's +0xf (0x49d35a).
            ui.image(1093, picture(i(9), i(8), false));
            ui.name(1087, i(9), i(8));
            ui.counts((1088, 1089), i(0x10), i(0xf), infos.i(0, 0xf));
            ui.visible(1094, infos.b(2, 0xa) != 0);
            if infos.b(2, 0xe) == 0xff {
                ui.visible(1096, false);
            } else {
                ui.slider(1096, i(0xe));
                ui.visible(1096, true);
            }
            ui.enable(1084, infos.b(2, 0xd) != 0);
            ui.select(1086, infos.b(2, 0xc) == 0);
            ui.visible(1086, infos.b(2, 0xb) != 0);
            if i(0x14) > 0 {
                ui.text(1091, u(i(0x15)));
                ui.image(1090, good_icon(i(0x14) as i32));
                ui.visible(1091, true);
                ui.visible(1090, true);
            } else {
                ui.visible(1091, false);
                ui.visible(1090, false);
            }
        }
        3 => {
            // 0x49f8e0.
            ui.image(1106, picture(i(9), i(8), false));
            ui.name(1100, i(9), i(8));
            ui.counts((1101, 1102), i(0x10), i(0xf), i(0xf));
            ui.visible(1107, infos.b(3, 0xa) != 0);
            if infos.b(3, 0xe) == 0xff {
                ui.visible(1137, false);
            } else {
                ui.slider(1137, i(0xe));
                ui.visible(1137, true);
            }
            ui.select(1099, infos.b(3, 0xc) == 0);
            ui.visible(1099, infos.b(3, 0xb) != 0);
            const INPUTS: [(u16, u16); 5] = [(1128, 1117), (1129, 1118), (1134, 1131), (1135, 1132), (1136, 1133)];
            const OUTPUTS: [(u16, u16); 10] =
                [(1119, 1108), (1120, 1109), (1104, 1103), (1121, 1110), (1122, 1111), (1123, 1112), (1124, 1113), (1125, 1114), (1126, 1115), (1127, 1116)];
            for (k, &links) in INPUTS.iter().enumerate() {
                ui.good_slot(links, i(0x14 + 2 * k), i(0x15 + 2 * k), FontOn::Text);
            }
            for (k, &links) in OUTPUTS.iter().enumerate() {
                ui.good_slot(links, i(0x1e + 2 * k), i(0x1f + 2 * k), FontOn::Text);
            }
            // The tool / weapon smiths' order page button (UI event 0x1cc).
            ui.visible(1138, matches!(infos.b(3, 8), 21 | 22));
        }
        4 => {
            // 0x49a210: the picture and name by the local player's race (0x15aabb8, set by the
            // procedure's init 0x49b090), then the soldier slots: top (+0x14) and bottom (+0x28),
            // (settler type, health %) each; type 0xfe = on the way, 0xff = wished.
            let race = infos.gui_race as i8;
            ui.header((976, 973, 974, 975), race, i(8), (i(0xf), i(0x10)), false);
            ui.select(972, infos.b(4, 0xc) == 0);
            ui.visible(972, infos.b(4, 0xb) != 0);
            for (base, button0, bar0) in [(0x14, 1001, 979), (0x28, 1011, 989)] {
                for k in 0..10 {
                    let (t, health) = (i(base + 2 * k), i(base + 2 * k + 1));
                    let (button, bar) = (button0 + k as u16, bar0 + k as u16);
                    match t {
                        -2 => {
                            ui.st.images.entry((d, button)).or_insert((0, 0)).0 = 158;
                            ui.slider(bar, health);
                            ui.visible(bar, false);
                            ui.visible(button, true);
                        }
                        -1 => {
                            ui.image(button, 159);
                            ui.slider(bar, health);
                            ui.visible(bar, false);
                            ui.visible(button, true);
                        }
                        t if t > 0 => {
                            if let Some((_, imgs)) = SOLDIER_IMAGE.iter().find(|s| s.0 as i8 == t) {
                                ui.st.images.insert((d, button), imgs[(race as usize).min(4)]);
                            }
                            ui.slider(bar, health);
                            ui.visible(bar, true);
                            ui.visible(button, true);
                        }
                        _ => {
                            ui.visible(button, false);
                            ui.visible(bar, false);
                        }
                    }
                }
            }
        }
        5 => {
            // 0x49c330: 15 (good, amount) slots, shown for goods 1..=42 with an amount > 0.
            ui.header((1033, 1028, 1029, 1030), i(9), i(8), (i(0xf), i(0x10)), false);
            ui.select(1027, infos.b(5, 0xc) == 0);
            ui.visible(1027, infos.b(5, 0xb) != 0);
            ui.visible(1034, infos.b(5, 0xa) != 0);
            const SLOTS: [(u16, u16); 15] = [
                (1055, 1044),
                (1056, 1045),
                (1058, 1059),
                (1062, 1064),
                (1063, 1065),
                (1046, 1035),
                (1047, 1036),
                (1032, 1031),
                (1048, 1037),
                (1049, 1038),
                (1050, 1039),
                (1051, 1040),
                (1052, 1041),
                (1053, 1042),
                (1054, 1043),
            ];
            for (k, &(text, icon)) in SLOTS.iter().enumerate() {
                let (good, amount) = (i(0x14 + 2 * k), i(0x15 + 2 * k));
                if (1..=42).contains(&good) && amount > 0 {
                    ui.text(text, amount.to_string());
                    ui.image(icon, good_icon(good as i32));
                    ui.visible(text, true);
                    ui.visible(icon, true);
                } else {
                    ui.visible(text, false);
                    ui.visible(icon, false);
                }
            }
            // 0x49c6ff: in a local free game or a campaign (`GlobalGameState +0x2a8` 1 / 3) the
            // foreign-trade button 1057 is hidden (+0x1b bit 8) and nothing is enabled; else
            // 1060 and 1057 are enabled, for a port (0x20) only with +0xa.
            if matches!(ui.st.game_kind, 1 | 3) {
                ui.visible(1057, false);
            } else {
                let on = infos.b(5, 8) != 0x20 || infos.b(5, 0xa) != 0;
                ui.enable(1060, on);
                ui.enable(1057, on);
            }
        }
        6 => {
            // 0x4ac390: 10 (good, amount) slots, the font set on the icon.
            ui.header((1614, 1608, 1609, 1610), i(9), i(8), (i(0xf), i(0x10)), false);
            ui.select(1607, infos.b(6, 0xc) == 0);
            ui.visible(1607, infos.b(6, 0xb) != 0);
            const SLOTS: [(u16, u16); 10] =
                [(1619, 1615), (1620, 1616), (1612, 1611), (1621, 1617), (1622, 1618), (1629, 1624), (1630, 1625), (1631, 1626), (1632, 1627), (1633, 1628)];
            for (k, &links) in SLOTS.iter().enumerate() {
                ui.good_slot(links, i(0x14 + 2 * k), i(0x15 + 2 * k), FontOn::NegativeOnIcon);
            }
        }
        7 => {
            // 0x46c230: the input goods go to fixed slots by good (34 -> 0, 8 -> 1, 5/6/38 -> 2,
            // 3 -> 3, 14 -> 4; table 0xffc2b0); slot 2 is hidden first.
            ui.header((337, 331, 332, 333), i(9), i(8), (i(0xf), i(0x10)), false);
            ui.select(330, infos.b(7, 0xc) == 0);
            ui.visible(330, infos.b(7, 0xb) != 0);
            const SLOTS: [(u16, u16); 5] = [(341, 338), (342, 339), (335, 334), (343, 340), (346, 345)];
            ui.visible(334, false);
            ui.visible(335, false);
            for k in 0..5 {
                let (good, amount) = (i(0x14 + 2 * k), i(0x15 + 2 * k));
                let slot = match good {
                    34 => 0,
                    8 => 1,
                    5 | 6 | 38 => 2,
                    3 => 3,
                    14 => 4,
                    _ => continue,
                };
                let (text, icon) = SLOTS[slot];
                ui.text(text, (amount as i32).unsigned_abs().to_string());
                ui.image(icon, good_icon(good as i32));
                ui.visible(icon, true);
                ui.visible(text, true);
                ui.font(text, if amount < 0 { 2 } else { 1 });
            }
        }
        8 => {
            // 0x4a5e40: the offering good, the player's mana (dword +0x18).
            ui.header((1394, 1390, 1391, 1392), i(9), i(8), (i(0xf), i(0x10)), false);
            if infos.b(8, 0x14) == 0 {
                ui.visible(1397, false);
                ui.visible(1396, false);
            } else {
                ui.text(1397, (i(0x15) as i32).unsigned_abs().to_string());
                ui.image(1396, good_icon(infos.b(8, 0x14) as i32));
                ui.visible(1397, true);
                ui.visible(1396, true);
                if i(0x15) < 0 {
                    ui.font(1396, 2);
                } else {
                    ui.font(1397, 1);
                }
            }
            ui.select(1389, infos.b(8, 0xc) == 0);
            ui.visible(1389, infos.b(8, 0xb) != 0);
            ui.visible(1395, infos.b(8, 0xa) != 0);
            ui.text(1399, infos.dword(8, 0x18).to_string());
        }
        9 => {
            // 0x4a5100: 5 (good, amount) slots (font on the icon), the vehicle under
            // construction: progress (dword +0x20) and type (dword +0x24, picture table 0xffec50).
            ui.header((1372, 1366, 1367, 1368), i(9), i(8), (i(0xf), i(0x10)), false);
            ui.select(1365, infos.b(9, 0xc) == 0);
            ui.visible(1365, infos.b(9, 0xb) != 0);
            ui.visible(1386, infos.b(9, 0xa) != 0);
            const SLOTS: [(u16, u16); 5] = [(1377, 1373), (1378, 1374), (1370, 1369), (1379, 1375), (1380, 1376)];
            for (k, &links) in SLOTS.iter().enumerate() {
                ui.good_slot(links, i(0x14 + 2 * k), i(0x15 + 2 * k), FontOn::NegativeOnIcon);
            }
            let vehicle = infos.dword(9, 0x24) as i32;
            if vehicle < 1 {
                ui.visible(1385, false);
                ui.visible(1383, false);
            } else {
                let k = (i(9) as i32 * 7 + vehicle) as usize;
                ui.st.images.entry((d, 1385)).or_insert((0, 0)).0 = VEHICLE_PICTURE.get(k).copied().unwrap_or(0);
                ui.visible(1385, true);
                ui.visible(1383, true);
            }
            let progress = infos.dword(9, 0x20) as i32;
            if progress > 0 {
                ui.slider(1384, progress as i8);
                ui.visible(1384, true);
            } else {
                ui.visible(1384, false);
            }
        }
        10 => {
            // 0x4b0a40.
            ui.text(1798, (ui.s)(if infos.last_tower { 2550 } else { 1971 }));
            ui.text(1796, (ui.s)(1421));
        }
        _ => {}
    }
}

// ------------------------------------------------------------------------------------------------
// Procedures

/// A building dialog's button, as the GUI event its procedure sends (`CreateUIEvent(id, w, l)`),
/// for code 3 on (dialog, link) with the control's `effects` after the click. Checked by running
/// the procedures (`tools/re/viewer-2026-10-19/bdlg/proc_events.txt`).
pub fn proc_event(dialog: usize, link: u16, effects: u8) -> Option<(u16, u32, u32)> {
    let pressed = effects & 1 != 0;
    Some(match (dialog, link) {
        (15, 465) | (14, 404) | (18, 1083) | (19, 1097) | (16, 970) | (17, 1025) | (23, 1605) | (22, 328) | (20, 1387) | (24, 1363) => (0x198, 0, 0),
        (15, 466) | (14, 406) | (18, 1085) | (19, 1098) | (16, 971) | (17, 1026) | (23, 1606) | (22, 329) | (20, 1388) | (24, 1364) => (0x194, 0, 0),
        (14, 405) | (18, 1084) | (22, 347) => (0x193, 0, 0),
        (14, 407) => (0x196, 0, pressed as u32),
        (18, 1086) | (19, 1099) | (16, 972) | (17, 1027) | (23, 1607) | (22, 330) | (20, 1389) | (24, 1365) => (0x196, 0, !pressed as u32),
        (14, 417) => (0x197, 0, pressed as u32),
        (43, 1799) => (0x195, 1, 0),
        (43, 1800) => (0x195, 0, 0),
        // D16: "more soldiers" (0x1bb, l = −1), "fewer" (0x1bc, −1), a soldier's button (0x1bc;
        // w = 1 top / 0 bottom, l = the slot).
        (16, 999) => (0x1bb, 0, u32::MAX),
        (16, 1000) => (0x1bc, 0, u32::MAX),
        (16, 1001..=1010) => (0x1bc, 1, (link - 1001) as u32),
        (16, 1011..=1020) => (0x1bc, 0, (link - 1011) as u32),
        (17, 1061) => (0x199, 0, 0),
        (24, 1383) => (0x1bf, 0, 0),
        // The sidebar buttons: closed → 0x1cc, opened → the dialog's sidebar. D19's 1138 sends
        // 0x1c4 / 0x1c5 by the building type (tool / weapon smith, 0x4a06cd), D22's 344 0x1c3 for
        // type 24 (the barracks; info +8, 0x46ca83) else 0x1c6; here 0x1c4 / 0x1c6 and the
        // caller picks.
        (19, 1138) | (17, 1057) | (17, 1060) | (23, 1623) | (22, 344) | (24, 1381) if !pressed => (0x1cc, 0, 0),
        (19, 1138) => (0x1c4, 0, 0),
        (17, 1057) => (0x1c8, 0, 0),
        (17, 1060) => (0x1c7, 0, 0),
        (23, 1623) => (0x1c9, 0, 0),
        (22, 344) => (0x1c6, 0, 0),
        (24, 1381) => (0x1cb, 0, 0),
        // D41 (the cart / ship dialog, 0x4ae300).
        (crate::vehicle_dialog::DIALOG, l) => (crate::vehicle_dialog::proc_event(l)?, 0, 0),
        _ => return None,
    })
}

/// The trade-target click (`CInputProcessor` state 7, handler 0x4eaf90): every building on the
/// map-object layer within spiral `Last(5)` of the click that 0x552960 accepts as the selected
/// trading building's partner gets a 0x138d `SetTradeTarget` (the loop does not stop at the
/// first). 0x552960 (the trading role, `+6` its building): both READY (0x2000000) and both
/// positions visible (`CFogging::IsPositionVisible` 0x5aee70, `visible`); a port (0x20) target
/// with a port; a port with another building, or a marketplace (0x21), when the two +0x48 nodes
/// lie in the same sector ([`crate::population::Building::land`]); nothing else. Nothing
/// excludes the trading building itself.
pub fn trade_targets(pop: &crate::population::Population, f: &Facts, (x, y): (i32, i32), visible: &dyn Fn(i32, i32) -> bool) -> Vec<u16> {
    let Some(me) = pop.buildings.iter().find(|b| b.id == f.id) else { return Vec::new() };
    let mut out = Vec::new();
    for i in 0..=s4_sim::spiral::last(5) {
        let (px, py) = (x + s4_sim::spiral::dx(i), y + s4_sim::spiral::dy(i));
        let Some(b) = pop.buildings.iter().find(|b| b.id != 0 && (b.x as i32, b.y as i32) == (px, py)) else { continue };
        if !(b.ready && me.ready && visible(me.x as i32, me.y as i32) && visible(px, py)) {
            continue;
        }
        let ok = match b.generic {
            32 if me.generic == 32 => true,
            32 | 33 => b.land == me.land,
            _ => false,
        };
        if ok {
            out.push(b.id);
        }
    }
    out
}

// ------------------------------------------------------------------------------------------------
// Sidebars

/// The sidebar a GUI event asks for (`CInputProcessor` handlers 0x4ebad0 / 0x4ebb20 /
/// 0x4ebc10 / 0x4eba80: they register the role's sidebar fill as `CLogic`'s periodic provider at
/// 0x14540d0 +0x10..0x18, which sends 0x25f once, then 0x25e refreshes): 0x1c4 tool smith
/// (0x54d480, code 0xc), 0x1c5 weapon smith (0x54d660, code 0xb), 0x1c9 storage (0x550a80, code
/// 0xf), 0x1c3 barracks (0x535150 through the provider 0x53d740, code 0x10), 0x1c7 trading
/// (0x4ebb70: 0x553300 through 0x53d7a0, code 0xe, D53 [`crate::trade_sidebar`]).
/// 0x1c8 (0x4ebbc0: 0x5531b0 through 0x53d7c0, code 0xd, D54 [`crate::trade_sidebar::TradeForeign`]),
/// which fills and sends nothing without the role's `CInternationalTrader` handle (+0x2a4).
/// 0x1cb workshop (0x4ebca0: 0x5597b0 through 0x53d920, code 0x11, D51
/// [`crate::workshop_sidebar`]): [not filled yet: the sim's workshop role is on a branch]. 0x1c6 (D22's
/// button for a type other than 24, 0x46cab8) has no handler.
pub fn sidebar_fill(infos: &mut Infos, f: &Facts, event: u16) -> Option<usize> {
    match event {
        0x1c4 if f.kind == 21 => {
            // 0x54d480: by tool good, the order count into +8..+0xe.
            let c = 0xc;
            infos.code_dword(c);
            for &(good, count, _) in &f.orders {
                let off = match good {
                    4 => 8,
                    0x12 => 9,
                    0x19 => 0xa,
                    0x1b => 0xb,
                    0x1c => 0xe,
                    0x1d => 0xc,
                    0x1f => 0xd,
                    _ => continue,
                };
                infos.set(c, off, count);
            }
            Some(c)
        }
        0x1c5 if f.kind == 22 => {
            // 0x54d660: per product (good, order count dword, percentage) at +0xc / +0x10 /
            // +0x14, 12 bytes apart; +8 auto-production on.
            let c = 0xb;
            for (k, &(good, count, pct)) in f.orders.iter().take(4).enumerate() {
                infos.set(c, 0xc + 12 * k, good);
                infos.obj[c][0x10 + 12 * k..0x14 + 12 * k].copy_from_slice(&(count as u32).to_le_bytes());
                infos.set(c, 0x14 + 12 * k, pct);
            }
            infos.code_dword(c);
            infos.set(c, 8, (f.weapon_auto == 100) as u8);
            Some(c)
        }
        0x1cb if f.house_kind == hk::WORKSHOP => {
            // CWorkshopBuildingRole 0x5597b0: three records at +8 {type, order count, boards,
            // ironbars}: the types 4, 5, 0 of the vehicle hall (type 23, .rdata 0x101a3d4), else
            // 1, 2, 3 (0x101a3e8); a type without a slot (pairs 0x1165134: 1..=5) leaves its
            // record as it was (the hall's third).
            let c = 0x11;
            let w = f.workshop.clone().unwrap_or_default();
            let types: [u8; 3] = if f.kind == 23 { [4, 5, 0] } else { [1, 2, 3] };
            for (k, &t) in types.iter().enumerate() {
                if !(1..=5).contains(&t) {
                    continue;
                }
                infos.set(c, 8 + 4 * k, t);
                infos.set(c, 9 + 4 * k, w.orders[t as usize - 1]);
                let (boards, iron) = w.costs[t as usize];
                infos.set(c, 10 + 4 * k, boards);
                infos.set(c, 11 + 4 * k, iron);
            }
            infos.code_dword(c);
            Some(c)
        }
        0x1c3 if f.house_kind == hk::BARRACK => {
            // CBarrackRole 0x535150: the door eco-sector's ten soldier orders (+0x33c..) into
            // +8..+0x11, its carriers over the minimum (at least 0) into the dword +0x14.
            let c = 0x10;
            infos.code_dword(c);
            for (k, &v) in f.soldier_orders.iter().enumerate() {
                infos.set(c, 8 + k, v as u8);
            }
            infos.obj[c][0x14..0x18].copy_from_slice(&(f.free_carriers as u32).to_le_bytes());
            Some(c)
        }
        0x1c7 if f.house_kind == hk::TRADE => {
            // 0x553300: +8..+0x32 cleared, then per good with the domestic flag (+0x1d6 bit 1)
            // the amount still to send (+0x1ab).
            let c = 0xe;
            infos.code_dword(c);
            infos.obj[c][8..8 + 43].fill(0);
            for g in 0..43 {
                if f.trade_flags.get(g).is_some_and(|&b| b & 2 != 0) {
                    infos.set(c, 8 + g, f.trade_send.get(g).copied().unwrap_or(0) as u8);
                }
            }
            Some(c)
        }
        0x1c8 if f.house_kind == hk::TRADE && f.trader => {
            // 0x5531b0: +8..+0x10 cleared, +8 + p = the trader's +0x14 + p for p = 1..=PlayerCount,
            // +0x11 + good = the building's record state (0 without a record).
            let c = 0xd;
            infos.code_dword(c);
            infos.obj[c][8..0x11].fill(0);
            for p in 1..=(f.player_count as usize).min(8) {
                infos.set(c, 8 + p, f.trade_with[p] as u8);
            }
            for g in 0..43 {
                infos.set(c, 0x11 + g, f.foreign.get(g).copied().unwrap_or(0));
            }
            Some(c)
        }
        0x1c9 if f.house_kind == hk::STORAGE => {
            // 0x550a80: (good, stored) for goods 0..43 from +8.
            let c = 0xf;
            infos.code_dword(c);
            for k in 0..43 {
                infos.set(c, 8 + 2 * k, k as u8);
                infos.set(c, 9 + 2 * k, f.store.get(k).copied().unwrap_or(false) as u8);
            }
            Some(c)
        }
        _ => None,
    }
}

/// D50's good → link table by race (built by its procedure init 0x467fd0 from the race; -1 = no
/// control), dumped by running it.
const STORAGE_LINKS: [[i16; 43]; 5] = [
    [0, -1, 208, 206, 197, -1, -1, 190, 205, 209, 196, 210, 212, -1, 195, 194, 213, -1, 198, -1, 193, 192, 189, -1, 211, 199, -1, 200, 203, 201, -1, 202, 191, 207, 204, -1, 214, 218, -1, -1, -1, -1, -1],
    [0, -1, -1, 206, 197, 219, -1, 190, 205, 209, 196, 210, 212, -1, 195, 194, 213, -1, 198, 220, 193, 192, 189, 218, 211, 199, -1, 200, 203, 201, -1, 202, 191, 207, 204, -1, 214, -1, -1, -1, -1, -1, -1],
    [0, 220, -1, 206, 197, -1, 219, 190, 205, 209, 196, 210, 212, -1, 195, 194, 213, 208, 198, -1, 193, 192, 189, -1, 211, 199, -1, 200, 203, 201, -1, 202, 191, 207, 204, 218, 214, -1, -1, -1, -1, -1, -1],
    [0, -1, 208, 206, 197, -1, -1, 190, 205, 209, 196, 210, 212, -1, 195, 194, 213, -1, 198, -1, 193, 192, 189, -1, 211, 199, -1, 200, 203, 201, -1, 202, 191, 207, 204, -1, 214, 218, -1, -1, -1, -1, -1],
    [0, -1, -1, 206, 197, -1, -1, 190, 205, 209, 196, 210, 212, -1, 195, 194, 213, -1, 198, -1, 193, 192, 189, -1, 211, 199, -1, 200, 203, 201, -1, 202, 191, 207, 204, -1, 214, -1, 219, -1, 208, 218, 220],
];

/// The smiths' count text (0x468c40 and its copies in 0x4aecb0): "0" hidden below 1, "E" from
/// 100 (unlimited), else "%u".
fn order_text(v: i32) -> (String, bool) {
    if v < 1 {
        ("0".into(), false)
    } else if v > 99 {
        ("E".into(), true)
    } else {
        (v.to_string(), true)
    }
}

/// The weapon smith's per-good links (0x4aecb0 / 0x4af090): count text, the two slider controls
/// it enables (track, knob) and the percentage text.
fn weapon_links(good: i8) -> Option<(u16, u16, u16, u16)> {
    Some(match good {
        3 => (1772, 1785, 1782, 1793),
        5 => (2913, 1787, 1786, 1794),
        6 => (1789, 1787, 1786, 1794),
        8 => (1774, 1784, 1781, 1792),
        34 => (1773, 1783, 1780, 1791),
        38 => (2889, 1787, 1786, 1794),
        _ => return None,
    })
}

/// A sidebar's update (0x488e00's switch for codes 0xb, 0xc, 0xf). With `init` also what the
/// procedure's init (code 0) adds: D46's slider knobs and percentages (0x4af090), D50's table is
/// by race (0x467fd0).
pub fn sidebar_update(code: usize, infos: &Infos, st: &mut GuiState, init: bool) {
    let d = DIALOGS[code];
    let mut ui = Ui { st, d, s: &|_| String::new() };
    let i = |o: usize| infos.i(code, o);
    match code {
        0xc => {
            // 0x468cd0: seven 0x468c40 (link, value).
            for (link, off) in [(236, 8), (234, 9), (239, 0xa), (240, 0xb), (238, 0xc), (235, 0xd), (237, 0xe)] {
                let (t, v) = order_text(i(off) as i32);
                ui.text(link, t);
                ui.visible(link, v);
            }
        }
        0xb => {
            // 0x4aecb0: auto-production pressed when +8 is clear (the toggle reads "off").
            ui.select(1778, infos.b(0xb, 8) == 0);
            for k in 0..4 {
                let good = i(0xc + 12 * k);
                let Some((text, a, b, pct)) = weapon_links(good) else { continue };
                ui.enable(a, infos.b(0xb, 8) != 0);
                ui.enable(b, infos.b(0xb, 8) != 0);
                let (t, v) = order_text(infos.dword(0xb, 0x10 + 12 * k) as i32);
                ui.text(text, t);
                ui.visible(text, v);
                if init {
                    // 0x4af090 code 0: knob image 0x11f / 0x151, its position, "%u%%".
                    ui.st.images.insert((d, a), (0x11f, 0x151));
                    ui.st.set_slider(d, a, i(0x14 + 12 * k) as i32 as u32);
                    ui.text(pct, format!("{}%", infos.b(0xb, 0x14 + 12 * k)));
                }
            }
        }
        0x10 => {
            // 0x467720: the order count of each soldier type under its tab (tables 0xffbea8 /
            // 0xffbe90 / 0xffbe78: swordsmen +8.., bowmen +0xb.., the race's third +0xe..; the
            // squad leader +0x11 on 159), then the free carriers ("%u" of the dword +0x14) on 167.
            for (k, link) in [150, 151, 152, 153, 154, 155, 156, 157, 158, 159].into_iter().enumerate() {
                let (t, v) = order_text(i(8 + k) as i32);
                ui.text(link, t);
                ui.visible(link, v);
            }
            ui.text(167, infos.dword(0x10, 0x14).to_string());
        }
        0xf => {
            // 0x467df0: each (good, stored) pair presses or releases the good's radio.
            let race = match infos.gui_race {
                r @ 0..=4 => r as usize,
                _ => 0,
            };
            for k in 0..43 {
                let (good, flag) = (infos.b(0xf, 8 + 2 * k) as usize, infos.b(0xf, 9 + 2 * k));
                let Some(&link) = STORAGE_LINKS[race].get(good) else { continue };
                if link < 0 {
                    continue;
                }
                ui.select(link as u16, flag == 1);
            }
        }
        _ => {}
    }
}

/// A sidebar's procedure, for code 3 on (dialog, link) with the control's `effects` after the
/// click: the GUI event it sends, given `selected`, the procedure's selected good (D46 0x15aad24,
/// D47 0x15aa4f8) or soldier type (D48 0x15aa444), which tab clicks set; `race` = the local
/// player's (D48's third soldier kind).
pub fn sidebar_event(dialog: usize, link: u16, effects: u8, selected: &mut u8, race: u8) -> Option<(u16, u32, u32)> {
    let pressed = effects & 1 != 0;
    let amount = |l: u16, base: u16| -> u32 {
        match l - base {
            0 => 100,
            1 => 0,
            2 => 5,
            3 => (-5i32) as u32,
            4 => 1,
            _ => u32::MAX,
        }
    };
    match (dialog, link) {
        // D47 0x468d50: tabs 227..=233 select a tool, 221..=226 order it (0x1ce).
        (47, 227..=233) => {
            let g = [4, 0x12, 0x19, 0x1b, 0x1d, 0x1f, 0x1c][(link - 227) as usize];
            *selected = if pressed { g } else { 0 };
            None
        }
        (47, 221..=226) if *selected != 0 => Some((0x1ce, *selected as u32, amount(link, 221))),
        // D46 0x4af090: tabs select a weapon, 1766..=1771 order it, 1778 auto-production
        // (0x1d0).
        (46, 1775 | 1776 | 1777 | 2888 | 2912 | 1788) => {
            let g = match link {
                1775 => 0x22,
                1776 => 8,
                1777 => 3,
                2888 => 0x26,
                2912 => 5,
                _ => 6,
            };
            *selected = if pressed { g } else { 0 };
            None
        }
        (46, 1766..=1771) => Some((0x1ce, *selected as u32, amount(link, 1766))),
        // D48 0x467970 (checked in the emulator, `tools/re/gui-2026-10-20/d48_proc.py`): tabs
        // 140..=149 select the soldier type (146..=148 the race's third kind: Viking 0x26..,
        // Maya 0x29.., Trojan 0x3d.., else 0x23..), 160..=165 send 0x1cd (type, 100 / 0 / +5 /
        // −5 / +1 / −1) whatever is selected.
        (48, 140..=149) => {
            let third = match race {
                1 => 0x26,
                2 => 0x29,
                4 => 0x3d,
                _ => 0x23,
            };
            *selected = match link {
                140..=145 => 0x1d + (link - 140) as u8,
                146..=148 => third + (link - 146) as u8,
                _ => 0x2c,
            };
            None
        }
        (48, 160..=165) => Some((0x1cd, *selected as u32, amount(link, 160))),
        (46, 1778) => Some((0x1d0, 0, pressed as u32)),
        // D50 0x468270: a good's radio → 0x1d4 (good, pressed).
        (50, _) => {
            let good = STORAGE_LINKS.iter().find_map(|t| t.iter().position(|&l| l == link as i16))?;
            Some((0x1d4, good as u32, pressed as u32))
        }
        _ => None,
    }
}

include!("building_dialog_tables.rs");

#[cfg(test)]
mod temple_tests {
    use super::*;

    /// D20's fill 0x54fe40: the offering pile's good / amount at +0x14 / +0x15, the mana dword
    /// at +0x18, +0x16 / +0x17 untouched.
    #[test]
    fn small_temple_pile_and_mana() {
        let f = Facts { house_kind: hk::SMALLTEMPLE, product: Some((37, 4)), mana: 1234, ..Default::default() };
        let mut infos = Infos::default();
        infos.obj[8][0x16] = 0x55;
        assert_eq!(fill(&mut infos, &f), Some(8));
        let o = &infos.obj[8];
        assert_eq!((o[0x14], o[0x15], o[0x16]), (37, 4, 0x55));
        assert_eq!(i32::from_le_bytes(o[0x18..0x1c].try_into().unwrap()), 1234);
    }
}

#[cfg(test)]
mod trade_tests {
    use super::*;

    /// The donkey's 0x577240 (code 0x18): the object cleared after +4, two slots at +8 / +0xc and
    /// +0x14 / +0x18; the transport ship's 0x591dc0 (0x16) keeps the rest of the object.
    #[test]
    fn donkey_and_transport_ship_fills() {
        let mut infos = Infos::default();
        infos.vehicle[0x80] = 7;
        let f = Facts { donkey: Some([(8, 2), (0, 0)]), ..Default::default() };
        assert_eq!(fill(&mut infos, &f), Some(DONKEY_CODE));
        let d = |infos: &Infos, o: usize| i32::from_le_bytes(infos.vehicle[o..o + 4].try_into().unwrap());
        assert_eq!((d(&infos, 4), d(&infos, 8), d(&infos, 0xc), d(&infos, 0x14), d(&infos, 0x18), infos.vehicle[0x80]), (0x18, 8, 2, 0, 0, 0));
        assert_eq!(dialog_of(DONKEY_CODE), crate::vehicle_dialog::DIALOG);
        infos.vehicle[0x80] = 7;
        infos.vehicle[0xf9] = 1;
        let mut slots = [[0; 2]; 8];
        slots[7] = [3, 9];
        let f = Facts { transport: Some(slots), ..Default::default() };
        assert_eq!(fill(&mut infos, &f), Some(TRANSPORT_CODE));
        assert_eq!((d(&infos, 4), d(&infos, 8 + 12 * 7), d(&infos, 12 + 12 * 7), infos.vehicle[0x80], infos.vehicle[0xf9]), (0x16, 3, 9, 7, 0));
    }
    use crate::population::{Building, Population};

    fn b(id: u16, x: u16, generic: u16, land: u16, ready: bool) -> Building {
        Building { id, x, y: 10, kind: generic as u8, owner: 1, race: 0, inhabitants: vec![], site: None, worker: None, door: (x, 10), land, ready, generic, flag: None, pen: None, mushroom: None }
    }

    /// 0x552960 through the click loop: a marketplace takes marketplaces and ports on its land,
    /// a port takes any port; neither a building in the fog, one not READY, nor another type.
    /// Nothing excludes the trading building itself (at the click's centre here).
    #[test]
    fn trade_targets_like_0x552960() {
        let others = vec![b(2, 12, 33, 1, true), b(3, 13, 33, 2, true), b(4, 11, 32, 1, true), b(5, 9, 32, 3, true), b(6, 8, 33, 1, false), b(7, 7, 33, 1, true), b(8, 14, 12, 1, true)];
        let all = |_: i32, _: i32| true;
        let fog = |x: i32, _: i32| x != 7;
        let run = |me: Building, vis: &dyn Fn(i32, i32) -> bool| {
            let pop = Population { buildings: [vec![me], others.clone()].concat(), ..Default::default() };
            let mut t = trade_targets(&pop, &Facts { id: 1, ..Default::default() }, (10, 10), vis);
            t.sort();
            t
        };
        assert_eq!(run(b(1, 10, 33, 1, true), &all), [1, 2, 4, 7]);
        assert_eq!(run(b(1, 10, 33, 1, true), &fog), [1, 2, 4]);
        assert_eq!(run(b(1, 10, 32, 1, true), &all), [1, 2, 4, 5, 7]);
        assert_eq!(run(b(1, 10, 33, 1, false), &all), Vec::<u16>::new());
    }
}
