//! What the simulation tells the outside world (audio, UI) during a tick. The sim never calls
//! a client system directly: it appends to [`crate::World::events`]; the client drains it after
//! each tick. Events are not simulation state (not hashed, not saved).

/// Game events for the map script that the simulation raises (`CGameScriptManager`'s
/// `SendEvent` from inside the logic: `GOODARRIVE` 17, `SETTLER_CHANGE_TYPE` 18). The original
/// calls the script at once, and only when the script requested the event (`requests > 0`); here
/// the script driver sets `wants` (bit per event) and receives the queue at its slot in the tick
/// (`ScriptDriver::update`): events after that slot reach it in the next tick. Not state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptEvents {
    /// Bit `e`: the script requested event `e`.
    pub wants: u32,
    pub queue: Vec<(u32, [i32; 3])>,
    /// Bit `p`: player `p` was declared lost this tick (event 0x38 with the flag 0x149b7f4 set,
    /// outside the script's own checks: [`crate::buildings::crush::destroy`]).
    pub lost: u32,
}

impl ScriptEvents {
    pub const GOODARRIVE: u32 = 17;
    pub const SETTLER_CHANGE_TYPE: u32 = 18;

    pub fn send(&mut self, event: u32, args: [i32; 3]) {
        if event < 32 && self.wants & (1 << event) != 0 {
            self.queue.push((event, args));
        }
    }
}

