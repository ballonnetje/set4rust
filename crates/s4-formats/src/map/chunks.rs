//! Typed map chunks. Each struct names the S4_Main code that consumes the bytes; fields without a
//! known consumer are kept as `unknown_*`.

use crate::reader::Reader;
use crate::{Result, bail};

/// Chunk 1, `SEditorGlobalMapData` (24 bytes, copied verbatim to `g_sGeneralMapData` 0x472756c by
/// `MA_OpenMapFile`; read back by `MA_GetMapData` 0x2fbdc80 and `CGameType::LoadMapData`
/// 0x1491490). The random-map generator writes the same struct (`CMapGeneratorHost::Init`
/// 0x1498520: +4 and +12 set, rest zero).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeneralInfo {
    /// +0. Bit 0x2 → `CGameType::m_iMode` = 2, else 1. Other bits: no reader found.
    pub flags: u32,
    /// +4. Not read by `MA_GetMapData`. Equals the chunk-2 player count in all 378 shipped maps,
    /// and the random-map generator stores its player count here (`CMapGeneratorHost::Init`,
    /// `this[73] = this[11]`) — so named from data, not from a reader.
    pub player_count: u32,
    /// +8 → `CGameType::m_iStartResources`.
    pub start_resources: u32,
    /// +12 → `CGameType::m_iWidthHeight` (map side in nodes).
    pub size: u32,
    /// +16. No reader found; 0 in all shipped maps.
    pub unknown_10: u32,
    /// +20. Option bits read in `CGameType::LoadMapData`:
    /// 0x1 → byte 0x36A (only if no settlers/buildings), 0x2 → byte 0x36B,
    /// 0x4 → !byte 0x36C (only if no settlers/buildings), 0x8 → !byte 0x369 (when clear the
    /// preview is loaded), 0x10 → byte 0x364, 0x20 → byte 0x365 (both only with >1 team),
    /// 0x40 → `m_bBlueByteAddOnMap`, 0x80 → forces `m_iMode` = 1 in one branch,
    /// 0x100 → `m_bBlueByteMCD2Map`, 0x200 → `m_bIsMCD2TextureSet`.
    pub options: u32,
}

impl GeneralInfo {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let mut r = Reader::new(d, "general info");
        Ok(Self {
            flags: r.u32()?,
            player_count: r.u32()?,
            start_resources: r.u32()?,
            size: r.u32()?,
            unknown_10: r.u32()?,
            options: r.u32()?,
        })
    }
    /// `CGameType::m_iMode` as `LoadMapData` derives it from `flags`.
    pub fn mode(&self) -> u32 {
        if self.flags & 2 != 0 { 2 } else { 1 }
    }
}

/// One entry of chunk 2 (`CPlayerData::m_sPlayers`, 45 bytes; `CPlayerData::Load` 0x2fbf900
/// copies `45 * param` bytes). Accessors: `GetRaceOfPlayer` 0x2fbf390, `GetXOfPlayer` 0x2fbf3d0,
/// `GetYOfPlayer` 0x2fbf410, `GetNameOfPlayer` 0x2fbf560.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    /// 1-based player id (index in the chunk + 1).
    pub id: u8,
    /// +0 u32. `CGameType::LoadMapData` treats 255 as "random race" (sets 5 + a flag).
    pub race: u32,
    /// +4 u32 start position.
    pub start_x: u32,
    /// +8 u32.
    pub start_y: u32,
    /// +12 char[33], NUL-terminated, ANSI code page.
    pub name: String,
}

/// Chunk 3 (`CPlayerData::LoadTeamData` 0x2fbf980): subtype = setup index (< 10).
/// Payload: char[33] setup name, then per player {u8 team, u8 control}
/// (`GetTeamOfPlayer` 0x2fbf610 / `GetControlOfPlayer` 0x2fbf5c0).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TeamSetup {
    pub index: u16,
    pub name: String,
    /// Indexed by player id - 1.
    pub slots: Vec<TeamSlot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TeamSlot {
    /// 255 = no fixed team (`MA_GetPlayerData` reports it as a separate flag).
    pub team: u8,
    /// `LoadMapData`: 0 = slot open for a human, 2 = AI-only; other values counted as
    /// "occupied" slots. Exact meaning beyond that unknown.
    pub control: u8,
}

