//! GUI dialog layouts: `Menu\GUISet*.DAT`.
//!
//! The game reads the whole file into `g_pDialogData` (`CStateGame` GUI setup, generated
//! `CStateGame.cpp:363`; `CStateMainMenu`, `CExtraCD::LoadMenuData`) and hands the raw pointer to
//! `IGuiEngine::Init` @0x2f9fff0 as `g_pFileHeader` (`struct GUI_MENU_FILE_HEADER`). Nothing is
//! copied or converted; the engine indexes into the buffer directly:
//! `IGuiEngine::OpenDialog` @0x2f9b380, `GetControlPtr` @0x2f9b870, `DrawControl` @0x2f9c0c0,
//! `IsNonTransparentGuiArea` @0x2f9ba00, `IGuiEngine::GetDialogDestinationRect` @0x2fa0b80.
//! The dialog-lookup helper (`sub_2F9E930`) is not decompiled; the offset table below is inferred
//! from the files (every offset lands exactly on a dialog, and dialogs tile the file).
//!
//! See `docs/GUISET.md` for the full layout and evidence. Field names follow the IDA struct
//! names where the decompilation uses them (`SGuiControl::valueLink`, `controlType`, `effects`,
//! `mainTexture`, `buttonPressedTexture`, `unknownId`).

use crate::reader::Reader;
use crate::{Result, bail};
use std::path::Path;

/// Sentinel for "no image / no text" in u16 id fields.
pub const NONE: u16 = 0xFFFF;
const CONTROL_SIZE: usize = 36;
const DIALOG_HEADER: usize = 16;

/// One `GUISet*.DAT` file.
#[derive(Clone, Debug)]
pub struct GuiSet {
    /// File header dword 0 (1 in every shipped file).
    pub version: u32,
    /// Dialogs by slot (the "dialog id" used by `OpenDialog`); `None` = empty slot.
    pub dialogs: Vec<Option<Dialog>>,
}

/// `GUI_MENU_DIALOG_HEADER` (16 bytes) + its controls.
#[derive(Clone, Debug)]
pub struct Dialog {
    /// Slot index in the file's offset table.
    pub slot: usize,
    /// Word 0: GUI surface the dialog renders into (`g_iOpenDialogs[surface]`, 0..14).
    pub surface: u16,
    /// Words 1..4: screen rectangle (render offset/scale is applied by the engine).
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    /// Word 5: background image index in the GUI gfx file (`CGfxManager::GetGuiGfxInfo`).
    /// Must be a raw (flag 0x20) image of exactly `width x height`.
    pub background: u16,
    /// Word 7: bit 0 = palette entries 0/1 of the background drawn as colour 31 behind controls;
    /// bit 1 = no background bitmap (controls are cleared with a solid fill, and the dialog is
    /// fully opaque for hit testing). Other bits unknown.
    pub flags: u16,
    pub controls: Vec<Control>,
}

/// `SGuiControl` (36 bytes). Offsets are relative to the control.
#[derive(Clone, Copy, Debug)]
pub struct Control {
    /// +0..+6: rectangle relative to the dialog.
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
    /// +8 `mainTexture`: image index in the GUI gfx file. For image-bearing types the image must
    /// be exactly `width x height` (`DrawControl`: "Control size mismatch").
    pub image: u16,
    /// +10 `valueLink`: control id used by game code (`GetControlPtr(dialog, valueLink)`).
    pub link: u16,
    /// +12: unknown (non-zero only on a few type-1 controls).
    pub unk12: u16,
    /// +14 `buttonPressedTexture`: image drawn while `effects & 1` (pressed/checked).
    pub pressed_image: u16,
    /// +16, +18: unknown (non-zero only on a few type-1 controls).
    pub unk16: u16,
    pub unk18: u16,
    /// +20: tooltip title string id (`TextTable`), [`NONE`] if absent.
    pub tooltip: u16,
    /// +22: extended tooltip string id, [`NONE`] if absent.
    pub tooltip_ext: u16,
    /// +24 `controlType`. See [`Control::kind_name`].
    pub kind: u8,
    /// +25 `id`: runtime text-cache slot, 0xFF in the files.
    pub text_slot: u8,
    /// +26: font template (index into the engine's 19 fonts) for text controls.
    pub font: u8,
    /// +27 `effects` initial state: 1 pressed, 2 highlighted, 4 disabled/grey, 8 hidden image,
    /// 0x10/0x20 seen in data (meaning unknown), 0xC0 = edit cursor.
    pub effects: u8,
    /// +28: control group (radio group / slider track+knob pairs share it). Inferred.
    pub group: u8,
    /// +29, +30: always 0 in the shipped files.
    pub unk29: [u8; 2],
    /// +31 `unknownId`: back-reference to the owning dialog's slot.
    pub dialog: u8,
    /// +32: text layout/alignment code for text controls (0..7). Inferred.
    pub text_style: u8,
    /// +33..+35: +33/+34 always 0, +35 always 1 in the shipped files.
    pub unk33: [u8; 3],
}

