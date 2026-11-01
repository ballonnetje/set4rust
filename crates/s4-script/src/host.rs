//! The host side of the original Lua API: one method per distinct C function the game
//! exports to scripts (aliases share a method). **Generated** by `tools/gen.py` from the export tables
//! in the shipped `S4_Main.exe` and `docs/SCRIPT_API.md`; edit the generator, not this file.
//!
//! Argument conversion follows Lua 3.2 `lua_getnumber`/`lua_getstring` as the original binding
//! code does: missing / nil / non-numeric arguments read as 0 (strings as ""), numbers are
//! truncated toward zero. Exceptions where the exe reads an argument only when enough were
//! passed (found by scanning every export for a second argument-count guard): the
//! `ExistsBuildingInArea` status defaults to 1, arg 4 of `SetConfigVar` is optional.
//! Every method has a default that reports the call through
//! [`ScriptHost::unhandled`] and returns 0, so a host only implements what it supports.

use crate::Arg;

#[allow(unused_variables, clippy::too_many_arguments)]
pub trait ScriptHost {
    /// Called by every default method implementation (and for nothing else).
    fn unhandled(&mut self, func: &'static str, args: &[Arg]) {}
    /// Script trace output (`_ERRORMESSAGE`, `print`, `write`, dbg helpers that only trace).
    fn trace(&mut self, msg: &str) {}
    /// `CGameData::IsGameWon` — the victory check is skipped once the game is decided.
    fn is_game_won(&mut self) -> bool { false }
    /// The game-type victory hook (`m_pVictoryConditionHook`, e.g. economy mode). Return true if
    /// one is installed and ran; it then replaces both the script and the default check.
    fn victory_hook(&mut self) -> bool { false }
    /// `ScriptDefaultVictoryConditionCheck` (= `GameDefaultPlayersLostCheck` + end announcement),
    /// run when no hook is installed and no script requested `Events.VICTORY_CONDITION_CHECK`.
    fn default_victory_check(&mut self) {}

    /// `Buildings.AddBuilding` — Places a building; returns its id. Same callee as AddBuildingEx with a different flag.
    /// Release exe 0x5c4ae0 (PDB name `sub_1C4AE0`); params read: [1, 2, 3, 4].
    fn buildings_add_building(&mut self, x: i32, y: i32, player: i32, building: i32) -> i32 {
        self.unhandled("Buildings.AddBuilding", &[Arg::Int(x), Arg::Int(y), Arg::Int(player), Arg::Int(building)]);
        0
    }
    /// `Buildings.AddBuildingEx` — Like AddBuilding (same callee, other flag); campaign setups use it for finished buildings (inferred).
    /// Release exe 0x5c4d70 (PDB name `sub_1C4D70`); params read: [1, 2, 3, 4].
    fn buildings_add_building_ex(&mut self, x: i32, y: i32, player: i32, building: i32) -> i32 {
        self.unhandled("Buildings.AddBuildingEx", &[Arg::Int(x), Arg::Int(y), Arg::Int(player), Arg::Int(building)]);
        0
    }
    /// `Buildings.Amount` — Number of buildings of a type in a status (Buildings.READY / UNDERCONSTRUCTION / ALL).
    /// Release exe 0x5c4fa0 (PDB name `sub_1C4FA0`); params read: [1, 2, 3].
    fn buildings_amount(&mut self, player: i32, building: i32, status: i32) -> i32 {
        self.unhandled("Buildings.Amount", &[Arg::Int(player), Arg::Int(building), Arg::Int(status)]);
        0
    }
    /// `Buildings.CrushBuilding` — Destroys the building.
    /// Release exe 0x5c4cf0 (PDB name `sub_1C4CF0`); params read: [1].
    fn buildings_crush_building(&mut self, building_id: i32) {
        self.unhandled("Buildings.CrushBuilding", &[Arg::Int(building_id)]);
    }
    /// `Buildings.ExistsBuildingInArea`, `Buildings.ExistsNearPt` — Count of matching buildings in the circle (scripts test == 0 or >= 1). 14 calls pass only 5 args: the status then defaults to 1 (READY); the exe reads arg 6 only when at least 6 were passed (0x5c530c). An explicit nil still reads as 0.
    /// Release exe 0x5c5140 (PDB name `sub_1C5140`); params read: [1, 2, 3, 4, 5, 6].
    fn buildings_exists_building_in_area(&mut self, player: i32, building: i32, x: i32, y: i32, radius: i32, status: i32) -> i32 {
        self.unhandled("Buildings.ExistsBuildingInArea", &[Arg::Int(player), Arg::Int(building), Arg::Int(x), Arg::Int(y), Arg::Int(radius), Arg::Int(status)]);
        0
    }
    /// `Buildings.IsWorkingArea`, `Buildings.WaNearPt` — Whether a building of that type has its work area at (x,y) (scripts test > 0).
    /// Release exe 0x5c5530 (PDB name `sub_1C5530`); params read: [1, 2, 3, 4].
    fn buildings_is_working_area(&mut self, player: i32, building: i32, x: i32, y: i32) -> i32 {
        self.unhandled("Buildings.IsWorkingArea", &[Arg::Int(player), Arg::Int(building), Arg::Int(x), Arg::Int(y)]);
        0
    }
    /// `Buildings.IsSelected` — 0/1: the local player has a building of this type selected.
    /// Release exe 0x5c5820 (PDB name `sub_1C5820`); params read: [1].
    fn buildings_is_selected(&mut self, building: i32) -> i32 {
        self.unhandled("Buildings.IsSelected", &[Arg::Int(building)]);
        0
    }
    /// `Buildings.CheckBuildingInhabitants` — 0/1: buildings of that type are staffed.
    /// Release exe 0x5c5930 (PDB name `sub_1C5930`); params read: [1, 2].
    fn buildings_check_building_inhabitants(&mut self, player: i32, building: i32) -> i32 {
        self.unhandled("Buildings.CheckBuildingInhabitants", &[Arg::Int(player), Arg::Int(building)]);
        0
    }
    /// `Buildings.GetInhabitantAmount` — Number of inhabitants (soldiers in towers).
    /// Release exe 0x5c5a70 (PDB name `sub_1C5A70`); params read: [1, 2].
    fn buildings_get_inhabitant_amount(&mut self, player: i32, building: i32) -> i32 {
        self.unhandled("Buildings.GetInhabitantAmount", &[Arg::Int(player), Arg::Int(building)]);
        0
    }
    /// `Buildings.GetFirstBuilding` — Id of the first building of that type; 0 if none.
    /// Release exe 0x5c5be0 (PDB name `sub_1C5BE0`); params read: [1, 2].
    fn buildings_get_first_building(&mut self, player: i32, building: i32) -> i32 {
        self.unhandled("Buildings.GetFirstBuilding", &[Arg::Int(player), Arg::Int(building)]);
        0
    }
    /// `Buildings.GetTarget` — Target of a building (e.g. the marketplace destination), 0 if none.
    /// Release exe 0x5c5d00 (PDB name `sub_1C5D00`); params read: [1].
    fn buildings_get_target(&mut self, building_id: i32) -> i32 {
        self.unhandled("Buildings.GetTarget", &[Arg::Int(building_id)]);
        0
    }
    /// `Buildings.Delete` — Removes a building; scripts always pass mode 2.
    /// Release exe 0x5c5e10 (PDB name `sub_1C5E10`); params read: [1, 2].
    fn buildings_delete(&mut self, building_id: i32, mode: i32) {
        self.unhandled("Buildings.Delete", &[Arg::Int(building_id), Arg::Int(mode)]);
    }
    /// `dbg.aioff`, `dbg.deactivateai`, `Debug.DeactivateAI` — PDB name LUA:DeactivateAI.
    /// Release exe 0x5c4770 (PDB name `LUA:DeactivateAI`); params read: [1].
    fn debug_deactivate_ai(&mut self, player: i32) {
        self.unhandled("Debug.DeactivateAI", &[Arg::Int(player)]);
    }
    /// `dbg.aion`, `dbg.activateai`, `Debug.ActivateAI` — PDB name LUA:ActivateAI (also dbg.aion / dbg.activateai).
    /// Release exe 0x5c3b90 (PDB name `LUA:ActivateAI`); params read: [1].
    fn debug_activate_ai(&mut self, player: i32) {
        self.unhandled("Debug.ActivateAI", &[Arg::Int(player)]);
    }
    /// `dbg.af`, `Debug.AssertFailed` — Empty function (nullsub) in release.
    /// Release exe 0x5c48e0 (PDB name `nullsub_641`); params read: none.
    fn debug_assert_failed(&mut self) {
        // A bare `ret` in the release: nothing to do.
    }
    /// `dbg.amk`, `DarkTribe.AddManakopter` — 
    /// Release exe 0x5cb370 (PDB name `sub_1CB370`); params read: [1, 2, 3].
    fn dark_tribe_add_manakopter(&mut self, x: i32, y: i32, player: i32) {
        self.unhandled("DarkTribe.AddManakopter", &[Arg::Int(x), Arg::Int(y), Arg::Int(player)]);
    }
    /// `dbg.av`, `Debug.AccessViolation` — Deliberate crash (developer).
    /// Release exe 0x5c4890 (PDB name `sub_1C4890`); params read: none.
    fn debug_access_violation(&mut self) {
        self.unhandled("Debug.AccessViolation", &[]);
    }
    /// `dbg.cg` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c4880 (PDB name `sub_1C4880`); params read: none.
    fn dbg_cg(&mut self) {
        self.unhandled("dbg.cg", &[]);
    }
    /// `dbg.dawc` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2db0 (PDB name `sub_1C2DB0`); params read: [1].
    fn dbg_dawc(&mut self, a1: &str) {
        self.unhandled("dbg.dawc", &[Arg::Str(a1.to_string())]);
    }
    /// `dbg.db` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2990 (PDB name `sub_1C2990`); params read: [1, 2, 3].
    fn dbg_db(&mut self, x: i32, y: i32, player: i32) {
        self.unhandled("dbg.db", &[Arg::Int(x), Arg::Int(y), Arg::Int(player)]);
    }
    /// `dbg.de` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3320 (PDB name `sub_1C3320`); params read: none.
    fn dbg_de(&mut self) {
        self.unhandled("dbg.de", &[]);
    }
    /// `dbg.dt`, `Debug.DumpTable` — 
    /// Release exe 0x5c3330 (PDB name `sub_1C3330`); params read: [1].
    fn debug_dump_table(&mut self, table_name: &str) {
        self.unhandled("Debug.DumpTable", &[Arg::Str(table_name.to_string())]);
    }
    /// `dbg.fe`, `Debug.FatalError` — Raises "SCRIPT: Debug.FatalError() called."
    /// Release exe 0x5c4920 (PDB name `sub_1C4920`); params read: none.
    fn debug_fatal_error(&mut self) {
        self.unhandled("Debug.FatalError", &[]);
    }
    /// `dbg.ods`, `Debug.OutputDebugString` — 
    /// Release exe 0x5c33b0 (PDB name `sub_1C33B0`); params read: [1].
    fn debug_output_debug_string(&mut self, text: &str) {
        self.unhandled("Debug.OutputDebugString", &[Arg::Str(text.to_string())]);
    }
    /// `dbg.ol` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2df0 (PDB name `sub_1C2DF0`); params read: [1, 2].
    fn dbg_ol(&mut self, a1: i32, a2: i32) {
        self.unhandled("dbg.ol", &[Arg::Int(a1), Arg::Int(a2)]);
    }
    /// `dbg.oom`, `Debug.OutOfMemory` — Deliberate out-of-memory (developer).
    /// Release exe 0x5c4900 (PDB name `sub_1C4900`); params read: none.
    fn debug_out_of_memory(&mut self) {
        self.unhandled("Debug.OutOfMemory", &[]);
    }
    /// `dbg.pcb` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2b20 (PDB name `sub_1C2B20`); params read: [1].
    fn dbg_pcb(&mut self, a1: i32) {
        self.unhandled("dbg.pcb", &[Arg::Int(a1)]);
    }
    /// `dbg.pfs` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3b10 (PDB name `sub_1C3B10`); params read: [1].
    fn dbg_pfs(&mut self, a1: i32) {
        self.unhandled("dbg.pfs", &[Arg::Int(a1)]);
    }
    /// `dbg.pi`, `Debug.PrintInt` — Empty function (nullsub) in release.
    /// Release exe 0x5c4ad0 (PDB name `nullsub_642`); params read: none.
    fn debug_print_int(&mut self) {
        // A bare `ret` in the release: nothing to do.
    }
    /// `dbg.pmm` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2c70 (PDB name `sub_1C2C70`); params read: none.
    fn dbg_pmm(&mut self) {
        self.unhandled("dbg.pmm", &[]);
    }
    /// `dbg.ptm` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2b00 (PDB name `sub_1C2B00`); params read: none.
    fn dbg_ptm(&mut self) {
        self.unhandled("dbg.ptm", &[]);
    }
    /// `dbg.psu` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c47f0 (PDB name `sub_1C47F0`); params read: [1].
    fn dbg_psu(&mut self, a1: i32) {
        self.unhandled("dbg.psu", &[Arg::Int(a1)]);
    }
    /// `dbg.rbi` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c2970 (PDB name `sub_1C2970`); params read: none.
    fn dbg_rbi(&mut self) {
        self.unhandled("dbg.rbi", &[]);
    }
    /// `dbg.sg` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3540 (PDB name `sub_1C3540`); params read: [1, 2, 3, 4].
    fn dbg_sg(&mut self, a1: i32, a2: i32, a3: i32, a4: i32) {
        self.unhandled("dbg.sg", &[Arg::Int(a1), Arg::Int(a2), Arg::Int(a3), Arg::Int(a4)]);
    }
    /// `dbg.scv`, `Game.SetConfigVar` — Writes a config variable (args 1-2 are read as strings, 3-4 as numbers). Arg 4 is read only when at least 4 were passed (0x5c76d6; a flag records whether it was), so a missing arg 4 is None. No shipped script uses it.
    /// Release exe 0x5c75f0 (PDB name `LUA:SetConfigVar`); params read: [3, 4].
    fn game_set_config_var(&mut self, section: &str, key: &str, value: i32, a4: Option<i32>) {
        self.unhandled("Game.SetConfigVar", &[Arg::Str(section.to_string()), Arg::Str(key.to_string()), Arg::Int(value), Arg::Int(a4.unwrap_or(0))]);
    }
    /// `dbg.sdl`, `Game.GetNumberOfSquaresWithDarkLand` — Count of map squares that are still dark land. Meaning of the argument (scripts pass 5..15) unknown.
    /// Release exe 0x5c6870 (PDB name `sub_1C6870`); params read: [1].
    fn game_get_number_of_squares_with_dark_land(&mut self, a1: i32) -> i32 {
        self.unhandled("Game.GetNumberOfSquaresWithDarkLand", &[Arg::Int(a1)]);
        0
    }
    /// `dbg.stm`, `Debug.ShowTextMessage` — Shows a raw (untranslated) message.
    /// Release exe 0x5c4940 (PDB name `sub_1C4940`); params read: [1].
    fn debug_show_text_message(&mut self, text: &str) {
        self.unhandled("Debug.ShowTextMessage", &[Arg::Str(text.to_string())]);
    }
    /// `dbg.tpd` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c49d0 (PDB name `nullsub_643`); params read: none.
    fn dbg_tpd(&mut self) {
        // A bare `ret` in the release: nothing to do.
    }
    /// `dbg.tst` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3040 (PDB name `sub_1C3040`); params read: [1, 2].
    fn dbg_tst(&mut self, a1: i32, a2: i32) {
        self.unhandled("dbg.tst", &[Arg::Int(a1), Arg::Int(a2)]);
    }
    /// `dbg.tp`, `Debug.TracePrint` — Empty function (nullsub) in release; scripts call it as dbg.tp(text).
    /// Release exe 0x5c49c0 (PDB name `nullsub_644`); params read: none.
    fn debug_trace_print(&mut self) {
        // A bare `ret` in the release: nothing to do.
    }
    /// `dbg.clearblocking` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3910 (PDB name `sub_1C3910`); params read: [1, 2].
    fn dbg_clearblocking(&mut self, x: i32, y: i32) {
        self.unhandled("dbg.clearblocking", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `dbg.setblocking` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3430 (PDB name `sub_1C3430`); params read: [1, 2].
    fn dbg_setblocking(&mut self, x: i32, y: i32) {
        self.unhandled("dbg.setblocking", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `dbg.setkey` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3a20 (PDB name `sub_1C3A20`); params read: [1, 2].
    fn dbg_setkey(&mut self, a1: &str, a2: &str) {
        self.unhandled("dbg.setkey", &[Arg::Str(a1.to_string()), Arg::Str(a2.to_string())]);
    }
    /// `dbg.setborderstones` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3c10 (PDB name `sub_1C3C10`); params read: [1, 2, 3, 4, 5].
    fn dbg_setborderstones(&mut self, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
        self.unhandled("dbg.setborderstones", &[Arg::Int(a1), Arg::Int(a2), Arg::Int(a3), Arg::Int(a4), Arg::Int(a5)]);
    }
    /// `dbg.checkblocking` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c4870 (PDB name `sub_1C4870`); params read: none.
    fn dbg_checkblocking(&mut self) {
        self.unhandled("dbg.checkblocking", &[]);
    }
    /// `dbg.setboundingcircle` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c3f50 (PDB name `sub_1C3F50`); params read: [1].
    fn dbg_setboundingcircle(&mut self, a1: i32) {
        self.unhandled("dbg.setboundingcircle", &[Arg::Int(a1)]);
    }
    /// `dbg.mcs` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c4040 (PDB name `sub_1C4040`); params read: [1, 2, 3, 4, 5].
    fn dbg_mcs(&mut self, a1: i32, a2: i32, a3: i32, a4: i32, a5: i32) {
        self.unhandled("dbg.mcs", &[Arg::Int(a1), Arg::Int(a2), Arg::Int(a3), Arg::Int(a4), Arg::Int(a5)]);
    }
    /// `dbg.ae` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c4280 (PDB name `sub_1C4280`); params read: [1, 2, 3].
    fn dbg_ae(&mut self, a1: i32, a2: i32, a3: i32) {
        self.unhandled("dbg.ae", &[Arg::Int(a1), Arg::Int(a2), Arg::Int(a3)]);
    }
    /// `dbg.aa` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c43f0 (PDB name `sub_1C43F0`); params read: [1, 2, 3, 4].
    fn dbg_aa(&mut self, a1: i32, a2: i32, a3: i32, a4: i32) {
        self.unhandled("dbg.aa", &[Arg::Int(a1), Arg::Int(a2), Arg::Int(a3), Arg::Int(a4)]);
    }
    /// `dbg.aan` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c45d0 (PDB name `sub_1C45D0`); params read: [1, 2, 3, 4].
    fn dbg_aan(&mut self, a1: i32, a2: i32, a3: i32, a4: i32) {
        self.unhandled("dbg.aan", &[Arg::Int(a1), Arg::Int(a2), Arg::Int(a3), Arg::Int(a4)]);
    }
    /// `dbg.scp` — Developer helper (dbg table); semantics unknown.
    /// Release exe 0x5c49e0 (PDB name `sub_1C49E0`); params read: [1, 2].
    fn dbg_scp(&mut self, a1: i32, a2: i32) {
        self.unhandled("dbg.scp", &[Arg::Int(a1), Arg::Int(a2)]);
    }
    /// `dbg.md`, `Debug.MakeDark` — 
    /// Release exe 0x5c3770 (PDB name `sub_1C3770`); params read: [1, 2].
    fn debug_make_dark(&mut self, x: i32, y: i32) {
        self.unhandled("Debug.MakeDark", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `dbg.mg`, `Debug.MakeGreen` — 
    /// Release exe 0x5c3840 (PDB name `sub_1C3840`); params read: [1, 2].
    fn debug_make_green(&mut self, x: i32, y: i32) {
        self.unhandled("Debug.MakeGreen", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `Debug.Error` — Empty function (nullsub) in release.
    /// Release exe 0x5c48f0 (PDB name `nullsub_645`); params read: none.
    fn debug_error(&mut self) {
        // A bare `ret` in the release: nothing to do.
    }
    /// `Debug.Freeze` — 
    /// Release exe 0x5c2920 (PDB name `sub_1C2920`); params read: none.
    fn debug_freeze(&mut self) {
        self.unhandled("Debug.Freeze", &[]);
    }
    /// `Debug.TerminateApplication` — 
    /// Release exe 0x5c3190 (PDB name `sub_1C3190`); params read: none.
    fn debug_terminate_application(&mut self) {
        self.unhandled("Debug.TerminateApplication", &[]);
    }
    /// `Debug.SendKey` — 
    /// Release exe 0x5c3220 (PDB name `sub_1C3220`); params read: [1].
    fn debug_send_key(&mut self, key: i32) {
        self.unhandled("Debug.SendKey", &[Arg::Int(key)]);
    }
    /// `Effects.AddEffect` — Spawns an effect animation with a sound (Sounds.NO_SOUND = 0). The last arg is always 0 in scripts.
    /// Release exe 0x5c5f00 (PDB name `sub_1C5F00`); params read: [1, 2, 3, 4, 5].
    fn effects_add_effect(&mut self, effect: i32, sound: i32, x: i32, y: i32, delay: i32) {
        self.unhandled("Effects.AddEffect", &[Arg::Int(effect), Arg::Int(sound), Arg::Int(x), Arg::Int(y), Arg::Int(delay)]);
    }
    /// `Game.ArePlayerAreasConnected` — For allied players only: whether a walkable path connects the two owned areas (A* with a player mask). Decompiled as GameArePlayerAreasConnected.
    /// Release exe 0x5c6150 (PDB name `sub_1C6150`); params read: [1, 2, 3, 4, 5, 6].
    fn game_are_player_areas_connected(&mut self, player_a: i32, x_a: i32, y_a: i32, player_b: i32, x_b: i32, y_b: i32) -> i32 {
        self.unhandled("Game.ArePlayerAreasConnected", &[Arg::Int(player_a), Arg::Int(x_a), Arg::Int(y_a), Arg::Int(player_b), Arg::Int(x_b), Arg::Int(y_b)]);
        0
    }
    /// `Game.DefaultGameEndCheck` — Default win/lose evaluation, with optional text keys for the end messages (scripts pass 0, 1 or 2 args).
    /// Release exe 0x5c8480 (PDB name `sub_1C8480`); params read: [1, 2].
    fn game_default_game_end_check(&mut self, win_text: &str, lose_text: &str) {
        self.unhandled("Game.DefaultGameEndCheck", &[Arg::Str(win_text.to_string()), Arg::Str(lose_text.to_string())]);
    }
    /// `Game.DefaultPlayerLostCheck` — Runs the default "has this player lost" check and announces the result.
    /// Release exe 0x5c7ff0 (PDB name `sub_1C7FF0`); params read: [1].
    fn game_default_player_lost_check(&mut self, player: i32) {
        self.unhandled("Game.DefaultPlayerLostCheck", &[Arg::Int(player)]);
    }
    /// `Game.DefaultPlayersLostCheck` — Default lost check for all players; ScriptDefaultVictoryConditionCheck calls the same.
    /// Release exe 0x5c8100 (PDB name `sub_1C8100`); params read: none.
    fn game_default_players_lost_check(&mut self) {
        self.unhandled("Game.DefaultPlayersLostCheck", &[]);
    }
    /// `Game.DisableLastNPlayersInStatistic` — Hides the last n players from the statistics screen (CStatistic::SetDontShowLastNPlayers).
    /// Release exe 0x5c81f0 (PDB name `sub_1C81F0`); params read: [1].
    fn game_disable_last_n_players_in_statistic(&mut self, n: i32) {
        self.unhandled("Game.DisableLastNPlayersInStatistic", &[Arg::Int(n)]);
    }
    /// `Game.EnableLandExploredCheck` — Enables "land explored" tracking in the fog of war (g_pFogging vtbl+52).
    /// Release exe 0x5c65e0 (PDB name `sub_1C65E0`); params read: [1].
    fn game_enable_land_explored_check(&mut self, enable: i32) {
        self.unhandled("Game.EnableLandExploredCheck", &[Arg::Int(enable)]);
    }
    /// `Game.EnemyPlayersLost` — Declares every enemy of player lost, so player wins.
    /// Release exe 0x5c7d90 (PDB name `sub_1C7D90`); params read: [1].
    fn game_enemy_players_lost(&mut self, player: i32) {
        self.unhandled("Game.EnemyPlayersLost", &[Arg::Int(player)]);
    }
    /// `Game.FindAnyUnit` — 1 if any unit of player is within radius.
    /// Release exe 0x5c6660 (PDB name `sub_1C6660`); params read: [1, 2, 3, 4].
    fn game_find_any_unit(&mut self, player: i32, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Game.FindAnyUnit", &[Arg::Int(player), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Game.GetAmountOfTreesInArea` — Number of trees in the circle.
    /// Release exe 0x5c6930 (PDB name `sub_1C6930`); params read: [1, 2, 3].
    fn game_get_amount_of_trees_in_area(&mut self, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Game.GetAmountOfTreesInArea", &[Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Game.GetDifficulty` — Campaign difficulty (scripts test 0, 1, 2).
    /// Release exe 0x5c6b50 (PDB name `sub_1C6B50`); params read: none.
    fn game_get_difficulty(&mut self) -> i32 {
        self.unhandled("Game.GetDifficulty", &[]);
        0
    }
    /// `Game.GetOffenceFightingStrength` — Offence fighting strength in percent.
    /// Release exe 0x5c6bb0 (PDB name `sub_1C6BB0`); params read: [1].
    fn game_get_offence_fighting_strength(&mut self, player: i32) -> i32 {
        self.unhandled("Game.GetOffenceFightingStrength", &[Arg::Int(player)]);
        0
    }
    /// `Game.HasPlayerLost` — 0/1.
    /// Release exe 0x5c7f10 (PDB name `sub_1C7F10`); params read: none.
    fn game_has_player_lost(&mut self, player: i32) -> i32 {
        self.unhandled("Game.HasPlayerLost", &[Arg::Int(player)]);
        0
    }
    /// `Game.IsAlmostAllLandExplored` — 0/1. Needs EnableLandExploredCheck(1) first.
    /// Release exe 0x5c6c90 (PDB name `sub_1C6C90`); params read: none.
    fn game_is_almost_all_land_explored(&mut self) -> i32 {
        self.unhandled("Game.IsAlmostAllLandExplored", &[]);
        0
    }
    /// `Game.IsAreaDarkLand` — 0/1.
    /// Release exe 0x5c6dc0 (PDB name `sub_1C6DC0`); params read: [1, 2, 3].
    fn game_is_area_dark_land(&mut self, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Game.IsAreaDarkLand", &[Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Game.IsAreaGreen` — 0/1.
    /// Release exe 0x5c6f60 (PDB name `sub_1C6F60`); params read: [1, 2, 3].
    fn game_is_area_green(&mut self, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Game.IsAreaGreen", &[Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Game.IsAreaOwned` — 0/1.
    /// Release exe 0x5c7100 (PDB name `sub_1C7100`); params read: [1, 2, 3, 4].
    fn game_is_area_owned(&mut self, player: i32, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Game.IsAreaOwned", &[Arg::Int(player), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Game.LocalPlayer` — Local player id (1-based).
    /// Release exe 0x5c7310 (PDB name `sub_1C7310`); params read: none.
    fn game_local_player(&mut self) -> i32 {
        self.unhandled("Game.LocalPlayer", &[]);
        0
    }
    /// `Game.NumberOfPlayers` — Number of players on the map.
    /// Release exe 0x5c7370 (PDB name `sub_1C7370`); params read: none.
    fn game_number_of_players(&mut self) -> i32 {
        self.unhandled("Game.NumberOfPlayers", &[]);
        0
    }
    /// `Game.PlayerLost` — Marks player as lost.
    /// Release exe 0x5c7c60 (PDB name `sub_1C7C60`); params read: [1].
    fn game_player_lost(&mut self, player: i32) {
        self.unhandled("Game.PlayerLost", &[Arg::Int(player)]);
    }
    /// `Game.Random` — Random integer below n (game RNG, so deterministic in multiplayer).
    /// Release exe 0x5c73d0 (PDB name `sub_1C73D0`); params read: [1].
    fn game_random(&mut self, n: i32) -> i32 {
        self.unhandled("Game.Random", &[Arg::Int(n)]);
        0
    }
    /// `Game.Random100` — Random integer 0..99.
    /// Release exe 0x5c74e0 (PDB name `sub_1C74E0`); params read: none.
    fn game_random100(&mut self) -> i32 {
        self.unhandled("Game.Random100", &[]);
        0
    }
    /// `Game.ResetFogging` — Re-darkens the fog of war (g_pFogging vtbl+48).
    /// Release exe 0x5c7560 (PDB name `sub_1C7560`); params read: none.
    fn game_reset_fogging(&mut self) {
        self.unhandled("Game.ResetFogging", &[]);
    }
    /// `Game.SetAlliesDontRevealFog` — Allies stop sharing vision (g_pFogging vtbl+44).
    /// Release exe 0x5c7570 (PDB name `sub_1C7570`); params read: [1].
    fn game_set_allies_dont_reveal_fog(&mut self, enable: i32) {
        self.unhandled("Game.SetAlliesDontRevealFog", &[Arg::Int(enable)]);
    }
    /// `Game.SetFightingStrength` — Sets a player fighting strength (AI handicap / campaign tuning).
    /// Release exe 0x5c7830 (PDB name `sub_1C7830`); params read: [1, 2].
    fn game_set_fighting_strength(&mut self, player: i32, percent: i32) {
        self.unhandled("Game.SetFightingStrength", &[Arg::Int(player), Arg::Int(percent)]);
    }
    /// `Game.ShowClock` — Shows the countdown clock.
    /// Release exe 0x5c7920 (PDB name `sub_1C7920`); params read: [1].
    fn game_show_clock(&mut self, seconds: i32) {
        self.unhandled("Game.ShowClock", &[Arg::Int(seconds)]);
    }
    /// `Game.ShowText` — Shows a message from s4_texts by key, with an optional map position (x,y) to jump to. 39 calls pass only the key.
    /// Release exe 0x5c7a30 (PDB name `sub_1C7A30`); params read: [1, 2, 3].
    fn game_show_text(&mut self, text_key: &str, x: i32, y: i32) {
        self.unhandled("Game.ShowText", &[Arg::Str(text_key.to_string()), Arg::Int(x), Arg::Int(y)]);
    }
    /// `Game.Time` — Elapsed game time in minutes.
    /// Release exe 0x5c7bc0 (PDB name `sub_1C7BC0`); params read: none.
    fn game_time(&mut self) -> i32 {
        self.unhandled("Game.Time", &[]);
        0
    }
    /// `Game.PlayerRace` — Race of player (Game.RACE_ROMAN=0 .. RACE_TROJAN=4).
    /// Release exe 0x5c6d00 (PDB name `sub_1C6D00`); params read: [1].
    fn game_player_race(&mut self, player: i32) -> i32 {
        self.unhandled("Game.PlayerRace", &[Arg::Int(player)]);
        0
    }
    /// `Goods.AddPileEx` — Drops a pile of goods.
    /// Release exe 0x5c8730 (PDB name `sub_1C8730`); params read: [1, 2, 3, 4].
    fn goods_add_pile_ex(&mut self, x: i32, y: i32, good: i32, amount: i32) {
        self.unhandled("Goods.AddPileEx", &[Arg::Int(x), Arg::Int(y), Arg::Int(good), Arg::Int(amount)]);
    }
    /// `Goods.Amount` — Total amount of a good the player owns.
    /// Release exe 0x5c88f0 (PDB name `sub_1C88F0`); params read: [1, 2].
    fn goods_amount(&mut self, player: i32, good: i32) -> i32 {
        self.unhandled("Goods.Amount", &[Arg::Int(player), Arg::Int(good)]);
        0
    }
    /// `Goods.GetAmountInArea` — Amount of a good lying in the circle.
    /// Release exe 0x5c8a30 (PDB name `sub_1C8A30`); params read: [1, 2, 3, 4, 5].
    fn goods_get_amount_in_area(&mut self, player: i32, good: i32, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Goods.GetAmountInArea", &[Arg::Int(player), Arg::Int(good), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Goods.Delete` — Removes piles of a good in the circle.
    /// Release exe 0x5c8d80 (PDB name `sub_1C8D80`); params read: [1, 2, 3, 4].
    fn goods_delete(&mut self, x: i32, y: i32, radius: i32, good: i32) {
        self.unhandled("Goods.Delete", &[Arg::Int(x), Arg::Int(y), Arg::Int(radius), Arg::Int(good)]);
    }
    /// `Magic.CastSpell` — Casts a spell at (x,y) (CMagic::CastSpell). Scripts pass e.g. (3,2,3,x,y). Meaning of a2 (race or caster?) is unconfirmed.
    /// Release exe 0x5c8ff0 (PDB name `sub_1C8FF0`); params read: [1, 2, 3, 4, 5].
    fn magic_cast_spell(&mut self, player: i32, a2: i32, spell: i32, x: i32, y: i32) -> i32 {
        self.unhandled("Magic.CastSpell", &[Arg::Int(player), Arg::Int(a2), Arg::Int(spell), Arg::Int(x), Arg::Int(y)]);
        0
    }
    /// `Magic.CurrentManaAmount` — Current mana.
    /// Release exe 0x5c9290 (PDB name `sub_1C9290`); params read: [1].
    fn magic_current_mana_amount(&mut self, player: i32) -> i32 {
        self.unhandled("Magic.CurrentManaAmount", &[Arg::Int(player)]);
        0
    }
    /// `Magic.DbgPrt` — Debug print. Reads one arg without number conversion.
    /// Release exe 0x5c9560 (PDB name `sub_1C9560`); params read: [1].
    fn magic_dbg_prt(&mut self, a1: &str) {
        self.unhandled("Magic.DbgPrt", &[Arg::Str(a1.to_string())]);
    }
    /// `Magic.IncreaseMana` — 
    /// Release exe 0x5c9370 (PDB name `sub_1C9370`); params read: [1, 2].
    fn magic_increase_mana(&mut self, player: i32, amount: i32) {
        self.unhandled("Magic.IncreaseMana", &[Arg::Int(player), Arg::Int(amount)]);
    }
    /// `Magic.DecreaseMana` — 
    /// Release exe 0x5c9460 (PDB name `sub_1C9460`); params read: [1, 2].
    fn magic_decrease_mana(&mut self, player: i32, amount: i32) {
        self.unhandled("Magic.DecreaseMana", &[Arg::Int(player), Arg::Int(amount)]);
    }
    /// `Map.AddDecoObject` — Places a map object (OBJECT_* id).
    /// Release exe 0x5c98c0 (PDB name `sub_1C98C0`); params read: [1, 2, 3].
    fn map_add_deco_object(&mut self, x: i32, y: i32, object: i32) {
        self.unhandled("Map.AddDecoObject", &[Arg::Int(x), Arg::Int(y), Arg::Int(object)]);
    }
    /// `Map.AddReef` — Places a reef/barrier object.
    /// Release exe 0x5c9c80 (PDB name `sub_1C9C80`); params read: [1, 2, 3].
    fn map_add_reef(&mut self, x: i32, y: i32, reef_type: i32) {
        self.unhandled("Map.AddReef", &[Arg::Int(x), Arg::Int(y), Arg::Int(reef_type)]);
    }
    /// `Map.DeleteReef` — 
    /// Release exe 0x5c9eb0 (PDB name `sub_1C9EB0`); params read: [1, 2].
    fn map_delete_reef(&mut self, x: i32, y: i32) {
        self.unhandled("Map.DeleteReef", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `Map.Height` — Map height in tiles.
    /// Release exe 0x5c9600 (PDB name `sub_1C9600`); params read: none.
    fn map_height(&mut self) -> i32 {
        self.unhandled("Map.Height", &[]);
        0
    }
    /// `Map.PointIsOnScreen` — 0/1: the tile is visible in the local viewport.
    /// Release exe 0x5c9660 (PDB name `sub_1C9660`); params read: [1, 2].
    fn map_point_is_on_screen(&mut self, x: i32, y: i32) -> i32 {
        self.unhandled("Map.PointIsOnScreen", &[Arg::Int(x), Arg::Int(y)]);
        0
    }
    /// `Map.Width` — Map width in tiles.
    /// Release exe 0x5c95a0 (PDB name `sub_1C95A0`); params read: none.
    fn map_width(&mut self) -> i32 {
        self.unhandled("Map.Width", &[]);
        0
    }
    /// `Map.SetScreenPos` — Scrolls the local view to (x,y).
    /// Release exe 0x5c9770 (PDB name `sub_1C9770`); params read: [1, 2].
    fn map_set_screen_pos(&mut self, x: i32, y: i32) {
        self.unhandled("Map.SetScreenPos", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `Map.DeleteDecoObject` — Removes map objects in the circle. 9 calls pass a 4th arg, which is ignored.
    /// Release exe 0x5c9ac0 (PDB name `sub_1C9AC0`); params read: [1, 2, 3].
    fn map_delete_deco_object(&mut self, x: i32, y: i32, radius: i32) {
        self.unhandled("Map.DeleteDecoObject", &[Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
    }
    /// `Sounds.PlaySound` — Plays a SIV_SOUNDS id (Sounds.* constants).
    /// Release exe 0x5c9fb0 (PDB name `LUA:PlaySound`); params read: [1].
    fn sounds_play_sound(&mut self, sound: i32) {
        self.unhandled("Sounds.PlaySound", &[Arg::Int(sound)]);
    }
    /// `Statistic.BuildingsCaptured` — 
    /// Release exe 0x5ca030 (PDB name `sub_1CA030`); params read: [1].
    fn statistic_buildings_captured(&mut self, player: i32) -> i32 {
        self.unhandled("Statistic.BuildingsCaptured", &[Arg::Int(player)]);
        0
    }
    /// `Statistic.GoodsProduced` — 
    /// Release exe 0x5ca0c0 (PDB name `sub_1CA0C0`); params read: [1, 2].
    fn statistic_goods_produced(&mut self, player: i32, good: i32) -> i32 {
        self.unhandled("Statistic.GoodsProduced", &[Arg::Int(player), Arg::Int(good)]);
        0
    }
    /// `Statistic.LandOwnedByPlayer` — 
    /// Release exe 0x5ca180 (PDB name `sub_1CA180`); params read: [1].
    fn statistic_land_owned_by_player(&mut self, player: i32) -> i32 {
        self.unhandled("Statistic.LandOwnedByPlayer", &[Arg::Int(player)]);
        0
    }
    /// `Statistic.ManaCollected` — 
    /// Release exe 0x5ca270 (PDB name `sub_1CA270`); params read: [1].
    fn statistic_mana_collected(&mut self, player: i32) -> i32 {
        self.unhandled("Statistic.ManaCollected", &[Arg::Int(player)]);
        0
    }
    /// `Statistic.MushroomFarmsDestroyed` — Decompiled as StatisticMushroomFarmsDestroyed (returns 0 and traces on an invalid player).
    /// Release exe 0x5ca350 (PDB name `sub_1CA350`); params read: [1].
    fn statistic_mushroom_farms_destroyed(&mut self, player: i32) -> i32 {
        self.unhandled("Statistic.MushroomFarmsDestroyed", &[Arg::Int(player)]);
        0
    }
    /// `Statistic.ServantsFreed` — Decompiled as StatisticServantsFreed.
    /// Release exe 0x5ca420 (PDB name `sub_1CA420`); params read: [1].
    fn statistic_servants_freed(&mut self, player: i32) -> i32 {
        self.unhandled("Statistic.ServantsFreed", &[Arg::Int(player)]);
        0
    }
    /// `Statistic.SpellsCast` — Takes a Magic.SPELL_* category.
    /// Release exe 0x5ca4f0 (PDB name `sub_1CA4F0`); params read: [1, 2].
    fn statistic_spells_cast(&mut self, player: i32, spell_category: i32) -> i32 {
        self.unhandled("Statistic.SpellsCast", &[Arg::Int(player), Arg::Int(spell_category)]);
        0
    }
    /// `Statistic.UnitsDestroyed` — 
    /// Release exe 0x5ca650 (PDB name `sub_1CA650`); params read: [1].
    fn statistic_units_destroyed(&mut self, player: i32) -> i32 {
        self.unhandled("Statistic.UnitsDestroyed", &[Arg::Int(player)]);
        0
    }
    /// `Tutorial.Exit` — Leaves the tutorial (posts an event).
    /// Release exe 0x5ca830 (PDB name `sub_1CA830`); params read: none.
    fn tutorial_exit(&mut self) {
        self.unhandled("Tutorial.Exit", &[]);
    }
    /// `Tutorial.Won` — Tutorial completed (posts an event).
    /// Release exe 0x5ca7a0 (PDB name `sub_1CA7A0`); params read: none.
    fn tutorial_won(&mut self) {
        self.unhandled("Tutorial.Won", &[]);
    }
    /// `Tutorial.GetCurrentZoomFactor` — CTutorial::CurrentZoomFactor.
    /// Release exe 0x5ca8c0 (PDB name `sub_1CA8C0`); params read: none.
    fn tutorial_get_current_zoom_factor(&mut self) -> i32 {
        self.unhandled("Tutorial.GetCurrentZoomFactor", &[]);
        0
    }
    /// `Tutorial.ClearMarker` — Removes the marker.
    /// Release exe 0x5caa80 (PDB name `sub_1CAA80`); params read: none.
    fn tutorial_clear_marker(&mut self) {
        self.unhandled("Tutorial.ClearMarker", &[]);
    }
    /// `Tutorial.DisableControls` — Disables the listed GUI controls (reads its args in a loop).
    /// Release exe 0x5caf90 (PDB name `sub_1CAF90`); params read: none.
    fn tutorial_disable_controls(&mut self, controls: &[i32]) {
        self.unhandled("Tutorial.DisableControls", &[Arg::Ints(controls.to_vec())]);
    }
    /// `Tutorial.DisableExcept` — Disables every GUI control except the listed Control.* ids (scripts pass 0..16 args).
    /// Release exe 0x5cade0 (PDB name `sub_1CADE0`); params read: none.
    fn tutorial_disable_except(&mut self, controls: &[i32]) {
        self.unhandled("Tutorial.DisableExcept", &[Arg::Ints(controls.to_vec())]);
    }
    /// `Tutorial.EnableControls` — Enables the listed GUI controls.
    /// Release exe 0x5caec0 (PDB name `sub_1CAEC0`); params read: none.
    fn tutorial_enable_controls(&mut self, controls: &[i32]) {
        self.unhandled("Tutorial.EnableControls", &[Arg::Ints(controls.to_vec())]);
    }
    /// `Tutorial.EnableAllControls` — 
    /// Release exe 0x5cada0 (PDB name `sub_1CADA0`); params read: none.
    fn tutorial_enable_all_controls(&mut self) {
        self.unhandled("Tutorial.EnableAllControls", &[]);
    }
    /// `Tutorial.PressButton` — Simulates a click (IGuiEngine::SelectControl) on Dialog.* / Control.*.
    /// Release exe 0x5cacc0 (PDB name `sub_1CACC0`); params read: [1, 2].
    fn tutorial_press_button(&mut self, dialog: i32, control: i32) {
        self.unhandled("Tutorial.PressButton", &[Arg::Int(dialog), Arg::Int(control)]);
    }
    /// `Tutorial.SetMarker` — Sets the blinking marker.
    /// Release exe 0x5ca920 (PDB name `sub_1CA920`); params read: [1, 2].
    fn tutorial_set_marker(&mut self, x: i32, y: i32) {
        self.unhandled("Tutorial.SetMarker", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `Tutorial.ShowText` — Shows tutorial text by key; mode is 0 or 1.
    /// Release exe 0x5cab10 (PDB name `sub_1CAB10`); params read: [1, 2].
    fn tutorial_show_text(&mut self, text_key: &str, mode: i32) {
        self.unhandled("Tutorial.ShowText", &[Arg::Str(text_key.to_string()), Arg::Int(mode)]);
    }
    /// `Tutorial.SelectNextBuilding` — Selects the next building of that type (posts a UI event).
    /// Release exe 0x5cb060 (PDB name `sub_1CB060`); params read: [1].
    fn tutorial_select_next_building(&mut self, building: i32) {
        self.unhandled("Tutorial.SelectNextBuilding", &[Arg::Int(building)]);
    }
    /// `Tutorial.RWM` — Unknown (name not expanded); one call, RWM(1).
    /// Release exe 0x5ca720 (PDB name `sub_1CA720`); params read: [1].
    fn tutorial_rwm(&mut self, a1: i32) {
        self.unhandled("Tutorial.RWM", &[Arg::Int(a1)]);
    }
    /// `Tutorial.SetZoom` — 
    /// Release exe 0x5cb140 (PDB name `sub_1CB140`); params read: [1].
    fn tutorial_set_zoom(&mut self, level: i32) {
        self.unhandled("Tutorial.SetZoom", &[Arg::Int(level)]);
    }
    /// `Tutorial.SetWorldCursor` — Shows the world cursor/arrow at (x,y).
    /// Release exe 0x5cb230 (PDB name `sub_1CB230`); params read: [1, 2].
    fn tutorial_set_world_cursor(&mut self, x: i32, y: i32) {
        self.unhandled("Tutorial.SetWorldCursor", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `Tutorial.DeleteWorldCursor` — 
    /// Release exe 0x5cb340 (PDB name `sub_1CB340`); params read: none.
    fn tutorial_delete_world_cursor(&mut self) {
        self.unhandled("Tutorial.DeleteWorldCursor", &[]);
    }
    /// `Vehicles.AddVehicle` — Spawns a vehicle and returns its id (used with AddWheelerToFerry / AddSettlersToFerry). Scripts pass 4, 5 or 7 args, e.g. (x,y,2,FERRY,1,0,1), (x,y,3,WARSHIP,5,90,1).
    /// Release exe 0x5c1280 (PDB name `sub_1C1280`); params read: [1, 2, 3, 4, 5, 6, 7].
    fn vehicles_add_vehicle(&mut self, x: i32, y: i32, player: i32, vehicle: i32, direction: i32, a6: i32, a7: i32) -> i32 {
        self.unhandled("Vehicles.AddVehicle", &[Arg::Int(x), Arg::Int(y), Arg::Int(player), Arg::Int(vehicle), Arg::Int(direction), Arg::Int(a6), Arg::Int(a7)]);
        0
    }
    /// `Vehicles.AddWheelerToFerry` — Loads a cart (Vehicles.FOUNDATION_CART) onto a ferry.
    /// Release exe 0x5c1600 (PDB name `sub_1C1600`); params read: [1, 2].
    fn vehicles_add_wheeler_to_ferry(&mut self, ferry_id: i32, vehicle: i32) {
        self.unhandled("Vehicles.AddWheelerToFerry", &[Arg::Int(ferry_id), Arg::Int(vehicle)]);
    }
    /// `Vehicles.Amount` — Number of vehicles of a type. 28 calls pass only 2 args.
    /// Release exe 0x5c1780 (PDB name `sub_1C1780`); params read: [1, 2, 3].
    fn vehicles_amount(&mut self, player: i32, vehicle: i32, a3: i32) -> i32 {
        self.unhandled("Vehicles.Amount", &[Arg::Int(player), Arg::Int(vehicle), Arg::Int(a3)]);
        0
    }
    /// `Vehicles.AmountInArea` — 
    /// Release exe 0x5c1930 (PDB name `sub_1C1930`); params read: [1, 2, 3, 4, 5].
    fn vehicles_amount_in_area(&mut self, player: i32, vehicle: i32, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Vehicles.AmountInArea", &[Arg::Int(player), Arg::Int(vehicle), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Vehicles.AmountInSector` — 
    /// Release exe 0x5c1c50 (PDB name `sub_1C1C50`); params read: [1, 2, 3].
    fn vehicles_amount_in_sector(&mut self, player: i32, vehicle: i32, sector: i32) -> i32 {
        self.unhandled("Vehicles.AmountInSector", &[Arg::Int(player), Arg::Int(vehicle), Arg::Int(sector)]);
        0
    }
    /// `Vehicles.GetFerryCargoInArea` — Returns 6 numbers describing ferry cargo in the circle (scripts name them mu,l1,l2,l3,sl,fc).
    /// Release exe 0x5c1e80 (PDB name `sub_1C1E80`); params read: [1, 2, 3, 4].
    fn vehicles_get_ferry_cargo_in_area(&mut self, player: i32, x: i32, y: i32, radius: i32) -> [i32; 6] {
        self.unhandled("Vehicles.GetFerryCargoInArea", &[Arg::Int(player), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        [0; 6]
    }
    /// `Vehicles.KillVehicles` — Destroys matching vehicles in the circle. 8 calls pass 5 args.
    /// Release exe 0x5c22a0 (PDB name `sub_1C22A0`); params read: [1, 2, 3, 4, 5, 6].
    fn vehicles_kill_vehicles(&mut self, player: i32, vehicle: i32, x: i32, y: i32, radius: i32, a6: i32) {
        self.unhandled("Vehicles.KillVehicles", &[Arg::Int(player), Arg::Int(vehicle), Arg::Int(x), Arg::Int(y), Arg::Int(radius), Arg::Int(a6)]);
    }
    /// `Vehicles.IsSelected` — 0/1: at least count vehicles of that type are selected.
    /// Release exe 0x5c2610 (PDB name `sub_1C2610`); params read: [1, 2].
    fn vehicles_is_selected(&mut self, vehicle: i32, count: i32) -> i32 {
        self.unhandled("Vehicles.IsSelected", &[Arg::Int(vehicle), Arg::Int(count)]);
        0
    }
    /// `Vehicles.GetHealth` — Health of the vehicle at (x,y).
    /// Release exe 0x5c27b0 (PDB name `sub_1C27B0`); params read: [1, 2].
    fn vehicles_get_health(&mut self, x: i32, y: i32) -> i32 {
        self.unhandled("Vehicles.GetHealth", &[Arg::Int(x), Arg::Int(y)]);
        0
    }
    /// `Settlers.AddSettlers` — Spawns settlers (CSettlerMgr::AddSettlers).
    /// Release exe 0x5bfd90 (PDB name `sub_1BFD90`); params read: [1, 2, 3, 4, 5].
    fn settlers_add_settlers(&mut self, x: i32, y: i32, player: i32, settler: i32, amount: i32) {
        self.unhandled("Settlers.AddSettlers", &[Arg::Int(x), Arg::Int(y), Arg::Int(player), Arg::Int(settler), Arg::Int(amount)]);
    }
    /// `Settlers.AddSettlersToFerry` — 
    /// Release exe 0x5bffc0 (PDB name `sub_1BFFC0`); params read: [1, 2, 3].
    fn settlers_add_settlers_to_ferry(&mut self, ferry_id: i32, settler: i32, amount: i32) {
        self.unhandled("Settlers.AddSettlersToFerry", &[Arg::Int(ferry_id), Arg::Int(settler), Arg::Int(amount)]);
    }
    /// `Settlers.Amount` — Number of settlers of that type.
    /// Release exe 0x5c05a0 (PDB name `sub_1C05A0`); params read: [1, 2].
    fn settlers_amount(&mut self, player: i32, settler: i32) -> i32 {
        self.unhandled("Settlers.Amount", &[Arg::Int(player), Arg::Int(settler)]);
        0
    }
    /// `Settlers.AmountInSector` — 
    /// Release exe 0x5c0700 (PDB name `sub_1C0700`); params read: [1, 2, 3].
    fn settlers_amount_in_sector(&mut self, player: i32, settler: i32, sector: i32) -> i32 {
        self.unhandled("Settlers.AmountInSector", &[Arg::Int(player), Arg::Int(settler), Arg::Int(sector)]);
        0
    }
    /// `Settlers.AmountInArea` — 
    /// Release exe 0x5c0cd0 (PDB name `sub_1C0CD0`); params read: [1, 2, 3, 4, 5].
    fn settlers_amount_in_area(&mut self, player: i32, settler: i32, x: i32, y: i32, radius: i32) -> i32 {
        self.unhandled("Settlers.AmountInArea", &[Arg::Int(player), Arg::Int(settler), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
        0
    }
    /// `Settlers.Kill` — Kills a settler. Argument meaning unconfirmed; no script uses it.
    /// Release exe 0x5c01a0 (PDB name `sub_1C01A0`); params read: [1].
    fn settlers_kill(&mut self, a1: i32) {
        self.unhandled("Settlers.Kill", &[Arg::Int(a1)]);
    }
    /// `Settlers.KillSelectableSettlers` — Kills selectable settlers in the circle; settler -1 means any; a6 is 0 or 1.
    /// Release exe 0x5c0220 (PDB name `sub_1C0220`); params read: [1, 2, 3, 4, 5, 6].
    fn settlers_kill_selectable_settlers(&mut self, player: i32, settler: i32, x: i32, y: i32, radius: i32, a6: i32) {
        self.unhandled("Settlers.KillSelectableSettlers", &[Arg::Int(player), Arg::Int(settler), Arg::Int(x), Arg::Int(y), Arg::Int(radius), Arg::Int(a6)]);
    }
    /// `Settlers.ProductionAmount` — No script uses it.
    /// Release exe 0x5c1000 (PDB name `sub_1C1000`); params read: [1].
    fn settlers_production_amount(&mut self, settler: i32) -> i32 {
        self.unhandled("Settlers.ProductionAmount", &[Arg::Int(settler)]);
        0
    }
    /// `Settlers.IsSelected` — 0/1: at least count settlers of that type are selected.
    /// Release exe 0x5c1100 (PDB name `sub_1C1100`); params read: [1, 2].
    fn settlers_is_selected(&mut self, settler: i32, count: i32) -> i32 {
        self.unhandled("Settlers.IsSelected", &[Arg::Int(settler), Arg::Int(count)]);
        0
    }
    /// `Settlers.SetHealthInArea` — Sets/heals health of matching settlers in the circle.
    /// Release exe 0x5c0900 (PDB name `sub_1C0900`); params read: [1, 2, 3, 4, 5, 6].
    fn settlers_set_health_in_area(&mut self, player: i32, settler: i32, x: i32, y: i32, radius: i32, health: i32) -> i32 {
        self.unhandled("Settlers.SetHealthInArea", &[Arg::Int(player), Arg::Int(settler), Arg::Int(x), Arg::Int(y), Arg::Int(radius), Arg::Int(health)]);
        0
    }
    /// `DarkTribe.FlyTo` — Sends the manakopter to (x,y). One call passes a 3rd arg.
    /// Release exe 0x5cb4a0 (PDB name `sub_1CB4A0`); params read: [1, 2].
    fn dark_tribe_fly_to(&mut self, x: i32, y: i32) {
        self.unhandled("DarkTribe.FlyTo", &[Arg::Int(x), Arg::Int(y)]);
    }
    /// `DarkTribe.CreateCrew` — No script uses it.
    /// Release exe 0x5cb5a0 (PDB name `sub_1CB5A0`); params read: [1, 2].
    fn dark_tribe_create_crew(&mut self, a1: i32, a2: i32) {
        self.unhandled("DarkTribe.CreateCrew", &[Arg::Int(a1), Arg::Int(a2)]);
    }
    /// `DarkTribe.LeaveMk` — No script uses it.
    /// Release exe 0x5cb7a0 (PDB name `sub_1CB7A0`); params read: none.
    fn dark_tribe_leave_mk(&mut self) {
        self.unhandled("DarkTribe.LeaveMk", &[]);
    }
    /// `AI.SetPlayerVar` — Sets one of the 23 AI player script variables (AttackMode, AttackDelay, SoldierLimitAbsolute, ...; name compared case-insensitively, see AI_PLAYER_VARS) for the three AI difficulty levels. The level args arrive as the binding reads them (sub at 0x43caa0): missing/non-number -1, negative -2, else the value. Values >= 0 are clamped to the variable range; normal = -1 copies easy, hard = -1 copies normal; anything still < 0 makes the call a no-op (so an explicit -1 from a script, read as -2, cancels it). player 0 sets the default for all 8 players, otherwise the player must be in use. Unknown names are ignored. Scripts pass 3 args twice and 4 args once.
    /// Release exe 0x43ced0 (PDB name `sub_3CED0`); params read: [1, 2].
    fn ai_set_player_var(&mut self, player: i32, var: &str, easy: i32, normal: i32, hard: i32) {
        self.unhandled("AI.SetPlayerVar", &[Arg::Int(player), Arg::Str(var.to_string()), Arg::Int(easy), Arg::Int(normal), Arg::Int(hard)]);
    }
    /// `AI.AttackNow` — If the player AI is active: g_pAI->PostAIEvent(0x1b, player, a2, a3). Scripts pass (player, 1, 1..3); meaning of a2/a3 (target player / strength?) not yet traced into the AI.
    /// Release exe 0x43d2d0 (PDB name `sub_3D2D0`); params read: [1, 2, 3].
    fn ai_attack_now(&mut self, player: i32, a2: i32, a3: i32) {
        self.unhandled("AI.AttackNow", &[Arg::Int(player), Arg::Int(a2), Arg::Int(a3)]);
    }
    /// `AI.DarkTribeUseShamans` — If the player AI is active: AI script flag 0x20000000 set (enable != 0) or cleared.
    /// Release exe 0x43d670 (PDB name `sub_3D670`); params read: [1, 2].
    fn ai_dark_tribe_use_shamans(&mut self, player: i32, enable: i32) {
        self.unhandled("AI.DarkTribeUseShamans", &[Arg::Int(player), Arg::Int(enable)]);
    }
    /// `AI.DarkTribeUseDarkGardeners` — If the player AI is active: AI script flag 0x10000000 set or cleared.
    /// Release exe 0x43d580 (PDB name `sub_3D580`); params read: [1, 2].
    fn ai_dark_tribe_use_dark_gardeners(&mut self, player: i32, enable: i32) {
        self.unhandled("AI.DarkTribeUseDarkGardeners", &[Arg::Int(player), Arg::Int(enable)]);
    }
    /// `AI.DarkTribeDbgIgnoreFighters` — If the player AI is active: AI script flag 0x40000000 set or cleared.
    /// Release exe 0x43d760 (PDB name `sub_3D760`); params read: [1, 2].
    fn ai_dark_tribe_dbg_ignore_fighters(&mut self, player: i32, enable: i32) {
        self.unhandled("AI.DarkTribeDbgIgnoreFighters", &[Arg::Int(player), Arg::Int(enable)]);
    }
    /// `AI.GuardRegion` — region 1..15, player 1..8 in use: sets bit 0 of the per-player region flags (player*16+region).
    /// Release exe 0x43d850 (PDB name `sub_3D850`); params read: [1, 2].
    fn ai_guard_region(&mut self, player: i32, region: i32) {
        self.unhandled("AI.GuardRegion", &[Arg::Int(player), Arg::Int(region)]);
    }
    /// `AI.DbgPrt` — g_pAI->DbgPrint(player) (developer).
    /// Release exe 0x43d250 (PDB name `sub_3D250`); params read: [1].
    fn ai_dbg_prt(&mut self, player: i32) {
        self.unhandled("AI.DbgPrt", &[Arg::Int(player)]);
    }
    /// `AI.CastGratisSpell` — If the player AI is active: casts spell (0..7) at (x,y) for free (the magic cast routine with free-flag 1).
    /// Release exe 0x43d0f0 (PDB name `sub_3D0F0`); params read: [1, 2, 3, 4].
    fn ai_cast_gratis_spell(&mut self, player: i32, spell: i32, x: i32, y: i32) {
        self.unhandled("AI.CastGratisSpell", &[Arg::Int(player), Arg::Int(spell), Arg::Int(x), Arg::Int(y)]);
    }
    /// `AI.SetFlagBits` — If the player AI is active: AI script flags |= bits.
    /// Release exe 0x43d3e0 (PDB name `sub_3D3E0`); params read: [1, 2].
    fn ai_set_flag_bits(&mut self, player: i32, bits: i32) {
        self.unhandled("AI.SetFlagBits", &[Arg::Int(player), Arg::Int(bits)]);
    }
    /// `AI.ClearFlagBits` — If the player AI is active: AI script flags &= ~bits.
    /// Release exe 0x43d4b0 (PDB name `sub_3D4B0`); params read: [1, 2].
    fn ai_clear_flag_bits(&mut self, player: i32, bits: i32) {
        self.unhandled("AI.ClearFlagBits", &[Arg::Int(player), Arg::Int(bits)]);
    }
    /// `AI.NewSquad` — player 1..8 in use, command one of AI.CMD_MOVE_HOME(7) / MOVE_AND_VANISH(8) / SUICIDE_MISSION(9) / MOVE_AND_STAY(13); for 8 and 13 (x,y) must be inside the map. Then g_pAI->PostAIEvent(0x1c, player, command, x | y<<16). Anything else (e.g. the nil AI.MOVE_HOME two scripts use) is silently ignored.
    /// Release exe 0x43cd40 (PDB name `sub_3CD40`); params read: [1, 2, 3, 4].
    fn ai_new_squad(&mut self, player: i32, command: i32, x: i32, y: i32) {
        self.unhandled("AI.NewSquad", &[Arg::Int(player), Arg::Int(command), Arg::Int(x), Arg::Int(y)]);
    }
    /// `AI.DbgTst` — Empty (AILuaGlobalFuncDebugTest).
    /// Release exe 0x43d920 (PDB name `nullsub_512`); params read: none.
    fn ai_dbg_tst(&mut self) {
        // A bare `ret` in the release: nothing to do.
    }
    /// `AI.DbgSaveCfgVars` — Writes the AI config variables to the config file (developer).
    /// Release exe 0x43d930 (PDB name `sub_3D930`); params read: none.
    fn ai_dbg_save_cfg_vars(&mut self) {
        self.unhandled("AI.DbgSaveCfgVars", &[]);
    }
    /// `AI.DefineRegion` — region 1..15, (x,y) inside the map, radius > 0: stores the circle as AI region `region` (and raises the highest-region counter).
    /// Release exe 0x43cbe0 (PDB name `sub_3CBE0`); params read: [1, 2, 3, 4].
    fn ai_define_region(&mut self, region: i32, x: i32, y: i32, radius: i32) {
        self.unhandled("AI.DefineRegion", &[Arg::Int(region), Arg::Int(x), Arg::Int(y), Arg::Int(radius)]);
    }
    /// `AI.DeactivateAllPlayerAIs` — g_pAI->DeactivateAllPlayerAIs().
    /// Release exe 0x43cd30 (PDB name `sub_3CD30`); params read: none.
    fn ai_deactivate_all_player_ais(&mut self) {
        self.unhandled("AI.DeactivateAllPlayerAIs", &[]);
    }
}