/// Chunk 66, `EDITOR_INFO` (20 bytes, copied to `g_sEditorInfo` 0x4727558).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EditorInfo {
    pub unknown_00: u16,
    /// +2 → `MA_GetDataChecksums` first value → `CGameType::m_iMapBuildingXMLVersion`.
    pub building_xml_version: u16,
    /// +4 → second value → `CGameType::m_iMapObjectXMLVersion`.
    pub object_xml_version: u16,
    pub unknown_06: [u8; 14],
}

impl EditorInfo {
    pub fn parse(d: &[u8]) -> Result<Self> {
        let mut r = Reader::new(d, "editor info");
        Ok(Self {
            unknown_00: r.u16()?,
            building_xml_version: r.u16()?,
            object_xml_version: r.u16()?,
            unknown_06: r.bytes(14)?.try_into().unwrap(),
        })
    }
}

/// One landscape node: chunk 13 (`T_GFX_MAP_ELEMENT`) merged with chunk 6 (map elements),
/// both read in `CWorldManager::LoadGfxData` (0x16a2a20).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Node {
    /// chunk 13 +0 (`T_GFX_MAP_ELEMENT::a`): terrain height.
    pub height: u8,
    /// chunk 13 +1 `m_uGroundType`. `CLandscapeProperties::IsWater` = `<= 8`;
    /// 28 / 29 get special move costs in `LoadGfxData`. See [`ground_rgb`].
    pub ground: u8,
    /// chunk 13 +2 `m_uGradient`: low nibble = shading gradient (`GetGradient` 0x151aa30);
    /// 0x40 → ORed into the flag map as 0x4 by `LoadGfxData`; 0x80 suppresses the gradient in
    /// the renderer.
    pub gradient: u8,
    /// chunk 13 +3 `m_uGfxBits`: bits 0..2 old fog, 3..5 new fog (`GetOldFogging` 0x15db1d0 /
    /// `GetNewFogging` 0x15db1b0), 0x40 used by the renderer. Rest unknown.
    pub gfx_bits: u8,
    /// chunk 6 +0: landscape object id; non-zero ids other than 162/209/210/211 become deco
    /// objects (`CDecoObjMgr::AddDecoObjWithoutFlags`).
    pub object: u8,
    /// chunk 6 +1: owner player, fed to `CWorldManager::StartUpOwner` (ignored on water).
    pub owner: u8,
    /// chunk 6 +2: flag map byte (`m_cFlagMap`). 0x2 → blocked (move cost 7); the random-map
    /// generator sets 0x1 on `IsBlockedLand` ground. Other bits unknown.
    pub flags: u8,
    /// chunk 6 +3: resource; high nibble type, low nibble amount
    /// (`CWorldManager::SetResource(x, y, v & 0xF0, v & 0xF)`).
    pub resource: u8,
}

impl Node {
    pub fn resource_type(&self) -> u8 {
        self.resource & 0xF0
    }
    pub fn resource_amount(&self) -> u8 {
        self.resource & 0x0F
    }
    pub fn is_water(&self) -> bool {
        self.ground <= 8
    }
}

/// The whole node grid, row-major (`index = y * width + x`).
#[derive(Clone, Debug)]
pub struct Landscape {
    pub width: u32,
    pub height: u32,
    pub nodes: Vec<Node>,
}

impl Landscape {
    pub fn parse(size: u32, gfx: &[u8], elements: Option<&[u8]>) -> Result<Self> {
        let n = size as usize * size as usize;
        if gfx.len() != n * 4 {
            bail!("landscape: {} bytes for a {size}x{size} map", gfx.len());
        }
        if let Some(e) = elements
            && e.len() != n * 4
        {
            bail!("map elements: {} bytes for a {size}x{size} map", e.len());
        }
        let mut nodes = Vec::with_capacity(n);
        for i in 0..n {
            let g = &gfx[i * 4..i * 4 + 4];
            let e = elements.map_or([0; 4], |e| e[i * 4..i * 4 + 4].try_into().unwrap());
            nodes.push(Node {
                height: g[0],
                ground: g[1],
                gradient: g[2],
                gfx_bits: g[3],
                object: e[0],
                owner: e[1],
                flags: e[2],
                resource: e[3],
            });
        }
        Ok(Self { width: size, height: size, nodes })
    }