impl Control {
    /// Inferred control-type names (from code use where noted in `docs/GUISET.md`, otherwise
    /// from how the type is used in the data).
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            0 => "button",
            1 => "toggle button",
            2 => "tab button",
            3 => "radio button",
            4 => "text",
            5 => "text field",
            6 => "icon",
            7 => "image",
            8 => "slider knob",
            9 => "owner-drawn area",
            10 => "bar",
            13 => "menu button",
            16 => "slider track",
            17 => "small button",
            19 => "list entry",
            20 => "edit field",
            21 => "multiline text",
            _ => "unknown",
        }
    }

    /// Whether `image` is a bitmap of the control's size. Types 4/5/9/20/21 draw no bitmap; for
    /// type 19 the image never matches the rectangle (its use is unknown).
    pub fn has_image(&self) -> bool {
        self.image != NONE && !matches!(self.kind, 4 | 5 | 9 | 19 | 20 | 21)
    }

    fn read(r: &mut Reader) -> Result<Control> {
        Ok(Control {
            x: r.u16()?,
            y: r.u16()?,
            width: r.u16()?,
            height: r.u16()?,
            image: r.u16()?,
            link: r.u16()?,
            unk12: r.u16()?,
            pressed_image: r.u16()?,
            unk16: r.u16()?,
            unk18: r.u16()?,
            tooltip: r.u16()?,
            tooltip_ext: r.u16()?,
            kind: r.u8()?,
            text_slot: r.u8()?,
            font: r.u8()?,
            effects: r.u8()?,
            group: r.u8()?,
            unk29: [r.u8()?, r.u8()?],
            dialog: r.u8()?,
            text_style: r.u8()?,
            unk33: [r.u8()?, r.u8()?, r.u8()?],
        })
    }
}

impl GuiSet {
    pub fn open(path: impl AsRef<Path>) -> Result<GuiSet> {
        Self::parse(&std::fs::read(crate::path::resolve(path))?)
    }

    /// Header: `u32 version, u32 dialog_count, u32 0, u32 file_size`, then a `u32` offset per slot
    /// up to the first dialog (100 slots in some files, `dialog_count` in others; 0 = empty).
    pub fn parse(data: &[u8]) -> Result<GuiSet> {
        let mut r = Reader::new(data, "GUISet");
        let version = r.u32()?;
        let count = r.u32()? as usize;
        r.skip(4)?;
        let size = r.u32()? as usize;
        if size != data.len() {
            bail!("GUISet: header size {size} != file size {}", data.len());
        }
        let first = r.u32()? as usize;
        if first < 20 || first > data.len() || !(first - 16).is_multiple_of(4) {
            bail!("GUISet: bad first dialog offset {first:#x}");
        }
        r.seek(16);
        let offsets = (0..(first - 16) / 4)
            .map(|_| r.u32())
            .collect::<Result<Vec<_>>>()?;
        let mut dialogs = Vec::with_capacity(offsets.len());
        for (slot, &off) in offsets.iter().enumerate() {
            if off == 0 {
                dialogs.push(None);
                continue;
            }
            let mut r = Reader::at(data, off as usize, "GUISet dialog");
            let surface = r.u16()?;
            let (x, y, width, height) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
            let background = r.u16()?;
            let n = r.u16()? as usize;
            let flags = r.u16()?;
            let controls = (0..n)
                .map(|_| Control::read(&mut r))
                .collect::<Result<Vec<_>>>()?;
            dialogs.push(Some(Dialog {
                slot,
                surface,
                x,
                y,
                width,
                height,
                background,
                flags,
                controls,
            }));
        }
        let found = dialogs.iter().flatten().count();
        if found != count {
            bail!("GUISet: header says {count} dialogs, offset table has {found}");
        }
        Ok(GuiSet { version, dialogs })
    }

