//! D51, the workshop's (vehicle hall / shipyard) order sidebar (info code 0x11, info object
//! 0x11ab2c4), as the shipped exe runs it (own RE; `tools/re/gui-2026-10-22/d51_emu.py` runs the
//! original functions in unicorn, `tests/d51_vectors.rs` checks this port against them).
//!
//! * Fill (`CWorkshopBuildingRole` 0x5597b0, through the provider 0x53d920 that the handler of UI
//!   event 0x1cb, 0x4ebca0, registers; [`crate::building_dialog::sidebar_fill`]): three records
//!   at +8 {type, order count, boards, ironbars}: the types 4, 5 (and a third, unwritten) for the
//!   vehicle hall (building type 23, .rdata 0x101a3d4), else 1, 2, 3 (0x101a3e8); the count is
//!   the role's +0x190 slot of the type (pairs 0x1165134), the costs the local race's
//!   `CVehicleInfoMgr` record +0 / +4.
//! * The procedure 0x469540: init (code 0) clears the selected type (0x15aa4fc); a first record of
//!   type 4 or 5 is the vehicle hall (0x15aa04b = 0: radio 0xff hidden, 0xfd / 0xfe get the war
//!   machine's and the cart's images and tooltips), else the shipyard (0x15aa04b = 1: three
//!   radios); the race's images and tooltips come from .rdata 0xffbed8 (stride 0x8c). Then
//!   `SelectControl(0xfd)` (its code 3 selects the first type), the update and the costs zeroed.
//!   Code 3: the radios select their type while pressed (0xfd: 4 / 1, 0xfe: 5 / 3, 0xff: 0 / 2),
//!   else 0; the buttons 0xf4..0xf9 send UI event 0x1d5 (type, 100 / 0 / 5 / −5 / 1 / −1) when a
//!   type is selected. 0x1d5's handler 0x4ea4e0 sends net 0x13b0 = 5040 `TakeOrder` (selected
//!   building << 16 | type, amount). Code 5 (the cursor on a radio) shows the costs of the
//!   radio's record (0x4690d0 with 0x15aa500 / 504 / 508); code 6 and code 5 elsewhere zero
//!   them.
//! * The update 0x4691f0 (0x488e00 case 0x11): per record its count under its radio ("0" hidden
//!   below 1, "E" from 100, else "%u") and the record's index into the radio's slot.
//!
//! Quirk kept: the vehicle-hall branch stores the war machine's record index in the second slot
//! (0x15aa504, the cart radio's) and never writes the first, so the cart radio shows the war
//! machine's costs and the war machine radio the record of whatever the first slot held (0 in a
//! fresh game).

use crate::guistate::GuiState;

/// The dialog slot.
pub const DIALOG: usize = 51;

/// The radios, count texts and cost texts.
const RADIOS: [u16; 3] = [0xfd, 0xfe, 0xff];
const COSTS: (u16, u16) = (0x102, 0x103);
/// The buttons 0xf4..0xf9: the amount of UI event 0x1d5.
const AMOUNTS: [i32; 6] = [100, 0, 5, -5, 1, -1];

/// .rdata 0xffbed8 + race · 0x8c, the dwords the init reads (indices 0..=23).
const RACE_TABLE: [[u16; 24]; 5] = [
    [614, 613, 2248, 2249, 603, 607, 604, 2236, 2237, 602, 610, 605, 2246, 2247, 606, 612, 608, 2232, 2233, 616, 609, 615, 2230, 2231],
    [968, 967, 2248, 2249, 960, 962, 961, 2236, 2237, 959, 965, 964, 2246, 2247, 969, 970, 963, 2244, 2245, 972, 971, 973, 2230, 2231],
    [982, 981, 2248, 2249, 974, 976, 975, 2236, 2237, 973, 979, 978, 2246, 2247, 983, 984, 977, 2238, 2239, 985, 987, 986, 2230, 2231],
    [0; 24],
    [1019, 1018, 2248, 2249, 1000, 1003, 1002, 2236, 2237, 1004, 1007, 1006, 2246, 2247, 1012, 1015, 1014, 298, 299, 1008, 1011, 1010, 2230, 2231],
];

/// What the procedure does to the GUI, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Call {
    Text(u16, String),
    Visible(u16, bool),
    /// (link, image, pressed image).
    Images(u16, u16, u16),
    /// `SetTooltips` 0x676c80 (link, tooltip, extended tooltip).
    Tooltips(u16, u16, u16),
    /// `SelectControl(51, link, 1)`: its code 3 has run already.
    Select(u16),
    /// UI event (id, w, l).
    Event(u16, u32, i32),
}

/// The procedure's statics.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkshopSide {
    /// 0x15aa4fc: the selected vehicle type (0 none).
    pub selected: i32,
    /// 0x15aa04b: the shipyard layout.
    pub shipyard: bool,
    /// 0x15aa500 / 504 / 508: the record shown under each radio.
    pub shown: [i32; 3],
}

/// The count text of 0x4691f0 (as the smiths' 0x468c40).
fn count_text(v: i32) -> (String, bool) {
    if v < 1 {
        ("0".into(), false)
    } else if v > 99 {
        ("E".into(), true)
    } else {
        (v.to_string(), true)
    }
}