    pub fn node(&self, x: u32, y: u32) -> Option<&Node> {
        (x < self.width && y < self.height).then(|| &self.nodes[(y * self.width + x) as usize])
    }
}

/// A landscape object placed by chunk 6 (tree, stone, deco...).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapObject {
    pub x: u32,
    pub y: u32,
    pub id: u8,
}

/// Chunk 7 record (6 bytes), `CSettlerMgr::LoadSettlerData` (0x1580eb0).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settler {
    /// +0 u16, +2 u16.
    pub x: u16,
    pub y: u16,
    /// +4: `>= 200` → vehicle of type `kind - 200` (`CVehicleMgr::AddVehicle`), `< 67` → settler
    /// type (`CSettlerMgr::AddSettler`), 67..199 ignored by the loader.
    pub kind: u8,
    /// +5: owner player (asserted 1..8).
    pub owner: u8,
}

impl Settler {
    pub fn parse_all(d: &[u8], count: u32) -> Result<Vec<Self>> {
        records(d, count, 6, "settlers", |r| {
            Ok(Self { x: r.u16()?, y: r.u16()?, kind: r.u8()?, owner: r.u8()? })
        })
    }
    pub fn vehicle_type(&self) -> Option<u8> {
        (self.kind >= 200).then(|| self.kind - 200)
    }
}

/// Chunk 8 record (20 bytes), `CBuildingMgr::LoadBuildingData`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Building {
    pub x: u16,
    pub y: u16,
    /// +4 building type (0 = skipped, asserted < 83).
    pub kind: u8,
    /// +5 owner player.
    pub owner: u8,
    /// +6: for military buildings, non-zero → the garrison counts below are applied.
    /// 1 in every shipped map.
    pub has_garrison: u8,
    /// +7..+16: `soldiers[j]` × `CBuildingMgr::AddSoldierToStartTower(id, j)`.
    pub soldiers: [u8; 9],
    /// +16..+20: no reader; all zero in shipped maps.
    pub unknown_10: [u8; 4],
}

impl Building {
    pub fn parse_all(d: &[u8], count: u32) -> Result<Vec<Self>> {
        records(d, count, 20, "buildings", |r| {
            Ok(Self {
                x: r.u16()?,
                y: r.u16()?,
                kind: r.u8()?,
                owner: r.u8()?,
                has_garrison: r.u8()?,
                soldiers: r.bytes(9)?.try_into().unwrap(),
                unknown_10: r.bytes(4)?.try_into().unwrap(),
            })
        })
    }
}

/// Chunk 9 record (8 bytes), `CPileMgr::LoadPileData` (0x155e970).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pile {
    pub x: u16,
    pub y: u16,
    /// +4 good type (asserted <= 42, `GOOD_MAX`).
    pub good: u8,
    /// +5 amount (asserted <= `CPile::MAX_PILE_AMOUNT`).
    pub amount: u8,
    /// +6: 254 → pile role type 3, anything else → 2 (`IPileRole` type). Always 254 in shipped maps.
    pub kind_flag: u8,
    /// +7: no reader; mostly 0, also 1, 205, 3, 2, 64, ... (23 distinct values).
    pub unknown_07: u8,
}

impl Pile {
    pub fn parse_all(d: &[u8], count: u32) -> Result<Vec<Self>> {
        records(d, count, 8, "piles", |r| {
            Ok(Self { x: r.u16()?, y: r.u16()?, good: r.u8()?, amount: r.u8()?, kind_flag: r.u8()?, unknown_07: r.u8()? })
        })
    }
    pub fn role_type(&self) -> u8 {
        if self.kind_flag == 254 { 3 } else { 2 }
    }
}

fn records<T>(
    d: &[u8],
    count: u32,
    size: usize,
    what: &'static str,
    f: impl Fn(&mut Reader) -> Result<T>,
) -> Result<Vec<T>> {
    if d.len() != count as usize * size {
        bail!("{what}: {} bytes for {count} records of {size}", d.len());
    }
    let mut r = Reader::new(d, what);
    (0..count).map(|_| f(&mut r)).collect()
}