    pub fn dialog(&self, slot: usize) -> Option<&Dialog> {
        self.dialogs.get(slot)?.as_ref()
    }
}

impl Dialog {
    /// `GetControlPtr` @0x2f9b870: first control with this `valueLink`.
    pub fn control(&self, link: u16) -> Option<&Control> {
        self.controls.iter().find(|c| c.link == link)
    }
    /// Size of this dialog in the file.
    pub fn byte_len(&self) -> usize {
        DIALOG_HEADER + CONTROL_SIZE * self.controls.len()
    }
}

/// Which `Gfx\N` file holds the images for a GUI set (`CGfxManager::GetGuiGfxInfo` reads the
/// file enabled as the GUI file, `EnableGfxFile(n, 9, ..)`). Code evidence (decompiled build):
/// main menu -> 0 (`CGameStateHandler.cpp:477`), AO start screens -> 28 (`CAddOn::EnsureExtraGUI`),
/// MD -> 18 (`CMissionCD`), MD2/XMD3 -> 40 (`CMissionCD2/3`), in-game -> 9.
/// The per-race split Romans 9 / Vikings 19 / Mayas 29 / Trojans 39 is inferred from the data:
/// with that file every background and every image control matches its rectangle exactly, with
/// any other GUI file dozens do not (the decompiled build only ever enables 9 in-game).
pub fn gfx_file_for(file_name: &str) -> Option<u32> {
    let n = file_name.to_ascii_lowercase();
    let n = n.strip_suffix(".dat").unwrap_or(&n);
    Some(match n.strip_prefix("guiset")? {
        "startscreens" => 0,
        "romans" => 9,
        "vikings" => 19,
        "mayas" => 29,
        "trojans" => 39,
        "aostartscreens" => 28,
        "mdstartscreens" => 18,
        "md2startscreens" | "xmd3startscreens" => 40,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> Vec<u8> {
        let mut d = Vec::new();
        for v in [1u32, 1, 0, 0, 20] {
            d.extend(v.to_le_bytes());
        }
        for v in [3u16, 10, 20, 100, 50, 7, 1, 0] {
            d.extend(v.to_le_bytes());
        }
        let mut c = vec![0u8; 36];
        c[0] = 5;
        c[10] = 42;
        c[24] = 2;
        c[25] = 0xFF;
        c[35] = 1;
        d.extend(c);
        let len = d.len() as u32;
        d[12..16].copy_from_slice(&len.to_le_bytes());
        d
    }

    #[test]
    fn parse_tiny() {
        let g = GuiSet::parse(&tiny()).unwrap();
        let dl = g.dialog(0).unwrap();
        assert_eq!(
            (dl.surface, dl.x, dl.width, dl.background, dl.controls.len()),
            (3, 10, 100, 7, 1)
        );
        let c = dl.control(42).unwrap();
        assert_eq!((c.x, c.kind, c.text_slot, c.unk33[2]), (5, 2, 0xFF, 1));
        assert_eq!(dl.byte_len(), 52);
    }

    #[test]
    fn rejects_bad() {
        let mut d = tiny();
        d[12] ^= 1;
        assert!(GuiSet::parse(&d).is_err());
        let d = tiny();
        assert!(GuiSet::parse(&d[..d.len() - 1]).is_err());
    }

    #[test]
    fn gfx_mapping() {
        assert_eq!(gfx_file_for("GUISetRomans.DAT"), Some(9));
        assert_eq!(gfx_file_for("GUISetMayas.DAT"), Some(29));
        assert_eq!(gfx_file_for("GUISetStartscreens.dat"), Some(0));
        assert_eq!(gfx_file_for("foo.dat"), None);
    }
}