impl WorkshopSide {
    /// Code 0. `info` is the info object (records at +8), `race` the local player's.
    pub fn init(&mut self, info: &[u8], race: u8, out: &mut Vec<Call>) {
        self.selected = 0;
        let t = &RACE_TABLE[(race as usize).min(4)];
        let first = info.get(8).copied().unwrap_or(0);
        if first == 4 || first == 5 {
            self.shipyard = false;
            out.push(Call::Visible(0xff, false));
            out.push(Call::Images(0xfd, t[15], t[16]));
            out.push(Call::Tooltips(0xfd, t[17], t[18]));
            out.push(Call::Images(0xfe, t[20], t[21]));
            out.push(Call::Tooltips(0xfe, t[22], t[23]));
        } else {
            self.shipyard = true;
            out.push(Call::Visible(0xff, true));
            out.push(Call::Images(0xfd, t[0], t[1]));
            out.push(Call::Tooltips(0xfd, t[2], t[3]));
            out.push(Call::Images(0xfe, t[10], t[11]));
            out.push(Call::Tooltips(0xfe, t[12], t[13]));
            out.push(Call::Images(0xff, t[5], t[6]));
            out.push(Call::Tooltips(0xff, t[7], t[8]));
        }
        // `OpenDialog` released the radios: the select's code 3 arrives pressed.
        out.push(Call::Select(0xfd));
        self.command(0xfd, true, out);
        self.update(info, out);
        self.costs(info, None, out);
    }

    /// 0x4691f0.
    pub fn update(&mut self, info: &[u8], out: &mut Vec<Call>) {
        let rec = |i: usize| (info.get(8 + 4 * i).copied().unwrap_or(0), info.get(9 + 4 * i).copied().unwrap_or(0) as i8 as i32);
        if !self.shipyard {
            self.shown[1] = 0;
        }
        for i in 0..3 {
            let (kind, count) = rec(i);
            let slot = match (self.shipyard, kind) {
                (false, 4) => Some((0xf3, 1)),
                (false, 5) => Some((0xfb, 2)),
                (false, _) => {
                    out.push(Call::Text(0xfc, "0".into()));
                    out.push(Call::Visible(0xfc, false));
                    None
                }
                (true, 1) => Some((0xf3, 0)),
                (true, 2) => Some((0xfc, 2)),
                (true, 3) => Some((0xfb, 1)),
                (true, _) => None,
            };
            if let Some((link, s)) = slot {
                let (t, v) = count_text(count);
                out.push(Call::Text(link, t));
                out.push(Call::Visible(link, v));
                self.shown[s] = i as i32;
            }
        }
    }

    /// 0x4690d0: the costs of the record under radio `radio` (`None`: zeros).
    fn costs(&self, info: &[u8], radio: Option<usize>, out: &mut Vec<Call>) {
        let (boards, iron) = match radio {
            None => (0, 0),
            Some(r) => {
                let at = 8 + 4 * self.shown[r] as usize;
                let b = |o: usize| info.get(at + o).copied().unwrap_or(0) as i8 as i32;
                (b(2), b(3))
            }
        };
        // "%u" of the sign-extended byte.
        out.push(Call::Text(COSTS.0, (boards as u32).to_string()));
        out.push(Call::Text(COSTS.1, (iron as u32).to_string()));
    }

    /// Code 3 on `link`, `pressed` = the control's pressed bit (the high word's bit 0).
    pub fn command(&mut self, link: u16, pressed: bool, out: &mut Vec<Call>) {
        match link {
            0xf4..=0xf9 => {
                if self.selected != 0 {
                    out.push(Call::Event(0x1d5, self.selected as u32, AMOUNTS[(link - 0xf4) as usize]));
                }
            }
            0xfd => self.selected = if !pressed { 0 } else if self.shipyard { 1 } else { 4 },
            0xfe => self.selected = if !pressed { 0 } else if self.shipyard { 3 } else { 5 },
            0xff => self.selected = if pressed && self.shipyard { 2 } else { 0 },
            _ => {}
        }
    }

    /// Codes 5 (`on`) and 6: the cursor on / off a control.
    pub fn highlight(&mut self, info: &[u8], link: u16, on: bool, out: &mut Vec<Call>) {
        let radio = RADIOS.iter().position(|&r| r == link).filter(|_| on);
        self.costs(info, radio, out);
    }

    /// The calls applied to the GUI (the procedure has heard the selects' code 3 already).
    pub fn apply(st: &mut GuiState, calls: &[Call]) {
        for c in calls {
            match c {
                Call::Text(link, t) => {
                    st.texts.insert((DIALOG, *link), t.clone());
                }
                Call::Visible(link, on) => st.set_hidden(DIALOG, *link, !*on),
                Call::Images(link, i, p) => {
                    st.images.insert((DIALOG, *link), (*i, *p));
                }
                Call::Tooltips(link, a, b) => {
                    st.tooltips.insert((DIALOG, *link), (*a, *b));
                }
                Call::Select(link) => {
                    st.release_group(DIALOG, *link);
                    st.press(DIALOG, *link);
                }
                Call::Event(..) => {}
            }
        }
    }
}