pub fn parse_players(d: &[u8], count: u32) -> Result<Vec<Player>> {
    records(d, count, 45, "players", |r| {
        let id = (r.pos() / 45 + 1) as u8;
        Ok(Player { id, race: r.u32()?, start_x: r.u32()?, start_y: r.u32()?, name: cstr(r.bytes(33)?) })
    })
}

pub fn parse_team_setup(index: u16, d: &[u8], players: u32) -> Result<TeamSetup> {
    if d.len() != 33 + 2 * players as usize {
        bail!("team setup {index}: {} bytes for {players} players", d.len());
    }
    let slots = d[33..].chunks(2).map(|s| TeamSlot { team: s[0], control: s[1] }).collect();
    Ok(TeamSetup { index, name: cstr(&d[..33]), slots })
}

/// NUL-terminated ANSI text (the game converts with `MultiByteToWideChar(CP_ACP)`); decoded as
/// Windows-1252 here.
pub fn cstr(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    b[..end].iter().map(|&c| cp1252(c)).collect()
}

fn cp1252(c: u8) -> char {
    const HI: [u16; 32] = [
        0x20AC, 0x81, 0x201A, 0x0192, 0x201E, 0x2026, 0x2020, 0x2021, 0x02C6, 0x2030, 0x0160,
        0x2039, 0x0152, 0x8D, 0x017D, 0x8F, 0x90, 0x2018, 0x2019, 0x201C, 0x201D, 0x2022, 0x2013,
        0x2014, 0x02DC, 0x2122, 0x0161, 0x203A, 0x0153, 0x9D, 0x017E, 0x0178,
    ];
    match c {
        0x80..=0x9F => char::from_u32(HI[(c - 0x80) as usize] as u32).unwrap_or('?'),
        _ => c as char,
    }
}

/// Ground colour per ground type: the RGB bytes `CLandscapeColorProperties`' constructor
/// (debug 0x2f8dcb0) stores at `this + 512 + 3*type` and `ColorEntry` (0x2f8eab0) returns; in the
/// release an initialised table at 0x11f6b40 (`this` = 0x11f6940; `ColorEntry` 0x664f60), equal
/// to this one. They are
/// plain 0..=255 channel values, not percentages: `HiColValue` (0x2fc5290, built at the end of the
/// constructor) converts them with `v · 31/255` to RGB555, and the minimap (`DrawCompleteMiniMap`
/// 0x2f8d020) adds `8·gradient − 64` and clamps to 255. The table only goes up to 110, so the
/// colours are dark. `None` for ground types the table leaves at zero (types the game does not
/// define).
pub fn ground_rgb(ground: u8) -> Option<[u8; 3]> {
    let rgb: [u8; 3] = match ground {
        0 => [0, 85, 110],
        1 => [0, 76, 101],
        2 => [0, 67, 92],
        3 => [0, 58, 83],
        4 => [0, 49, 74],
        5 => [0, 40, 65],
        6 => [0, 31, 54],
        7 => [0, 22, 47],
        8 => [100, 90, 50],
        16 => [0, 100, 0],
        17 => [10, 80, 10],
        18 => [30, 100, 70],
        20 => [50, 80, 0],
        21 => [30, 80, 0],
        23 => [20, 70, 40],
        24 => [40, 80, 20],
        25 => [60, 100, 30],
        28 => [98, 90, 90],
        29 => [70, 60, 60],
        32 => [40, 40, 30],
        33 => [20, 60, 20],
        35 => [65, 70, 65],
        48 => [100, 90, 50],
        64 => [100, 70, 0],
        65 => [80, 70, 0],
        80 => [60, 0, 0],
        81 => [40, 50, 0],
        96 => [50, 70, 100],
        97 => [50, 80, 100],
        98 => [60, 70, 100],
        99 => [50, 50, 100],
        128 => [88, 92, 98],
        129 => [76, 80, 82],
        144 => [40, 25, 55],
        145 => [30, 45, 50],
        _ => return None,
    };
    Some(rgb)
}
