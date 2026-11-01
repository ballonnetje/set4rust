//! Commands: typed `s4_sim::Command`s stamped with the tick they execute at.
//!
//! The original sends player input through the network engine, which delivers it "for a later
//! valid tick"; every peer runs it at the start of that tick (`CInputProcessor::Process`, see
//! docs/SIM.md §2). Here every command carries its execution tick from the moment it is issued, so
//! the same log replays identically, and s4-net's lockstep runs the same commands over the network.
//! The tick is the one the original's single-player path gives it, see [`execution_tick`].

use s4_sim::{Command, PlayerCommand};
use s4_viewer::guistate::UiEvent;

// The command log type and the tick arithmetic live in s4-net (shared with the lockstep).
pub use s4_net::commands::{CommandQueue, Stamped, execution_tick};

/// The game command a GUI event stands for, if any.
///
/// Tabs and pages only change the GUI; building placement, working areas, priorities etc. are
/// sent by their own handlers. Here:
/// * `SendTo` (the right click, `CInputProcessor` 0x4ec960 for GUI event 0x1e0): net command
///   0x1396 = 5014 with `w` = the node (`y << 16 | x`), `l` = the mode and the selected entity
///   ids as its data (the release: the first 100; ours: up to [`s4_viewer::selection::MAX`],
///   1000, the user's request).
///   Mode bit 2 is Alt: the forced order. Mode: `((k >> 1 & 4 | k & 0x10) >> 1 | k & 4) >> 1` of the mouse key
///   state `k`, i.e. bit 0 Ctrl (`MK_CONTROL`), bit 1 Shift (`MK_SHIFT`), bit 2 key bit 0x10; 8
///   for a negative node. The handler clears a selection that starts with a building or a flag
///   0x4000 entity instead (select.rs) and plays sound 10.
pub fn ui_command(ev: &UiEvent) -> Option<Command> {
    match ev {
        UiEvent::SendTo { ids, x, y, mode } => {
            let entities: Vec<u16> = ids.iter().copied().take(s4_viewer::selection::MAX).collect();
            (!entities.is_empty()).then_some(Command::SendTo { x: *x, y: *y, mode: *mode as u32, entities })
        }
        _ => None,
    }
}

/// A command log: one command per line, `tick player Name args…`; `#` starts a comment.
/// Names: `SetWorkingArea building x y`, `CrushBuilding building`, `SwitchBuilding building`,
/// `PrioritizeBuildingSite building`, `Build kind x y`, `SendTo x y mode id…`.
pub fn parse_log(text: &str) -> anyhow::Result<Vec<Stamped>> {
    let mut out = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let w: Vec<&str> = line.split_whitespace().collect();
        let num = |i: usize| -> anyhow::Result<u32> {
            w.get(i).ok_or_else(|| anyhow::anyhow!("line {}: missing argument {i}", ln + 1))?.parse::<u32>().map_err(|e| anyhow::anyhow!("line {}: {e}", ln + 1))
        };
        let command = match *w.get(2).ok_or_else(|| anyhow::anyhow!("line {}: no command", ln + 1))? {
            "SetWorkingArea" => Command::SetWorkingArea { building: num(3)?, x: num(4)? as u16, y: num(5)? as u16 },
            "CrushBuilding" => Command::CrushBuilding { building: num(3)? },
            "SwitchBuilding" => Command::SwitchBuilding { building: num(3)? },
            "PrioritizeBuildingSite" => Command::PrioritizeBuildingSite { building: num(3)? },
            "Build" => Command::Build { building: num(3)?, x: num(4)? as u16, y: num(5)? as u16 },
            "SendTo" => Command::SendTo { x: num(3)? as u16, y: num(4)? as u16, mode: num(5)?, entities: (6..w.len()).map(|i| num(i).map(|v| v as u16)).collect::<anyhow::Result<_>>()? },
            other => anyhow::bail!("line {}: unknown command {other}", ln + 1),
        };
        out.push(Stamped { tick: num(0)?, command: PlayerCommand { player: num(1)? as u8, command } });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_parses() {
        let l = parse_log("# comment\n10 1 SetWorkingArea 7 100 120\n20 2 SwitchBuilding 9\n").unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].tick, 10);
        assert_eq!(l[1].command.command, Command::SwitchBuilding { building: 9 });
    }

    #[test]
    fn send_to_is_5014_with_at_most_1000_ids() {
        let ids: Vec<u16> = (1..=1200).collect();
        let c = ui_command(&UiEvent::SendTo { ids, x: 172, y: 76, mode: 3 }).unwrap();
        let Command::SendTo { x, y, mode, entities } = &c else { panic!("{c:?}") };
        assert_eq!((*x, *y, *mode, entities.len(), entities[999]), (172, 76, 3, 1000, 1000));
        assert_eq!(ui_command(&UiEvent::SendTo { ids: vec![], x: 1, y: 1, mode: 0 }), None);
    }
}
