//! What each numbered file in `Gfx\` holds, and how the game addresses it.
//!
//! Sources: `CGfxManager::OpenGFXFiles` 0x13616d0 (which files are opened, in which
//! `EnableGfxFile` mode), the `Get*GfxInfo` accessors (which file each object class reads, and how
//! the job/direction/frame levels are used), `CGameStateHandler` (GUI file 0 vs 9) and
//! `CGfxManager::LoadGfxFile` 0x1365750 (2/41 are the landscape textures `.gl5/.gl6/.gh5/.gh6`).
//! Race order everywhere is Roman, Viking, Maya, Dark Tribe, Trojan (`race + 10/20/30`).

use super::image::HeaderKind;

/// How the job/direction levels of a file are used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// `jil[job] -> dil[job_start + dir] -> gil[frame]` with a fixed number of "directions"
    /// per job (settlers, animals, vehicles, effects, piles, objects, accessories).
    Jobs,
    /// Files 10..=19: `jil[building] -> dil[first .. next building]`; each dil entry is one
    /// animation/part of the building, its gil range the frames (`GetBuildingGfxInfo`
    /// 0x1361af0: dil +0 = construction image (flag 2), +1 = finished building (flag 1),
    /// +2 = flag 8 overlay; further entries are `SBuildingFX` animations addressed directly by
    /// dil index). `GetJobFrameCount` 0x1363d10 has this special branch for 10 <= file < 20.
    Buildings,
    /// No .jil/.dil: images addressed by gil index directly, palette per image
    /// (GUI files and the user-logo files 1/35).
    Flat,
}

#[derive(Clone, Copy, Debug)]
pub struct FileInfo {
    pub number: u32,
    pub name: &'static str,
    pub layout: Layout,
    pub header: HeaderKind,
    /// Uses `.pil` + `.pa5/.pa6` (GUI mode) instead of `.pi2` + `.p2x`.
    pub gui_palette: bool,
    /// What the "direction" level means for this file.
    pub dir_meaning: &'static str,
}

const fn f(
    number: u32,
    name: &'static str,
    layout: Layout,
    header: HeaderKind,
    gui_palette: bool,
    dir_meaning: &'static str,
) -> FileInfo {
    FileInfo { number, name, layout, header, gui_palette, dir_meaning }
}

use HeaderKind::{Object as O, Settler as S};
use Layout::{Buildings as B, Flat as F, Jobs as J};

/// Every sprite file present in the History Edition install (2 and 41 are landscape textures,
/// owned by `gfx::landscape`; 25 is optional `g_bGfxHJBLoadSpecialGfx`; 33 ships only a
/// .jil/.sil/.pi2/.pi4 stub — the Dark Tribe has no vehicles).
pub const FILES: &[FileInfo] = &[
    f(0, "GUI: main menu (EnableGfxFile mode 9 when no game is running)", F, O, true, "-"),
    f(1, "User/player logos (written at runtime by CRTComp; 2 images per logo)", F, O, false, "-"),
    f(3, "Resource piles (GetPileGfxInfo: job = pile type)", J, O, false, "amount-1 (stack size)"),
    f(4, "Effects, 4444-palette variant (used when Use4444Palettes)", J, O, false, "direction"),
    f(5, "Landscape objects: trees, stones, waves, border stones (GetObjectGfxInfo)", J, O, false, "amount-1 / growth stage"),
    f(6, "Effects, 555/565-palette variant", J, O, false, "direction"),
    f(7, "Accessory icons above settlers (96 = ACCESSORY_MAX; MoveAccessoriesToGfxEngine)", J, O, false, "-"),
    f(8, "Animals (GetAnimalGfxInfo)", J, O, false, "direction (6)"),
    f(9, "GUI: in-game interface (EnableGfxFile mode 9 when a game runs)", F, O, true, "-"),
    f(10, "Buildings: Roman", B, O, false, "building part / animation"),
    f(11, "Buildings: Viking", B, O, false, "building part / animation"),
    f(12, "Buildings: Maya", B, O, false, "building part / animation"),
    f(13, "Buildings: Dark Tribe", B, O, false, "building part / animation"),
    f(14, "Buildings: Trojan", B, O, false, "building part / animation"),
    f(18, "GUI screen set (loaded via CGUIWrapper)", F, O, true, "-"),
    f(19, "GUI screen set (loaded via CGUIWrapper)", F, O, true, "-"),
    f(20, "Settlers: Roman", J, S, false, "direction (6)"),
    f(21, "Settlers: Viking", J, S, false, "direction (6)"),
    f(22, "Settlers: Maya", J, S, false, "direction (6)"),
    f(23, "Settlers: Dark Tribe", J, S, false, "direction (6)"),
    f(24, "Settlers: Trojan", J, S, false, "direction (6)"),
    f(25, "Settlers: special (optional, g_bGfxHJBLoadSpecialGfx)", J, S, false, "direction (6)"),
    f(28, "GUI screen set (loaded via CGUIWrapper)", F, O, true, "-"),
    f(29, "GUI screen set (loaded via CGUIWrapper)", F, O, true, "-"),
    f(30, "Vehicles/ships: Roman (GetVehicleGfxInfo)", J, O, false, "direction (6)"),
    f(31, "Vehicles/ships: Viking", J, O, false, "direction (6)"),
    f(32, "Vehicles/ships: Maya", J, O, false, "direction (6)"),
    f(34, "Vehicles/ships: Trojan", J, O, false, "direction (6)"),
    f(35, "Second logo set (same layout as file 1)", F, O, false, "-"),
    f(36, "Effects 2 (effect ids >= 100), 4444-palette variant", J, O, false, "direction"),
    f(37, "Effects 2 (effect ids >= 100), 555/565-palette variant", J, O, false, "direction"),
    f(39, "GUI screen set (loaded via CGUIWrapper)", F, O, true, "-"),
    f(40, "GUI screen set (loaded via CGUIWrapper)", F, O, true, "-"),
];

pub fn info(number: u32) -> Option<&'static FileInfo> {
    FILES.iter().find(|i| i.number == number)
}