/// One notification from a tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimEvent {
    /// A job sound may play (`ISettlerRole::Update`: the settler's gfx job is on its sound's
    /// frame). The client rolls `chance` percent (the original uses the CRT `rand()`, 100 =
    /// always), checks the fog at (x, y) and plays `sound` (`SIV_SOUNDS`) there
    /// (`CSoundManager::PlayEnvironmentSound`).
    JobSound { settler: u16, x: i32, y: i32, sound: u16, chance: u8 },
    /// `CLogic::SetWarAction`: a warrior (type 2..5) animated this tick (drives fight music).
    WarAction,
    /// `CEffects::AddEffect(effect, sound, x, y, 0, 0, 0)` (vtable +0x10, release 0x4e3a80): a
    /// map effect starts this tick at node (x, y). `effect` is the effect gfx type (the record's
    /// byte 1), `sound` its `SIV_SOUNDS` id (byte 2, 0 = none). The original plays the sound
    /// while drawing (0x4e33b0: the first time the effect is iterated, if that is its start tick,
    /// `CalcPanningAndVolume` + `PlaySoundFX`), so both belong to the client.
    Effect { effect: u8, sound: u16, x: i32, y: i32 },
    /// `AddEffect(effect, sound, x, y, delay, 0, 1)`: as [`SimEvent::Effect`], starting `delay`
    /// ticks after this one (record +8 = `GameTick + delay`, 0x4e2190), so its sound plays then.
    /// The catapult's impact (0x594c80).
    EffectLater { effect: u8, sound: u16, x: i32, y: i32, delay: u32 },
    /// `CEffects` vtbl+0x14 (0x4e3c80 → 0x4e2260) `(effect, sound, x, y, player, 0)`: a tribe
    /// effect of `player` at node (x, y) starting this tick: gfx job .data 0x1000774[effect]
    /// (effects 1..=3: jobs 0x14d..0x14f) of the settler gfx file of the player's tribe (+0x14),
    /// in the player's colour (0x149b5e8), with `sound`. The dying warrior's (0x580bff).
    TribeEffect { effect: u8, sound: u16, x: i32, y: i32, player: u8 },
    /// `CEffects` vtbl+0x18 (0x4e3b20, [`crate::projectile`]): a flying effect of `kind`
    /// (`projectile::KINDS`, .rdata 0x1000640) from node `from` — drawn from its screen position
    /// plus the pixel `offset` (a tower soldier's slot) — to node `to`. It appears `delay` ticks
    /// after this tick and flies `frames` ticks (0x4e2310; the client draws the arc from the
    /// kind's record).
    Projectile { kind: u8, from: (i32, i32), to: (i32, i32), delay: i32, offset: (i32, i32), frames: u32 },
    /// A sound at node (x, y) that the original plays from the logic when the node is visible to
    /// the local player (`IsPositionVisible`, then `CalcPanningAndVolume` + `PlaySoundFX`): the
    /// falling tree (`CTree::LogicUpdate` stage 4, release 0x593c50, `SOUND_WORK_TREEFALL`).
    Sound { sound: u16, x: i32, y: i32 },
    /// A text message for `player` about node (x, y): the logic's `0x52a460(text, player, x, y)`
    /// (`filtered`: its class must pass the player's filter) or `0x52a320(text, player, x, y,
    /// 0, 0)` (not filtered). `text` is the string id. The original makes the call only when
    /// `player` is the local one; the client keeps the list (s4-game `messages.rs`).
    Message { player: u8, text: u16, x: i32, y: i32, filtered: bool },
    /// 0x4c4e00 `(x, y)`: an entity of `player` at (x, y) is hit (soldiers 0x589250, tower
    /// soldiers 0x58c200), just before the damage. The original calls it for the local player's
    /// entities; its throttle before "You are under attack!" is the client's (`messages.rs`).
    Attacked { player: u8, x: i32, y: i32 },
    /// `IBuildingRole::TakeJobTrigger(trigger)` (0x540da0) on `building`: a worker's task with a
    /// building trigger (job byte +9) started. Starts / stops the building's patch animations,
    /// display state the client keeps (s4-game `patches.rs`).
    JobTrigger { building: u16, trigger: u8 },
    // Maps onto s4-audio's `AudioEvent::Fx` (after the client's chance/fog roll; effects when
    // drawn) and `AudioEvent::WarAction`; new variants are added as systems are ported.
    /// `CastSpell` succeeded (0x4f5570): the AI's notice (0x146b148 vtbl+0x2c(1, player, spell,
    /// pos)) and, when the map script asked for it (`GameScriptManager +0x5f4 > 0`), its
    /// callback (0x5bdd60 with player, spell, x, y). A deferred spell happens 12 ticks later.
    SpellCast { player: u8, spell: u8, x: i32, y: i32 },
    /// The end of a manakopter's `LogicUpdate` (0x56b4e0, from 0x56b71a): its rotor sound at
    /// its node (x, y). `ordered` (state bit 1): `SOUND_MAGIC_MANAKOPTERTURN` (99) once, else
    /// `SOUND_MAGIC_MANAKOPTERROTOR` (100) looped; only if its shadow effect was drawn since the
    /// last update (the client's, s4-game `sound.rs`).
    Rotor { id: u16, x: i32, y: i32, ordered: bool },
    /// `CManakopter::Delete` (0x56b330): both rotor sounds stop.
    RotorStop { id: u16 },
    /// The Maya's spell 5 (0x4f4780, 0x4f4846..0x4f48b0): a vanished bowman becomes a butterfly
    /// at its node — when the ambient manager (0x149c5f8, the client's `CAnimalMgr` part) exists:
    /// effect type `0xb + its RNG % 3` (`CEffects` vtbl+0x28), its count +0xc (and +0x10 at the
    /// limit +8) up, and a kind-7 move event one tick later (`(x << 16) | effect id`, `y`). Client
    /// state (s4-game `ambient.rs`), as the other ambient creatures.
    Butterfly { x: i32, y: i32 },
    /// A call into a system that is not ported yet (see `jobs::hooks`).
    Unported { what: &'static str, entity: u16 },
}

impl SimEvent {
    /// 0x4c4e00 for the entity `target`, if it exists.
    pub fn attacked(world: &crate::World, target: u16) -> Option<SimEvent> {
        let t = world.entities.by_index(target as u32)?;
        Some(SimEvent::Attacked { player: t.owner, x: t.pos.x(), y: t.pos.y() })
    }

    /// `0x52a460(text, owner, x, y)` about the node `pos` (a filtered [`SimEvent::Message`]).
    pub fn warning(player: u8, text: u16, pos: crate::grid::Y16X16) -> SimEvent {
        SimEvent::Message { player, text, x: pos.x(), y: pos.y(), filtered: true }
    }
}
