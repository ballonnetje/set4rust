//! The campaign missions played to their end through the game's sim thread and the mission's
//! real script (`Script\<map>.txt`), as `tutorial.rs` does for the tutorials. A mission without
//! its own victory check ends by `ScriptDefaultVictoryConditionCheck` (every 8th tick after 140):
//! every other player has lost, i.e. has no READY, inhabited military building (types 46, 47,
//! 48, 50, 51; `World::default_player_lost`).
//!
//! The player's side is a blunt conqueror: soldiers are added next to the player's building
//! nearest to the enemy's nearest military building (as the scripts' `Settlers.AddSettlers`
//! does, `create::add_settlers_around`) and sent there (`SendTo`, the right click), wave after
//! wave. So a test exercises the script, the AI's defence, attacks and conquest, and the
//! victory check; not the player's economy. Each mission says what more its script wants.

#![cfg(test)]

use std::sync::atomic::Ordering;

use crate::commands::{Stamped, execution_tick};
use crate::sim::{Clock, SimHandle, SimSetup};
use s4_sim::entity::ObjType;

/// The military building types of the default lost check.
const MILITARY: [u16; 5] = [46, 47, 48, 50, 51];
/// `SETTLER_SWORDSMAN_03`.
const SWORDSMAN_3: u16 = 31;
/// `SETTLER_BOWMAN_03`.
const BOWMAN_3: u16 = 34;
/// `SETTLER_GARDENER`.
const GARDENER: u16 = 50;

pub struct Mission {
    pub sim: SimHandle,
    pub name: String,
    /// Per unit the last move order: (x, y, tick). A player does not click every few seconds;
    /// a walk that is ordered again starts over.
    sent: std::cell::RefCell<std::collections::HashMap<u16, (i32, i32, u32)>>,
    /// What the script shares with the client (its debug lines: [`Mission::vars`]).
    tutorial: crate::tutorial::Shared,
    /// The conquest's waves bring bowmen 3 too (MD_Bonus: not, see its test).
    pub bowmen: std::cell::Cell<bool>,
}

impl Mission {
    /// `Campaign/<map>`; `None` without the game data.
    pub fn start(map: &str) -> Option<Mission> {
        let map_path = s4_viewer::scene::resolve_map(&format!("Campaign/{map}"));
        if !map_path.exists() {
            return None;
        }
        let tutorial = crate::tutorial::shared();
        // The briefing's start (`CStateBriefing` 0x510970 at 0x510c5d and `CStateMD2Briefing`
        // 0x523710 both call the game setup 0x5069b0 with the difficulty): the map's own setup
        // with its other slots computers (`ai::add_players_at_start`), here at easy.
        let players = crate::frontend::setup::campaign_setup(&map_path, 0);
        let setup = SimSetup {
            map_path,
            prefer_file_script: true,
            run_script: true,
            fog: false,
            outcome: Default::default(),
            tutorial: tutorial.clone(),
            net: None,
            players,
            load: None,
        };
        Some(Mission { sim: SimHandle::spawn(setup, Clock::Manual).unwrap(), name: map.to_string(), sent: Default::default(), tutorial, bowmen: std::cell::Cell::new(true) })
    }

    pub fn tick(&self) -> u32 {
        self.sim.snapshot().tick
    }

    pub fn outcome(&self) -> u8 {
        self.sim.outcome.load(Ordering::Relaxed)
    }

    pub fn command(&self, command: s4_sim::Command) {
        let tick = execution_tick(self.tick());
        self.sim.command(Stamped { tick, command: s4_sim::PlayerCommand { player: 1, command } });
    }

    /// Run `f` on the world between ticks and return its result.
    pub fn ask<T: Send + 'static>(&self, f: impl FnOnce(&mut s4_sim::World) -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        self.sim.with_world(move |w| {
            let _ = tx.send(f(w));
        });
        rx.recv().unwrap()
    }

    /// The enemies' (other teams') READY, inhabited military buildings: (id, owner, x, y, race).
    pub fn enemy_military(&self) -> Vec<(u16, u8, i32, i32, u8)> {
        self.ask(|w| {
            w.entities
                .iter()
                .filter(|(id, e)| {
                    e.obj_type == ObjType::Building
                        && w.alliances.alliance_id[e.owner as usize % 9] != w.alliances.alliance_id[1]
                        && MILITARY.contains(&e.kind)
                        && e.flags & s4_sim::entity::flag::READY != 0
                        && s4_sim::buildings::have_inhabitant(w, *id)
                })
                .map(|(id, e)| (id, e.owner, e.x(), e.y(), e.race))
                .collect()
        })
    }

    /// Player 1's settlers of the kinds `pick` takes, without a home (a garrison is ATTACHED to
    /// its tower): (id, x, y).
    pub fn units(&self, pick: impl Fn(&s4_sim::World, u16) -> bool + Send + 'static) -> Vec<(u16, i32, i32)> {
        self.ask(move |w| {
            w.entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && !e.is_dead() && pick(w, e.kind) && e.flags & s4_sim::entity::flag::ATTACHED == 0)
                .map(|(i, e)| (i, e.x(), e.y()))
                .collect()
        })
    }

    pub fn soldiers(&self) -> Vec<(u16, i32, i32)> {
        // Warrior types 2..=5 fight; 7 are the specialists (gardeners, pioneers, thieves, …).
        self.units(|w, k| (2..=5).contains(&w.rules.warrior_type(k)))
    }

    pub fn gardeners(&self) -> Vec<(u16, i32, i32)> {
        self.units(|_, k| k == GARDENER)
    }

    /// The land sector of node `(x, y)` (`CTiling`), 0 off the map.
    pub fn sector(&self, x: i32, y: i32) -> u32 {
        self.ask(move |w| w.grid.checked_index(x, y).map_or(0, |n| w.tiling.sector_of(n) as u32))
    }

    /// The land around a building at `(x, y)`: the sector of the first free node (no map
    /// object, none of node flags 0x39) at least 4 from it.
    pub fn land_of(&self, x: i32, y: i32) -> u32 {
        self.ask(move |w| land_near(w, x, y, 4).map_or(0, |(_, _, s)| s))
    }

    /// `(kind, n)` settlers of player 1 at the spawn point for a target at `(x, y)`
    /// ([`spawn_point`]: its building nearest on that land, else at least `min` from it).
    pub fn reinforce_at(&self, x: i32, y: i32, min: i32, wave: &'static [(u16, u32)]) -> u32 {
        self.ask(move |w| {
            let Some((bx, by)) = spawn_point(w, x, y, min) else { return 0 };
            wave.iter().map(|&(k, n)| s4_sim::create::add_settlers_around(w, bx, by, 1, k, n)).sum()
        })
    }

    /// Send the ones of `units` on `land` to `(x, y)`; fewer than `keep` there: `wave` first,
    /// near the target `(tx, ty)` (at least `min` from it on an island; `min` < 0: at `(x, y)`).
    #[allow(clippy::too_many_arguments)]
    fn send_wave(&self, units: &dyn Fn() -> Vec<(u16, i32, i32)>, land: u32, (tx, ty, min): (i32, i32, i32), x: i32, y: i32, keep: usize, wave: &'static [(u16, u32)]) -> usize {
        let on = |v: Vec<(u16, i32, i32)>| v.into_iter().filter(|s| self.sector(s.1, s.2) == land).map(|s| s.0).collect::<Vec<u16>>();
        let mut there = on(units());
        if there.len() < keep {
            if min < 0 {
                // At the destination itself (the gardeners' frontier, outside the auras: the
                // way there from a base could cross one).
                self.ask(move |w| {
                    if let Some((bx, by, _)) = land_near(w, x, y, 0) {
                        for &(k, n) in wave {
                            s4_sim::create::add_settlers_around(w, bx, by, 1, k, n);
                        }
                    }
                });
            } else {
                self.reinforce_at(tx, ty, min, wave);
            }
            there = on(units());
        }
        // New orders only: another destination, or the same one again after 3000 ticks.
        let now = self.tick();
        let fresh: Vec<u16> = {
            let mut sent = self.sent.borrow_mut();
            there
                .iter()
                .copied()
                .filter(|u| {
                    let again = sent.get(u).is_none_or(|&(sx, sy, at)| (sx, sy) != (x, y) || now >= at + 3000);
                    if again {
                        sent.insert(*u, (x, y, now));
                    }
                    again
                })
                .collect()
        };
        for chunk in fresh.chunks(100) {
            self.command(s4_sim::Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: chunk.to_vec() });
        }
        there.len()
    }

    /// What to go for: the other tribes' military buildings first; once only the Dark Tribe's
    /// are left, its mushroom farms (the temple's aura grows with their number, 0x5458c0), then
    /// its temples / fortresses.
    pub fn targets(&self, enemies: &[(u16, u8, i32, i32, u8)]) -> Vec<(u16, u8, i32, i32, u8)> {
        let others: Vec<_> = enemies.iter().filter(|e| e.4 != 3).copied().collect();
        if !others.is_empty() {
            return others;
        }
        let dark: Vec<u8> = enemies.iter().map(|e| e.1).collect();
        let farms = self.ask(move |w| {
            w.entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Building && e.kind == 49 && e.race == 3 && dark.contains(&e.owner) && !e.is_dead())
                .map(|(id, e)| (id, e.owner, e.x(), e.y(), 3u8))
                .collect::<Vec<_>>()
        });
        if farms.is_empty() {
            return enemies.to_vec();
        }
        // Farms outside every temple's aura first (gardeners on dark land there die).
        let auras = self.ask(|w| auras(w));
        let outside: Vec<_> = farms.iter().copied().filter(|f| !in_aura(&auras, f.2, f.3)).collect();
        if outside.is_empty() { farms } else { outside }
    }

    /// The dark node of `land` within 40 of `(tx, ty)` nearest to player 1's gardeners there (their
    /// centre), else to its nearest building on that land; `(tx, ty)` when there is none.
    pub fn dark_frontier(&self, tx: i32, ty: i32, land: u32) -> (i32, i32) {
        self.ask(move |w| {
            let on = |x: i32, y: i32| w.grid.checked_index(x, y).is_some_and(|n| w.tiling.sector_of(n) as u32 == land);
            let g: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && e.kind == GARDENER && !e.is_dead() && on(e.x(), e.y())).map(|(_, e)| (e.x(), e.y())).collect();
            let from = if g.is_empty() {
                spawn_point(w, tx, ty, 30)
            } else {
                let n = g.len() as i32;
                Some((g.iter().map(|p| p.0).sum::<i32>() / n, g.iter().map(|p| p.1).sum::<i32>() / n))
            };
            let Some((fx, fy)) = from else { return (tx, ty) };
            let auras = auras(w);
            let dark = |x: i32, y: i32| on(x, y) && w.grid.checked_index(x, y).is_some_and(|n| w.layers.flags[n] & 4 != 0);
            // Outside the auras if any is left there, else the nearest anyway.
            let pick = |safe: bool| {
                s4_sim::spiral::walk(w.grid, tx, ty, 40)
                    .filter(|&(x, y)| dark(x, y) && (!safe || !in_aura(&auras, x, y)))
                    .min_by_key(|&(x, y)| s4_sim::spiral::grid_distance(x - fx, y - fy))
            };
            pick(true).or_else(|| pick(false)).unwrap_or((tx, ty))
        })
    }

    /// Player 1's start (the map's): where the objectives' units come from.
    pub fn home(&self) -> (i32, i32) {
        self.ask(|w| w.player(1).map_or((0, 0), |p| (p.start_x as i32, p.start_y as i32)))
    }

    /// `ids` sent to `(x, y)`, new orders only (another destination, or the same again after
    /// 3000 ticks).
    pub fn order(&self, ids: Vec<u16>, x: i32, y: i32) {
        let now = self.tick();
        let fresh: Vec<u16> = {
            let mut sent = self.sent.borrow_mut();
            ids.into_iter()
                .filter(|u| {
                    let again = sent.get(u).is_none_or(|&(sx, sy, at)| (sx, sy) != (x, y) || now >= at + 3000);
                    if again {
                        sent.insert(*u, (x, y, now));
                    }
                    again
                })
                .collect()
        };
        for chunk in fresh.chunks(100) {
            self.command(s4_sim::Command::SendTo { x: x as u16, y: y as u16, mode: 0, entities: chunk.to_vec() });
        }
    }

    /// The free node nearest to `(x, y)` (within 20) and its land.
    pub fn spot(&self, x: i32, y: i32) -> Option<(i32, i32, u32)> {
        self.ask(move |w| land_near(w, x, y, 0))
    }

    /// Player 1's settlers of `kind` on the map standing on `land`, by id.
    pub fn kind_on(&self, kind: u16, land: u32) -> Vec<u16> {
        let mut ids = self.ask(move |w| {
            w.entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && e.kind == kind && !e.is_dead() && e.flags & s4_sim::entity::flag::ATTACHED == 0)
                .filter(|(_, e)| w.grid.checked_index(e.x(), e.y()).is_some_and(|n| w.tiling.sector_of(n) as u32 == land))
                .map(|(i, _)| i)
                .collect::<Vec<u16>>()
        });
        ids.sort_unstable();
        ids
    }

    /// At least `n` settlers of `kind` of player 1 on the land of `(x, y)`, the missing ones added
    /// where reinforcements for that land appear ([`spawn_point`]). Returns them (by id) and the
    /// free node nearest to `(x, y)`.
    pub fn muster(&self, kind: u16, n: usize, x: i32, y: i32) -> Option<(Vec<u16>, (i32, i32))> {
        let (dx, dy, land) = self.spot(x, y)?;
        let have = self.kind_on(kind, land);
        if have.len() >= n {
            return Some((have, (dx, dy)));
        }
        let missing = (n - have.len()) as u32;
        self.ask(move |w| {
            if let Some((sx, sy)) = spawn_point(w, x, y, 10) {
                s4_sim::create::add_settlers_around_ex(w, sx, sy, 1, kind, missing, 1);
            }
        });
        Some((self.kind_on(kind, land), (dx, dy)))
    }

    /// At least `n` settlers of `kind` of player 1 on the land of `(x, y)` ([`Mission::muster`]),
    /// kept going there. Every 50000 ticks: how many are within 20 of it.
    pub fn gather(&self, kind: u16, n: usize, x: i32, y: i32) {
        let Some((ids, (dx, dy))) = self.muster(kind, n, x, y) else { return };
        if self.tick() % 50_000 < 200 {
            let near = self.units(move |_, k| k == kind).iter().filter(|u| s4_sim::spiral::grid_distance(u.1 - x, u.2 - y) <= 20).count();
            eprintln!("{} tick {}: {} of kind {kind} on the land of ({x}, {y}), {near} within 20", self.name, self.tick(), ids.len());
        }
        self.order(ids, dx, dy);
    }

    /// At least `n` of `good` in player 1's statistic (`Goods.Amount`): the missing ones put down
    /// near `(x, y)`.
    pub fn stock(&self, good: u8, n: i32, x: i32, y: i32) {
        let have = self.ask(move |w| w.statistic.players.get(1).and_then(|p| p.goods.get(good as usize)).copied().unwrap_or(0));
        if have < n {
            self.piles(x, y, good, (n - have) as u32);
        }
    }

    /// Player `p`'s vehicles of `vtype` (1 war ship, 2 ferry, 4 war machine, 5 cart): (id, x, y).
    pub fn vehicles_of(&self, p: u8, vtype: u16) -> Vec<(u16, i32, i32)> {
        self.ask(move |w| s4_sim::vehicles::iter(w, p, vtype).filter_map(|id| w.entities.by_index(id as u32).map(|e| (id, e.x(), e.y()))).collect())
    }

    /// The open-water node nearest to `(x, y)` where a ship can be made (0x5bc2f0).
    pub fn water_near(&self, x: i32, y: i32) -> Option<(i32, i32)> {
        self.ask(move |w| {
            use s4_sim::vehicles::ship::can_place;
            s4_sim::spiral::walk(w.grid, x, y, 70).find(|&(a, b)| can_place(w, a, b)).or_else(|| {
                let size = w.grid.size() as i32;
                (0..size * size).map(|n| (n % size, n / size)).filter(|&(a, b)| can_place(w, a, b)).min_by_key(|&(a, b)| s4_sim::spiral::grid_distance(a - x, b - y))
            })
        })
    }

    /// The node where a ship can be made on the water of `to` (its ship sector), nearest to
    /// `(x, y)`.
    pub fn water_linked(&self, (x, y): (i32, i32), to: (i32, i32)) -> Option<(i32, i32)> {
        let (tx, ty) = self.water_near(to.0, to.1)?;
        self.ask(move |w| {
            use s4_sim::vehicles::ship;
            let sector = ship::sector(w, w.grid.checked_index(tx, ty));
            let size = w.grid.size() as i32;
            (0..size * size)
                .map(|n| (n % size, n / size))
                .filter(|&(a, b)| ship::sector(w, w.grid.checked_index(a, b)) == sector && ship::can_place(w, a, b))
                .min_by_key(|&(a, b)| s4_sim::spiral::grid_distance(a - x, b - y))
        })
    }

    /// At least `n` ships of `vtype` of player 1, the missing ones made on the water of `to`
    /// nearest to `(x, y)`. Returns them.
    pub fn fleet(&self, vtype: u16, n: usize, (x, y): (i32, i32), to: (i32, i32)) -> Vec<u16> {
        let have = self.vehicles_of(1, vtype).len();
        for _ in have..n {
            // Each on its own place: a cell holds few ships.
            let Some((wx, wy)) = self.water_linked((x, y), to) else { break };
            if self.vehicle(wx, wy, vtype as i32) == 0 {
                break;
            }
        }
        self.vehicles_of(1, vtype).into_iter().map(|v| v.0).collect()
    }

    /// Player `p`'s war ships sunk by player 1's: three of them (made next to it when fewer)
    /// sent onto the first one (a move order on an enemy within 2 attacks it, 0x5923b0).
    pub fn sink(&self, p: u8) {
        let Some(&(_, ex, ey)) = self.vehicles_of(p, 1).first() else { return };
        let own = self.fleet(1, 3, (ex, ey), (ex, ey));
        self.order(own, ex, ey);
    }

    /// Trees on the free land nodes within `r` of `(x, y)` until `n` stand there (deco kinds
    /// 1..=22, what `Game.GetAmountOfTreesInArea` counts): the foresters' work at once
    /// (`CTree` 0x593880, a growing tree).
    pub fn forest(&self, x: i32, y: i32, r: u8, n: usize) {
        self.ask(move |w| {
            let tree = |w: &s4_sim::World, n: usize| {
                let id = w.layers.deco_object[n];
                id != 0 && w.entities.by_index(id as u32).is_some_and(|e| e.obj_type == ObjType::Deco && (1..=22).contains(&e.kind))
            };
            let nodes: Vec<(i32, i32)> = s4_sim::spiral::walk(w.grid, x, y, r).collect();
            let mut have = nodes.iter().filter(|&&(a, b)| tree(w, w.grid.index(a as u32, b as u32))).count();
            for &(a, b) in &nodes {
                if have >= n {
                    break;
                }
                let i = w.grid.index(a as u32, b as u32);
                let free = w.layers.map_object[i] == 0 && w.layers.deco_object[i] == 0 && w.layers.flags[i] & 0x39 == 0 && w.layers.ground[i] > 8;
                if free && (a + b) % 2 == 0 && s4_sim::deco::tree::plant(w, a, b, 1 + ((a * 7 + b) % 8) as u16).is_some() {
                    have += 1;
                }
            }
        });
    }

    /// Gardeners light the dark land within `r` of `(x, y)`: up to `keep` of them, made at the
    /// dark node nearest to them, sent there. Whether the area is green (`Game.IsAreaGreen`).
    pub fn green(&self, x: i32, y: i32, r: i32, keep: usize) -> bool {
        if self.ask(move |w| w.is_area_green(x, y, r)) {
            return true;
        }
        let wave: &'static [(u16, u32)] = &[(GARDENER, 20)];
        let land = self.land_of(x, y);
        let (fx, fy) = self.dark_frontier(x, y, land);
        self.send_wave(&|| self.gardeners(), land, (x, y, -1), fx, fy, keep, wave);
        false
    }

    /// The mission's objective alone: `step` before every `every` ticks until the game is
    /// decided, `ticks`, or [`wall`] time. Returns the outcome.
    pub fn play(&self, ticks: u32, every: u32, step: &dyn Fn(&Mission)) -> u8 {
        let started = std::time::Instant::now();
        loop {
            let t = self.tick();
            let o = self.outcome();
            if o != 0 {
                eprintln!("{}: decided ({o}) at tick {t}", self.name);
                return o;
            }
            assert!(t < ticks, "{}: not decided after {ticks} ticks", self.name);
            assert!(started.elapsed() < wall(), "{}: not decided after {:?} (tick {t})", self.name, wall());
            if t % 50_000 < every {
                eprintln!("{} tick {t}: vars {}", self.name, self.vars());
            }
            step(self);
            self.sim.step(every);
        }
    }

    /// At least `n` of `good` of player 1 within `r` of `(x, y)` (`Goods.GetAmountInArea`): the
    /// missing ones put down at `(x, y)` once player 1 owns it.
    pub fn area_stock(&self, good: u8, n: i32, x: i32, y: i32, r: i32) {
        let (have, own) = self.ask(move |w| (w.goods_amount_in_area(1, good as i32, x, y, r), w.grid.checked_index(x, y).is_some_and(|i| w.owner_of(i) == 1)));
        if have < n && own {
            self.piles(x, y, good, (n - have) as u32);
        }
    }

    /// Player 1 owning the land within `r` of `(x, y)` (`Game.IsAreaOwned`): a castle placed on
    /// the first free (unowned) land node there; whether it is owned already.
    pub fn claim(&self, x: i32, y: i32, r: u8) -> bool {
        let castle = k("Buildings", "CASTLE") as u8;
        let free = self.ask(move |w| {
            if w.is_area_owned(1, x, y, r as i32) {
                return None;
            }
            Some(s4_sim::spiral::walk(w.grid, x, y, r).find(|&(a, b)| {
                let i = w.grid.index(a as u32, b as u32);
                w.layers.ground[i] > 8 && w.owner_of(i) == 0 && w.layers.flags[i] & 0x39 == 0
            }))
        });
        match free {
            None => true,
            Some(Some((a, b))) => {
                self.place(a, b, castle);
                false
            }
            Some(None) => false,
        }
    }

    /// A land path from `(xa, ya)` to `(xb, yb)` (breadth first over the six neighbours, nodes
    /// without `BLOCKED_LAND`), every 12th node of it; empty without one.
    pub fn land_route(&self, (xa, ya): (i32, i32), (xb, yb): (i32, i32)) -> Vec<(i32, i32)> {
        self.ask(move |w| {
            let size = w.grid.size() as i32;
            let open = |x: i32, y: i32| w.grid.checked_index(x, y).is_some_and(|i| w.layers.flags[i] & s4_sim::grid::flag::BLOCKED_LAND == 0);
            let mut from = vec![u32::MAX; (size * size) as usize];
            let mut queue = std::collections::VecDeque::from([(xa, ya)]);
            from[(ya * size + xa) as usize] = (ya * size + xa) as u32;
            while let Some((x, y)) = queue.pop_front() {
                if (x, y) == (xb, yb) {
                    let mut path = vec![(x, y)];
                    let mut at = (y * size + x) as u32;
                    while from[at as usize] != at {
                        at = from[at as usize];
                        path.push(((at as i32) % size, (at as i32) / size));
                    }
                    path.reverse();
                    return path.into_iter().step_by(12).collect();
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if open(nx, ny) && from[(ny * size + nx) as usize] == u32::MAX {
                        from[(ny * size + nx) as usize] = (y * size + x) as u32;
                        queue.push_back((nx, ny));
                    }
                }
            }
            Vec::new()
        })
    }

    /// Castles on the free land along `route`, one per call, where no player owns the land
    /// yet (a land path for `Game.ArePlayerAreasConnected`). Returns the route's nodes owned by
    /// others than `ours`.
    pub fn bridge(&self, route: &[(i32, i32)], ours: &'static [u8]) -> Vec<(i32, i32, u8)> {
        let castle = k("Buildings", "CASTLE") as u8;
        let route = route.to_vec();
        let others = self.ask(move |w| route.iter().filter_map(|&(x, y)| w.grid.checked_index(x, y).map(|i| (x, y, w.owner_of(i)))).filter(|n| !ours.contains(&n.2)).collect::<Vec<_>>());
        if let Some(&(x, y, _)) = others.iter().find(|n| n.2 == 0) {
            self.place(x, y, castle);
        }
        others
    }

    /// The script's `Vars.Save1..9`, read through the cheat console (`Debug.ShowTextMessage`,
    /// one tick).
    pub fn vars(&self) -> String {
        self.eval(&(1..=9).map(|i| format!("(Vars.Save{i} or -1)")).collect::<Vec<_>>().join(" .. ' ' .. "))
    }

    /// A Lua expression of the mission's script state, as text (the cheat console's
    /// `Debug.ShowTextMessage`, one tick).
    pub fn eval(&self, expr: &str) -> String {
        self.sim.console(&format!("Debug.ShowTextMessage({expr})"));
        self.sim.step(1);
        let mut t = self.tutorial.lock().unwrap_or_else(|e| e.into_inner());
        let line = t.requests.iter().rev().find_map(|r| if let crate::tutorial::Request::DebugText(s) = r { Some(s.clone()) } else { None });
        t.requests.retain(|r| !matches!(r, crate::tutorial::Request::DebugText(_)));
        line.unwrap_or_default()
    }

    /// `n` settlers of `kind` of player 1 at each of `places`: per land the living ones (by id)
    /// shared out in turn ([`Mission::muster`] for all of that land's places), each group sent
    /// to its place.
    pub fn squads(&self, kind: u16, n: usize, places: &[(i32, i32)]) {
        let spots: Vec<Option<(i32, i32, u32)>> = places.iter().map(|&(x, y)| self.spot(x, y)).collect();
        let mut done = vec![false; places.len()];
        for i in 0..places.len() {
            let Some((_, _, land)) = spots[i] else { continue };
            if done[i] {
                continue;
            }
            let same: Vec<usize> = (i..places.len()).filter(|&j| spots[j].is_some_and(|s| s.2 == land)).collect();
            let Some((ids, _)) = self.muster(kind, n * same.len(), places[i].0, places[i].1) else { continue };
            for (g, &j) in same.iter().enumerate() {
                done[j] = true;
                let (dx, dy, _) = spots[j].unwrap();
                self.order(ids.iter().copied().skip(g * n).take(n).collect(), dx, dy);
            }
        }
    }

    /// At least `n` vehicles of `vtype` of player 1 (war machines 4, carts 5) on the land of
    /// `(x, y)`, the missing ones made where reinforcements for it appear, sent there.
    pub fn wheels(&self, vtype: u16, n: usize, x: i32, y: i32) {
        let Some((dx, dy, land)) = self.spot(x, y) else { return };
        let on = |m: &Mission| {
            let v = m.vehicles_of(1, vtype);
            m.ask(move |w| v.into_iter().filter(|&(_, a, b)| w.grid.checked_index(a, b).is_some_and(|i| w.tiling.sector_of(i) as u32 == land)).map(|v| v.0).collect::<Vec<u16>>())
        };
        let have = on(self).len();
        if have < n {
            self.ask(move |w| {
                // Next to the spawn point (a building's own node takes no vehicle).
                let Some((sx, sy)) = spawn_point(w, x, y, 10) else { return };
                let mut made = have;
                for (px, py) in s4_sim::spiral::walk(w.grid, sx, sy, 12) {
                    if made >= n {
                        break;
                    }
                    if land_near(w, px, py, 0).is_some_and(|l| (l.0, l.1) == (px, py) && l.2 == land) && s4_sim::vehicles::scripted::add_vehicle(w, px, py, 1, vtype as i32, -1, 100, 0) != 0 {
                        made += 1;
                    }
                }
            });
        }
        self.order(on(self), dx, dy);
    }

    /// The 16 × 16 cells with at least `n` dark nodes (`Game.GetNumberOfSquaresWithDarkLand`),
    /// and gardeners sent into the first one; returns how many are left.
    pub fn dark_cells(&self, n: u8, keep: usize) -> usize {
        let cells = self.ask(move |w| {
            let side = (w.grid.size() >> 4) as i32;
            (0..side).flat_map(|cy| (0..side).map(move |cx| (cx, cy))).filter(|&(cx, cy)| w.animals.cells.get(cx, cy).dark() >= n).collect::<Vec<_>>()
        });
        if let Some(&(cx, cy)) = cells.first() {
            self.green(cx * 16 + 8, cy * 16 + 8, 11, keep);
        }
        cells.len()
    }

    /// Every 50000 ticks: player 1's vehicle counts (0x1549a8c, building + finished, types 1..=5)
    /// and `Settlers.Amount` of `kinds`.
    pub fn census(&self, kinds: &'static [u16]) {
        if self.tick() % 50_000 >= 200 {
            return;
        }
        let (v, s) = self.ask(move |w| {
            let v: Vec<(u16, u16)> = (1..=5).map(|t| (w.vehicles.building[6 + t], w.vehicles.finished[6 + t])).collect();
            let s: Vec<i32> = kinds.iter().map(|&k| w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && e.kind == k && !e.is_dead()).count() as i32).collect();
            (v, s)
        });
        eprintln!("{} tick {}: vehicles (building, finished) {v:?}, settlers {kinds:?} {s:?}", self.name, self.tick());
    }

    /// `Vars.Save1..9` as numbers (−1 for nil).
    pub fn var_list(&self) -> Vec<i32> {
        self.vars().split(' ').map(|v| v.parse::<f64>().map_or(-1, |f| f as i32)).collect()
    }

    /// Ferries of player 1 made on the water of `to` nearest to home, loaded with `cargo`
    /// (`Settlers.AddSettlersToFerry`, per ferry its `(type, n)`s, once), sent to `to`.
    pub fn ferry_out(&self, cargo: &[&[(u16, i32)]], loaded: &std::cell::Cell<bool>, to: (i32, i32)) {
        let home = self.home();
        let Some((tx, ty)) = self.water_near(to.0, to.1) else { return };
        let ferries = self.fleet(2, cargo.len(), home, (tx, ty));
        if !loaded.get() && ferries.len() >= cargo.len() {
            let jobs: Vec<(u16, u16, i32)> = ferries.iter().zip(cargo).flat_map(|(&f, c)| c.iter().map(move |&(k, n)| (f, k, n))).collect();
            self.ask(move |w| {
                for (f, k, n) in jobs {
                    s4_sim::vehicles::scripted::add_settlers_to_ferry(w, f as i32, k as i32, n);
                }
            });
            loaded.set(true);
        }
        self.order(ferries, tx, ty);
    }

    /// The player's and its allies' military buildings with enemy warriors within 12 get the
    /// player's soldiers next to them (up to 8 within 8 of it): an empty tower an enemy walks into is his (a Dark Tribe soldier
    /// destroys it: `SettlerEnter` 0x5488d0 → `FirstSettlerEnter` 0x5484f0), and the conquest
    /// takes the soldiers away from home.
    pub fn defend(&self) {
        self.ask(|w| {
            let warrior = |w: &s4_sim::World, e: &s4_sim::entity::Entity| e.obj_type == ObjType::Settler && !e.is_dead() && (2..=6).contains(&w.rules.warrior_type(e.kind));
            let threatened: Vec<(i32, i32, usize)> = w
                .entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Building && !w.alliances.hostile(1, e.owner) && e.owner != 0 && MILITARY.contains(&e.kind) && !e.is_dead())
                .filter_map(|(_, b)| {
                    let (x, y) = (b.x(), b.y());
                    let near = |r: i32, mine: bool| {
                        w.entities
                            .iter()
                            .filter(|(_, e)| warrior(w, e) && (e.owner == 1) == mine && (mine || w.alliances.hostile(1, e.owner)) && s4_sim::spiral::grid_distance(e.x() - x, e.y() - y) <= r)
                            .count()
                    };
                    (near(12, false) > 0).then(|| (x, y, near(8, true)))
                })
                .collect();
            for (x, y, have) in threatened.into_iter().take(4) {
                if have < 8 {
                    s4_sim::create::add_settlers_around(w, x, y, 1, SWORDSMAN_3, (8 - have) as u32);
                }
            }
        });
    }

    // The objectives' shortcuts: what a script itself does with `Goods.AddPileEx`,
    // `Buildings.AddBuilding`, `Settlers.AddSettlers` and `Vehicles.AddVehicle`. A test that
    // uses them exercises the mission's check and the host's answers, not the player's economy.

    /// `amount` goods near `(x, y)` in piles of 8 (`CPileMgr::SearchSpaceForGoods`, as
    /// `Goods.AddPileEx` 0x5c8730).
    pub fn piles(&self, x: i32, y: i32, good: u8, amount: u32) {
        self.ask(move |w| {
            let mut left = amount;
            while left > 0 {
                let n = left.min(8);
                s4_sim::pile::search_space_for_goods(w, x, y, good, n as u8);
                left -= n;
            }
        });
    }

    /// A finished building of player 1 on the first place along the spiral around `(x, y)`
    /// (radius 0x18, as `Buildings.AddBuildingEx` 0x5c4d70 → 0x53c580). Returns its id, 0 if none.
    pub fn place(&self, x: i32, y: i32, building: u8) -> u16 {
        self.ask(move |w| s4_sim::build::scripted::add_building_scripted(w, x, y, 1, building, -1))
    }

    /// A vehicle of player 1 at `(x, y)` (`Vehicles.AddVehicle` 0x5c1280). Returns its id.
    pub fn vehicle(&self, x: i32, y: i32, kind: i32) -> u16 {
        self.ask(move |w| s4_sim::vehicles::scripted::add_vehicle(w, x, y, 1, kind, -1, 100, 0))
    }

    /// Conquer: every `every` ticks, the soldiers outside go to the enemy military building
    /// nearest to them; fewer than `keep` of them on its land: `wave` more. A Dark Tribe
    /// building (temple, fortress) falls to light land only: gardeners go there too (its land
    /// turning light under it destroys it, 0x4cbc15). Until the game is decided, `ticks` or
    /// three minutes of wall time. Returns the outcome.
    pub fn conquer(&self, ticks: u32, every: u32, keep: usize, wave: u32) -> u8 {
        self.conquer_and(ticks, every, keep, wave, &|_| {})
    }

    /// [`Mission::conquer`] with `extra`, the mission's own objective, run before every step.
    pub fn conquer_and(&self, ticks: u32, every: u32, keep: usize, wave: u32, extra: &dyn Fn(&Mission)) -> u8 {
        let bow = if self.bowmen.get() { wave / 2 } else { 0 };
        let soldiers: &'static [(u16, u32)] = Box::leak(vec![(SWORDSMAN_3, wave), (BOWMAN_3, bow)].into_boxed_slice());
        let gardeners: &'static [(u16, u32)] = Box::leak(vec![(GARDENER, wave * 2)].into_boxed_slice());
        let started = std::time::Instant::now();
        let mut last_log = 0;
        // A target that holds out (out of reach, say) is put aside for a while: (id, since).
        let mut current: (u16, u32) = (0, 0);
        let mut aside: std::collections::HashMap<u16, u32> = std::collections::HashMap::new();
        loop {
            let lap = std::time::Instant::now();
            let t = self.tick();
            let o = self.outcome();
            if o != 0 {
                eprintln!("{}: decided ({o}) at tick {t}", self.name);
                return o;
            }
            assert!(t < ticks, "{}: not decided after {ticks} ticks; enemy military {:?}", self.name, self.enemy_military());
            assert!(started.elapsed() < wall(), "{}: not decided after {:?} (tick {t}); enemy military {:?}", self.name, wall(), self.enemy_military());
            let enemies = self.enemy_military();
            let mut targets = self.targets(&enemies);
            if targets.iter().any(|e| aside.get(&e.0).is_none_or(|&until| until <= t)) {
                targets.retain(|e| aside.get(&e.0).is_none_or(|&until| until <= t));
            }
            let ours = self.soldiers();
            if let Some(&(_, _, ex, ey, _)) = targets.first() {
                // The target: the enemy building nearest to the soldiers' centre (else the first).
                let (cx, cy) = if ours.is_empty() {
                    (ex, ey)
                } else {
                    let n = ours.len() as i32;
                    (ours.iter().map(|s| s.1).sum::<i32>() / n, ours.iter().map(|s| s.2).sum::<i32>() / n)
                };
                let &(id, owner, tx, ty, race) = targets.iter().min_by_key(|e| s4_sim::spiral::grid_distance(e.2 - cx, e.3 - cy)).unwrap();
                if current.0 != id {
                    current = (id, t);
                } else if t >= current.1 + 12_000 && race != 3 {
                    aside.insert(id, t + 30_000);
                    current = (0, t);
                }
                let land = self.land_of(tx, ty);
                let (n, g) = if race == 3 {
                    // The temple's aura (0x545ba0) strikes enemy warriors on dark land within 24
                    // rings: the gardeners light the land from its edge in, the soldiers guard them.
                    let (fx, fy) = self.dark_frontier(tx, ty, land);
                    let g = self.send_wave(&|| self.gardeners(), land, (tx, ty, -1), fx, fy, keep * 5, gardeners);
                    (self.send_wave(&|| self.soldiers(), land, (tx, ty, -1), fx, fy, keep, soldiers), g)
                } else {
                    // Waves while fewer than `keep` swordsmen (warrior type 2) are there: bowmen
                    // alone do not take a tower (MD_conflict3 stood with 39 of them at one).
                    let bows = self.send_wave(&|| self.units(|w, k| (3..=5).contains(&w.rules.warrior_type(k))), land, (tx, ty, 12), tx, ty, 0, &[]);
                    (bows + self.send_wave(&|| self.units(|w, k| w.rules.warrior_type(k) == 2), land, (tx, ty, 12), tx, ty, keep, soldiers), 0)
                };
                if t >= last_log + 10_000 {
                    last_log = t;
                    let dark = self.ask(move |w| s4_sim::spiral::walk(w.grid, tx, ty, 24).filter(|&(x, y)| w.grid.checked_index(x, y).is_some_and(|n| w.layers.flags[n] & 4 != 0)).count());
                    eprintln!("{} tick {t}: {} enemy military buildings, {n} soldiers and {g} gardeners on its land → building {id} of player {owner} (race {race}) at ({tx}, {ty}), {dark} dark nodes within 24", self.name, enemies.len());
                }
            }
            self.defend();
            extra(self);
            let t1 = std::time::Instant::now();
            self.sim.step(every);
            if std::env::var("S4_MISSION_TRACE").is_ok() {
                let (entities, by) = self.ask(|w| {
                    let mut c: std::collections::HashMap<(u8, u16, u8), usize> = Default::default();
                    for (_, e) in w.entities.iter() {
                        *c.entry((e.obj_type as u8, e.kind, e.owner)).or_default() += 1;
                    }
                    let mut v: Vec<_> = c.into_iter().collect();
                    v.sort_by_key(|x| std::cmp::Reverse(x.1));
                    v.truncate(6);
                    (w.entities.iter().count(), v)
                });
                eprintln!("{} tick {t}: driver {:?}, sim {:?}, {entities} entities, most {by:?}", self.name, t1 - lap, t1.elapsed());
            }
        }
    }
}

/// The wall time a mission test may take: `S4_MISSION_WALL` seconds, else 3600 (the Dark Tribe
/// end games take 100 000..270 000 ticks; with eight at once 30 minutes were not enough).
fn wall() -> std::time::Duration {
    std::time::Duration::from_secs(std::env::var("S4_MISSION_WALL").ok().and_then(|v| v.parse().ok()).unwrap_or(3600))
}

/// A constant of the scripts' tables (`Goods.BOARD`, `Settlers.THIEF`, …).
fn k(group: &str, name: &str) -> i32 {
    s4_script::CONSTANTS.iter().find(|c| c.0 == group && c.1 == name).map(|c| c.2 as i32).unwrap_or_else(|| panic!("{group}.{name}"))
}

/// The Dark Tribe temples' auras (`CDarkTempleRole` 0x5458c0): centre `(x + 1, y + 2)` and
/// reach `n < 4 ? 2n + 16 : 24` rings with `n` the owner's finished mushroom farms (none: no
/// aura).
fn auras(w: &s4_sim::World) -> Vec<(i32, i32, i32)> {
    w.entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Building && e.kind == 50 && e.race == 3 && !e.is_dead())
        .filter_map(|(_, e)| {
            let n = w.statistic.players.get(e.owner as usize).map_or(0, |p| p.count.get(49).copied().unwrap_or(0));
            (n > 0).then(|| (e.x() + 1, e.y() + 2, if n < 4 { 2 * n + 16 } else { 24 }))
        })
        .collect()
}

fn in_aura(auras: &[(i32, i32, i32)], x: i32, y: i32) -> bool {
    auras.iter().any(|&(cx, cy, r)| s4_sim::spiral::grid_distance(x - cx, y - cy) <= r + 1)
}

/// Where reinforcements for a target at `(x, y)` appear: player 1's building nearest to it on the
/// same land, else a free node of that land at least `min` from it.
fn spawn_point(w: &s4_sim::World, x: i32, y: i32, min: i32) -> Option<(i32, i32)> {
    let (_, _, target) = land_near(w, x, y, 4)?;
    let near = w
        .entities
        .iter()
        .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == 1 && !e.is_dead())
        .filter(|(_, e)| land_near(w, e.x(), e.y(), 3).is_some_and(|l| l.2 == target))
        .map(|(_, e)| (s4_sim::spiral::grid_distance(e.x() - x, e.y() - y), e.x(), e.y()))
        .min();
    match near {
        Some((_, bx, by)) => Some((bx, by)),
        // A small piece of land (a sector is also split by owners): nearer then.
        None => [min, min / 2, 4].into_iter().find_map(|m| {
            s4_sim::spiral::walk(w.grid, x, y, (min + 40).clamp(0, 255) as u8)
                .filter(|&(px, py)| s4_sim::spiral::grid_distance(px - x, py - y) >= m)
                .find_map(|(px, py)| land_near(w, px, py, 0).filter(|l| (l.0, l.1) == (px, py) && l.2 == target))
                .map(|(px, py, _)| (px, py))
        }),
    }
}

/// The first free node (no map object, none of node flags 0x39) at least `min` from `(x, y)`
/// (spiral order, radius 20) and its sector.
fn land_near(w: &s4_sim::World, x: i32, y: i32, min: i32) -> Option<(i32, i32, u32)> {
    s4_sim::spiral::walk(w.grid, x, y, 20).filter(|&(px, py)| s4_sim::spiral::grid_distance(px - x, py - y) >= min).find_map(|(px, py)| {
        let n = w.grid.checked_index(px, py)?;
        (w.layers.map_object[n] == 0 && w.layers.flags[n] & 0x39 == 0 && w.layers.ground[n] > 8).then(|| (px, py, w.tiling.sector_of(n) as u32))
    })
}

/// The base campaigns: no script victory check, the default one.
fn conquered(map: &str, ticks: u32) {
    let Some(m) = Mission::start(map) else { return };
    let o = m.conquer(ticks, 200, 30, 30);
    assert_eq!(o, crate::script::WON, "{map}");
}

/// A mission played by `driver` to Won.
fn won(map: &str, driver: impl FnOnce(&Mission) -> u8) {
    let Some(m) = Mission::start(map) else { return };
    assert_eq!(driver(&m), crate::script::WON, "{map}");
}

/// The settlers a script counts, by name.
fn settler(name: &str) -> u16 {
    k("Settlers", name) as u16
}

fn good(name: &str) -> u8 {
    k("Goods", name) as u8
}

/// AO_roman1: player 2's castles at (280, 154) and (215, 159) gone and none of its war ships
/// left (`Vehicles.Amount(2, 1) == 0`), then `EnemyPlayersLost(1)`.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_roman1_played_to_won() {
    won("AO_roman1", |m| m.conquer_and(400_000, 200, 30, 30, &|m| m.sink(2)));
}

/// AO_roman3: 20 swordsmen 3, 20 bowmen 3, 20 thieves and a squad leader within 20 of
/// (438, 555), and 300 boards (`Goods.Amount`).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_roman3_played_to_won() {
    won("AO_roman3", |m| {
        m.play(400_000, 200, &|m| {
            for (kind, n) in [(SWORDSMAN_3, 24), (BOWMAN_3, 24), (settler("THIEF"), 24), (settler("SQUADLEADER"), 2)] {
                m.gather(kind, n, 438, 555);
            }
            let (hx, hy) = m.home();
            m.stock(good("BOARD"), 300, hx, hy);
        })
    });
}

/// AO_settle1: goods (swords 200, bread 150, meat 160, fish 255, bows 220, gold bars 250), part
/// of them within 73 of (395, 246); then `DefaultPlayersLostCheck`: players 3 and 4 conquered.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_settle1_played_to_won() {
    won("AO_settle1", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            for (g, n) in [("SWORD", 200), ("BREAD", 150), ("MEAT", 160), ("FISH", 255), ("BOW", 220), ("GOLDBAR", 250)] {
                m.area_stock(good(g), n, 395, 246, 73);
            }
        })
    });
}

/// AO_settle2 (player 1 alone): a READY big temple within 32 of each of four places. Each
/// place is first taken with a castle (`Buildings.AddBuildingEx` wants free land for it).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_settle2_played_to_won() {
    let temple = k("Buildings", "BIGTEMPLE");
    let castle = k("Buildings", "CASTLE") as u8;
    won("AO_settle2", |m| {
        m.play(400_000, 200, &|m| {
            for (x, y) in [(787, 284), (857, 782), (648, 754), (159, 236)] {
                let (done, own) = m.ask(move |w| (w.exists_building_in_area(1, temple, x, y, 32, 1), w.grid.checked_index(x, y).is_some_and(|i| w.owner_of(i) == 1)));
                if done {
                    continue;
                }
                if !own {
                    m.place(x, y, castle);
                } else {
                    m.place(x, y, temple as u8);
                }
            }
        })
    });
}

/// AO_settle3 (players 1 and 2 one team): no dark land within 24 of (443, 263).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_settle3_played_to_won() {
    won("AO_settle3", |m| {
        m.play(400_000, 200, &|m| {
            m.green(443, 263, 24, 120);
        })
    });
}

/// AO_settle4 (player 1 alone): 250 trees within 40 of each of five places.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_settle4_played_to_won() {
    won("AO_settle4", |m| {
        m.play(400_000, 200, &|m| {
            for (x, y) in [(155, 570), (438, 611), (591, 552), (681, 526), (817, 496)] {
                m.forest(x, y, 40, 260);
            }
        })
    });
}

/// AO_trojan01 (player 1 alone): ferries within 30 of (480, 113) carrying 10 civilians, 30
/// soldiers of level 3 and 3 squad leaders (`Vehicles.GetFerryCargoInArea`), and 5 war ships
/// there. The ferries are loaded at home (`Settlers.AddSettlersToFerry`).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan01_played_to_won() {
    let loaded = std::cell::Cell::new(false);
    won("AO_trojan01", |m| {
        m.play(400_000, 200, &|m| {
            let (hx, hy) = m.home();
            let Some((tx, ty)) = m.water_near(480, 113) else { return };
            let ferries = m.fleet(2, 5, (hx, hy), (tx, ty));
            if !loaded.get() && ferries.len() >= 5 {
                let (thief, squad) = (settler("THIEF") as i32, settler("SQUADLEADER") as i32);
                let f = ferries.clone();
                m.ask(move |w| {
                    use s4_sim::vehicles::scripted::add_settlers_to_ferry as load;
                    load(w, f[0] as i32, thief, 12);
                    load(w, f[1] as i32, squad, 4);
                    for &id in &f[1..] {
                        load(w, id as i32, SWORDSMAN_3 as i32, 12);
                    }
                });
                loaded.set(true);
            }
            m.order(ferries, tx, ty);
            let ships = m.fleet(1, 6, (hx, hy), (tx, ty));
            m.order(ships, tx, ty);
            if m.tick() % 50_000 < 200 {
                let cargo = m.ask(|w| s4_sim::vehicles::scripted::ferry_cargo_in_area(w, 1, 480, 113, 30));
                eprintln!("AO_trojan01 tick {}: home ({hx}, {hy}), to ({tx}, {ty}); ferries {:?}, ships {:?}, cargo {cargo:?}", m.tick(), m.vehicles_of(1, 2), m.vehicles_of(1, 1));
            }
        })
    });
}

/// AO_trojan03: no dark land within 25 of (352, 359), a priest, 5 swordsmen 3, 5 bowmen 3 and a
/// squad leader within 32 of it and no unit of player 5 within 35 (`Game.FindAnyUnit`).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan03_played_to_won() {
    won("AO_trojan03", |m| {
        m.play(400_000, 200, &|m| {
            m.green(352, 359, 25, 120);
            for (kind, n) in [(SWORDSMAN_3, 30), (BOWMAN_3, 30), (settler("PRIEST"), 2), (settler("SQUADLEADER"), 2)] {
                m.gather(kind, n, 352, 359);
            }
        })
    });
}

/// AO_trojan04: player 1's READY castle, big temple and eye catchers 1..=12 anywhere (within 300
/// of (562, 545)), built at home.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan04_played_to_won() {
    let first = k("Buildings", "EYECATCHER01");
    let kinds: Vec<i32> = [k("Buildings", "CASTLE"), k("Buildings", "BIGTEMPLE")].into_iter().chain(first..first + 12).collect();
    won("AO_trojan04", |m| {
        m.play(400_000, 200, &|m| {
            // The circle (300 rings from (562, 545)) does not reach the player's start: land
            // there first.
            if !m.claim(562, 545, 12) {
                return;
            }
            let mut missing = Vec::new();
            for &b in &kinds {
                if !m.ask(move |w| w.exists_building_in_area(1, b, 562, 545, 300, 1)) {
                    missing.push((b, m.place(562, 545, b as u8)));
                }
            }
            if m.tick() % 20_000 < 200 {
                eprintln!("AO_trojan04 tick {}: missing (kind, placed id) {missing:?}", m.tick());
            }
        })
    });
}

/// AO_trojan07: the Dark Tribe's manakopter hall at (108, 502) gone. A Dark Tribe building
/// falls when the land under it turns light (0x4cbc15 → 0x53ce10): gardeners there.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan07_played_to_won() {
    let hall = k("Buildings", "MANACOPTERHALL");
    won("AO_trojan07", |m| {
        m.play(400_000, 200, &|m| {
            if m.ask(move |w| w.exists_building_in_area(2, hall, 108, 502, 10, 1)) {
                m.green(108, 502, 6, 120);
            }
        })
    });
}

/// AO_maya4: 150 gold bars of player 1 within 40 of (154, 591) and player 4 lost (conquered).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_maya4_played_to_won() {
    won("AO_maya4", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            // Nobody's land: a castle there once player 4 is beaten (earlier it takes the
            // reinforcements' place).
            if !m.enemy_military().iter().any(|e| e.1 == 4) && m.claim(154, 591, 4) {
                m.area_stock(good("GOLDBAR"), 150, 154, 591, 40);
            }
            if m.tick() % 20_000 < 200 {
                let (gold, owner) = m.ask(|w| (w.goods_amount_in_area(1, 14, 154, 591, 40), w.owner_of(w.grid.index(154, 591))));
                eprintln!("AO_maya4 tick {}: {gold} gold bars within 40 of (154, 591), owner {owner}; vars {}", m.tick(), m.vars());
            }
        })
    });
}

/// MD_settle1: any unit of player 1 within 32 of (88, 665) (`Game.FindAnyUnit`).
#[test]
#[ignore = "slow: a whole mission"]
fn md_settle1_played_to_won() {
    won("MD_settle1", |m| m.play(400_000, 200, &|m| m.gather(SWORDSMAN_3, 10, 88, 665)));
}

/// MD_settle2: player 1's land at (846, 636) connected by land with ally 2's at (253, 231)
/// (`Game.ArePlayerAreasConnected`): castles along the way.
#[test]
#[ignore = "slow: a whole mission"]
fn md_settle2_played_to_won() {
    won("MD_settle2", |m| {
        let a = m.spot(846, 636).map_or((846, 636), |s| (s.0, s.1));
        let b = m.spot(253, 231).map_or((253, 231), |s| (s.0, s.1));
        let route = m.land_route(a, b);
        eprintln!("MD_settle2: a land route of {} steps", route.len() * 12);
        m.play(400_000, 200, &|m| {
            let others = m.bridge(&route, &[1, 2]);
            if m.tick() % 50_000 < 200 {
                eprintln!("MD_settle2 tick {}: route nodes not owned by players 1, 2: {others:?}", m.tick());
            }
        })
    });
}

/// MD_conflict2: players 2, 3 and 4 lost (conquered) and the land within 20 of (641, 680) owned.
#[test]
#[ignore = "slow: a whole mission"]
fn md_conflict2_played_to_won() {
    won("MD_conflict2", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            m.claim(641, 680, 20);
        })
    });
}

/// MD_viking1: 4 READY carts, 4 ferries, 12 swordsmen 3, 16 bowmen 3 and 9 thieves.
#[test]
#[ignore = "slow: a whole mission"]
fn md_viking1_played_to_won() {
    won("MD_viking1", |m| {
        m.play(400_000, 200, &|m| {
            let (hx, hy) = m.home();
            for (kind, n) in [(SWORDSMAN_3, 14), (BOWMAN_3, 18), (settler("THIEF"), 11)] {
                m.gather(kind, n, hx, hy);
            }
            m.fleet(2, 4, (hx, hy), (hx, hy));
            m.wheels(5, 4, hx, hy);
            m.census(&[31, 34, 48]);
        })
    });
}

/// MD_viking3: a READY port within 50 of (469, 101) and player 2 lost (conquered).
#[test]
#[ignore = "slow: a whole mission"]
fn md_viking3_played_to_won() {
    let port = k("Buildings", "PORT");
    won("MD_viking3", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            if !m.ask(move |w| w.exists_building_in_area(1, port, 469, 101, 50, 1)) && m.claim(469, 101, 12) {
                m.place(469, 101, port as u8);
            }
        })
    });
}

/// MD_viking5: a READY castle of player 1 within 50 of (332, 358).
#[test]
#[ignore = "slow: a whole mission"]
fn md_viking5_played_to_won() {
    won("MD_viking5", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            m.claim(332, 358, 6);
        })
    });
}

/// MD_settle3: battle axes 100, bread 150, meat 160, fish 255, mead 220, gold bars 55 and 600
/// mana before 150 minutes.
#[test]
#[ignore = "slow: a whole mission"]
fn md_settle3_played_to_won() {
    won("MD_settle3", |m| {
        m.play(400_000, 200, &|m| {
            let (hx, hy) = m.home();
            for (g, n) in [("BATTLEAXE", 100), ("BREAD", 150), ("MEAT", 160), ("FISH", 255), ("MEAD", 220), ("GOLDBAR", 55)] {
                m.stock(good(g), n + 5, hx, hy);
            }
            // As `Magic.IncreaseMana` (0x5c9370).
            m.ask(|w| w.magic.players[1].mana = w.magic.players[1].mana.max(620));
        })
    });
}

/// MD_roman2: the land within 55 of (105, 130) owned, with a READY woodcutter, stone cutter,
/// sawmill and military building within 74; a minute later the enemies are declared lost.
#[test]
#[ignore = "slow: a whole mission"]
fn md_roman2_played_to_won() {
    let kinds: Vec<i32> = ["WOODCUTTERHUT", "STONECUTTERHUT", "SAWMILL"].iter().map(|n| k("Buildings", n)).collect();
    won("MD_roman2", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            if m.claim(105, 130, 55) {
                for &b in &kinds {
                    if !m.ask(move |w| w.exists_building_in_area(1, b, 105, 130, 74, 1)) {
                        m.place(105, 130, b as u8);
                    }
                }
            }
        })
    });
}

/// AO_trojan09: a priest and 3 squad leaders within 15 of each of (511, 519), (128, 456) and
/// (93, 92).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan09_played_to_won() {
    let places = [(511, 519), (128, 456), (93, 92)];
    won("AO_trojan09", |m| {
        m.play(400_000, 200, &|m| {
            m.squads(settler("PRIEST"), 1, &places);
            m.squads(settler("SQUADLEADER"), 3, &places);
        })
    });
}

/// AO_trojan10: a priest within 15 of (316, 212) with 200 mana starts the script's scene
/// (`Vars.Save5`); 125 ticks later `Vars.Save7` = 1 wins.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan10_played_to_won() {
    won("AO_trojan10", |m| {
        m.play(400_000, 200, &|m| {
            m.gather(settler("PRIEST"), 1, 316, 212);
            m.ask(|w| w.magic.players[1].mana = w.magic.players[1].mana.max(220));
        })
    });
}

/// AO_trojan11: 50 swordsmen 3, 50 bowmen 3, 50 of type 63 and 10 war machines within 40 of
/// (146, 109).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_trojan11_played_to_won() {
    won("AO_trojan11", |m| {
        m.play(400_000, 200, &|m| {
            for kind in [SWORDSMAN_3, BOWMAN_3, 63] {
                m.gather(kind, 55, 146, 109);
            }
            m.wheels(4, 11, 146, 109);
        })
    });
}

/// AO_Viking2: a priest and 4 squad leaders within 5 of (87, 281).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_viking2_played_to_won() {
    won("AO_Viking2", |m| {
        m.play(400_000, 200, &|m| {
            m.gather(settler("PRIEST"), 1, 87, 281);
            m.gather(settler("SQUADLEADER"), 4, 87, 281);
        })
    });
}

/// AO_Viking4: player 2 lost (conquered), at most 40 cells with 10 dark nodes, and 30 swordsmen
/// 3, 30 bowmen 3 and 10 war machines within 30 of (83, 183).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_viking4_played_to_won() {
    won("AO_Viking4", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            if m.enemy_military().is_empty() {
                m.dark_cells(10, 120);
                for kind in [SWORDSMAN_3, BOWMAN_3] {
                    m.gather(kind, 34, 83, 183);
                }
                m.wheels(4, 11, 83, 183);
            }
        })
    });
}

/// MCD2_roman1: 4 ferries, 3 war ships and 60 soldiers aboard ferries within 64 of (295, 64).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_roman1_played_to_won() {
    let loaded = std::cell::Cell::new(false);
    won("MCD2_roman1", |m| {
        m.play(400_000, 200, &|m| {
            let one: &[(u16, i32)] = &[(SWORDSMAN_3, 16)];
            m.ferry_out(&[one; 5], &loaded, (295, 64));
            m.fleet(1, 3, m.home(), (295, 64));
        })
    });
}

/// MCD2_roman3: a unit within 15 of (260, 279) opens the storage task (`Vars.Save2`); 128 gold
/// bars in the two storage areas (`Vars.Save1`); then 100 water, 4 ferries and 3 war ships
/// (water before that is taken away by the script's `waterCheck`).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_roman3_played_to_won() {
    won("MCD2_roman3", |m| {
        m.play(400_000, 200, &|m| {
            let v = m.var_list();
            if v.get(1) != Some(&1) {
                m.gather(SWORDSMAN_3, 5, 260, 279);
            } else if v.first() != Some(&1) {
                for (x, y) in [(278, 246), (263, 230)] {
                    if m.claim(x, y, 3) {
                        m.area_stock(good("GOLDBAR"), 70, x, y, 10);
                    }
                    if m.tick() % 50_000 < 200 {
                        let (gold, owner) = m.ask(move |w| (w.goods_amount_in_area(1, 14, x, y, 10), w.owner_of(w.grid.index(x as u32, y as u32))));
                        eprintln!("MCD2_roman3 tick {}: ({x}, {y}) owner {owner}, {gold} gold bars within 10", m.tick());
                    }
                }
            } else {
                let home = m.home();
                m.stock(good("WATER"), 110, home.0, home.1);
                m.fleet(2, 4, home, home);
                m.fleet(1, 3, home, home);
            }
        })
    });
}

/// MCD2_trojan2: 3 ferries, 3 war ships, 3 carts, 10 each of swordsmen 3, bowmen 3 and
/// backpack catapultists 3.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_trojan2_played_to_won() {
    won("MCD2_trojan2", |m| {
        m.play(400_000, 200, &|m| {
            let home = m.home();
            for kind in [SWORDSMAN_3, BOWMAN_3, settler("BACKPACKCATAPULTIST_03")] {
                m.gather(kind, 12, home.0, home.1);
            }
            m.fleet(2, 3, home, home);
            m.fleet(1, 3, home, home);
            m.wheels(5, 3, home.0, home.1);
            m.census(&[31, 34, 63]);
        })
    });
}

/// MCD2_viking2: 50 each of meat, bread, fish and water.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_viking2_played_to_won() {
    won("MCD2_viking2", |m| {
        m.play(400_000, 200, &|m| {
            let (hx, hy) = m.home();
            for g in ["MEAT", "BREAD", "FISH", "WATER"] {
                m.stock(good(g), 55, hx, hy);
            }
        })
    });
}

/// MCD2_viking3: ferries within 25 of (13, 225) carrying 45 civilians and 15 soldiers of level 3,
/// and 15 each of pioneers, geologists, gardeners and swordsmen 3 (`Settlers.Amount`).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_viking3_played_to_won() {
    let loaded = std::cell::Cell::new(false);
    won("MCD2_viking3", |m| {
        m.play(400_000, 200, &|m| {
            let (p, g) = (settler("PIONEER"), settler("GEOLOGIST"));
            let cargo: [&[(u16, i32)]; 8] = [&[(p, 8)], &[(p, 8)], &[(g, 8)], &[(g, 8)], &[(GARDENER, 8)], &[(GARDENER, 8)], &[(SWORDSMAN_3, 8)], &[(SWORDSMAN_3, 8)]];
            m.ferry_out(&cargo, &loaded, (13, 225));
        })
    });
}

/// MCD2_maya3: a priest within 15 of each wonder, (255, 255) and (408, 450), with 250 mana
/// each time (`Vars.Save2` / `Save3` count to 30).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_maya3_played_to_won() {
    won("MCD2_maya3", |m| {
        m.play(400_000, 200, &|m| {
            m.squads(settler("PRIEST"), 1, &[(255, 255), (408, 450)]);
            m.ask(|w| w.magic.players[1].mana = w.magic.players[1].mana.max(600));
        })
    });
}

/// MCD2_maya4: a ferry within 20 of (24, 870) carrying 2 civilians, 10 soldiers of level 3 and 3
/// squad leaders, with 2 priests alive.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_maya4_played_to_won() {
    let loaded = std::cell::Cell::new(false);
    won("MCD2_maya4", |m| {
        m.play(400_000, 200, &|m| {
            let stage = m.var_list().first().copied().unwrap_or(-1);
            if m.tick() % 50_000 < 200 {
                eprintln!("MCD2_maya4 tick {}: stage {stage}, ferries {:?}", m.tick(), m.vehicles_of(1, 2));
            }
            match stage {
                // A ferry of the player at the war ships of Gibraltar (25 of (342, 432),
                // `warShipCheck`) makes the script send player 2's ferry to (96, 156).
                0 => {
                    let Some(to) = m.water_near(342, 432) else { return };
                    let ferries = m.fleet(2, 1, m.home(), to);
                    m.order(ferries, to.0, to.1);
                }
                // There it becomes the player's (`ferryCheck`): it takes the units of the
                // Canary Islands (here loaded by `Settlers.AddSettlersToFerry`) to the exit.
                2 => {
                    let Some(&(id, _, _)) = m.vehicles_of(1, 2).iter().min_by_key(|v| s4_sim::spiral::grid_distance(v.1 - 96, v.2 - 156)) else { return };
                    if !loaded.get() {
                        let (thief, squad) = (settler("THIEF") as i32, settler("SQUADLEADER") as i32);
                        // 15 aboard at most (+0xb4).
                        m.ask(move |w| {
                            use s4_sim::vehicles::scripted::add_settlers_to_ferry as load;
                            load(w, id as i32, squad, 3);
                            load(w, id as i32, thief, 2);
                            load(w, id as i32, SWORDSMAN_3 as i32, 10);
                        });
                        loaded.set(true);
                    }
                    if let Some(to) = m.water_near(24, 870) {
                        m.order(vec![id], to.0, to.1);
                    }
                }
                _ => {}
            }
        })
    });
}

/// MCD2_trojan1: 5 thieves, a cart and 10 soldiers brought to the harbour (25 of (638, 647))
/// before the time limit (`harborCheck` counts and removes them, `Vars.Save4` = 1); then the
/// script's three ferries of player 2 sail to (756, 756) (`AI.NewSquad(2, CMD_MOVE_AND_STAY)`),
/// each arrival counting `Vars.Save4` up to 4.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_trojan1_played_to_won() {
    let thief = settler("THIEF");
    won("MCD2_trojan1", |m| {
        // The escape itself (the carts cross the enemies' land, Trigger1..3) is not played: the
        // thieves and a cart appear in the harbour; the soldiers are the player's own.
        let cart = m.ask(move |w| {
            s4_sim::create::add_settlers_around_ex(w, 638, 647, 1, thief, 6, 1);
            s4_sim::spiral::walk(w.grid, 638, 647, 24).find_map(|(x, y)| {
                let id = s4_sim::vehicles::scripted::add_vehicle(w, x, y, 1, 5, -1, 0, 0);
                (id != 0).then_some((id, x, y))
            })
        });
        eprintln!("MCD2_trojan1: the cart {cart:?}");
        m.play(400_000, 200, &|m| {
            let v = m.var_list();
            if v.get(3).is_some_and(|&s| s == 0) {
                m.gather(SWORDSMAN_3, 11, 638, 647);
            }
            if m.tick() % 5_000 < 200 {
                eprintln!("MCD2_trojan1 tick {}: vars {v:?}; player 2's ferries {:?}", m.tick(), m.vehicles_of(2, 2));
            }
        })
    });
}

/// MCD2_maya1: 25 each of bread, fish, meat and tequila (`checkFood`, `Vars.Save3`) before
/// player 5's ferry, sent along its two routes by `AI.NewSquad(5, CMD_MOVE_AND_STAY)` (killed
/// and remade at every waypoint), reaches (615, 124).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_maya1_played_to_won() {
    won("MCD2_maya1", |m| {
        m.play(400_000, 200, &|m| {
            let (hx, hy) = m.home();
            for g in ["BREAD", "FISH", "MEAT", "TEQUILA"] {
                m.stock(good(g), 30, hx, hy);
            }
            m.defend();
            if std::env::var("S4_CHECK_SHIP_CELLS").is_ok() {
                // The ship cells against the ships: stale entries, ships missing.
                let (stale, missing) = m.ask(|w| {
                    let ships: Vec<(u16, i32)> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Ship && !e.is_dead() && e.pos.0 != 0 && e.pos.0 <= 0x7fff_ffff).map(|(i, e)| (i, e.pos.0 as i32)).collect();
                    let entries: Vec<i32> = w.vehicles.ship_cells.0.iter().flatten().copied().filter(|&p| p != 0).collect();
                    let xy = |p: i32| (p & 0xffff, p >> 16);
                    let stale: Vec<(i32, i32)> = entries.iter().filter(|&&p| !ships.iter().any(|s| s.1 == p)).map(|&p| xy(p)).collect();
                    let missing: Vec<(u16, (i32, i32))> = ships.iter().filter(|s| !entries.contains(&s.1)).map(|s| (s.0, xy(s.1))).collect();
                    (stale, missing)
                });
                if !stale.is_empty() || !missing.is_empty() {
                    eprintln!("MCD2_maya1 tick {}: ship cells: stale {stale:?}, ships not in them {missing:?}", m.tick());
                }
            }
            // Player 4's war machines on the coast sink the ferry on its last leg (41 800, near
            // (444, 325)): soldiers go for them.
            let machines: Vec<(u16, i32, i32)> = (2..=4).flat_map(|p| m.vehicles_of(p, 4)).collect();
            if let Some(&(_, x, y)) = machines.first() {
                m.gather(SWORDSMAN_3, 12, x, y);
            }
            if m.tick() % 10_000 < 200 {
                let mil = m.ask(|w| w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == 1 && MILITARY.contains(&e.kind)).count());
                if let Some(&(fid, _, _)) = m.vehicles_of(5, 2).first() {
                    let near = m.ask(move |w| {
                        let hp = s4_sim::vehicles::get(w, fid).map(|v| (v.health, v.task, v.listed, v.walking.as_ref().map(|s| (s.state, s.walk_to & 0xffff, s.walk_to >> 16, s.requested & 0xffff, s.requested >> 16))));
                        let foes: Vec<(u16, u8, u16, i32, i32)> = w.entities.iter().filter(|(i, e)| *i != fid && matches!(e.obj_type, ObjType::Ship) && s4_sim::spiral::grid_distance(e.x() - 415, e.y() - 240) <= 70).map(|(i, e)| (i, e.owner, e.kind, e.x(), e.y())).take(12).collect();
                        (hp, foes)
                    });
                    eprintln!("MCD2_maya1 tick {}: the ferry (hit points, task, list, walk), ships within 70 of (415, 240) {:?}, {:?}", m.tick(), near.0, near.1);
                    // Ship marks (water flag 0x800) without a ship near them.
                    let stray = m.ask(|w| {
                        let ships: Vec<(i32, i32)> = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Ship && !e.is_dead()).map(|(_, e)| (e.x(), e.y())).collect();
                        let size = w.grid.size() as i32;
                        (0..size * size)
                            .filter(|&n| w.tiling.water_flags.get(n as usize).is_some_and(|f| f & 0x800 != 0))
                            .map(|n| (n % size, n / size))
                            .filter(|&(x, y)| ships.iter().all(|&(sx, sy)| s4_sim::spiral::grid_distance(sx - x, sy - y) > 3))
                            .take(20)
                            .collect::<Vec<_>>()
                    });
                    eprintln!("MCD2_maya1 tick {}: ship marks with no ship near {stray:?}", m.tick());
                }
                eprintln!("MCD2_maya1 tick {}: vars {:?}, player 5's ferries {:?}, player 1's military {mil}", m.tick(), m.var_list(), m.vehicles_of(5, 2));
            }
        })
    });
}

/// MCD2_viking1: a ferry or war ship of the player within the goal circle at (596, 598).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_viking1_played_to_won() {
    won("MCD2_viking1", |m| {
        m.play(400_000, 200, &|m| {
            let Some(to) = m.water_near(596, 598) else { return };
            for t in [1, 2] {
                m.order(m.vehicles_of(1, t).into_iter().map(|v| v.0).collect(), to.0, to.1);
            }
            if m.tick() % 50_000 < 200 {
                eprintln!("MCD2_viking1 tick {}: ships {:?}, ferries {:?}", m.tick(), m.vehicles_of(1, 1), m.vehicles_of(1, 2));
            }
        })
    });
}

/// MCD2_viking4: 64 meat and 64 bread in the meat (or bread) storage area, 64 fish in the fish
/// one (15 around (604, 107), (144, 145), (360, 640)) before the time limit.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_viking4_played_to_won() {
    won("MCD2_viking4", |m| {
        m.play(400_000, 200, &|m| {
            for (g, x, y) in [("MEAT", 604, 107), ("BREAD", 604, 107), ("FISH", 144, 145)] {
                if m.claim(x, y, 3) {
                    m.area_stock(good(g), 70, x, y, 15);
                }
            }
        })
    });
}

/// MCD2_maya5: the castle at (333, 286) the player's (`Vars.Save1`), a READY small temple
/// within 10 of the healing well (473, 498) (`Save2`), and a priest at each wonder, (177, 239)
/// and (523, 227), with 150 mana (`Save3` / `Save4` = 2); at least 2 priests alive.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_maya5_played_to_won() {
    let temple = k("Buildings", "SMALLTEMPLE");
    won("MCD2_maya5", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            m.squads(settler("PRIEST"), 2, &[(177, 239), (523, 227)]);
            m.ask(|w| w.magic.players[1].mana = w.magic.players[1].mana.max(400));
            if !m.ask(move |w| w.exists_building_in_area(1, temple, 473, 498, 10, 1)) && m.claim(473, 498, 4) {
                m.place(473, 498, temple as u8);
            }
            if m.tick() % 50_000 < 200 {
                eprintln!("MCD2_maya5 tick {}: vars {:?}", m.tick(), m.var_list());
            }
        })
    });
}

/// MCD2_roman4: a thief at the start position (20 of (480, 262), `Vars.Save1`), none left
/// near the camps (`killThieves`): player 3's ferry brings the settlers (`Save1` = 2); a unit
/// at the ferries (30 of (82, 450) / (111, 466) / (143, 489)) gets them (`Save3`); then 30 each
/// of water, bread, fish and meat, 3 ferries within 50 of (520, 270) and nobody of the player
/// within 30 of (159, 76).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_roman4_played_to_won() {
    won("MCD2_roman4", |m| {
        m.play(400_000, 200, &|m| {
            let v = m.var_list();
            let (s1, s3) = (v.first().copied().unwrap_or(-1), v.get(2).copied().unwrap_or(-1));
            if s1 == 0 {
                // The sneak past player 2's soldiers is not played (a thief they come within 6
                // of is seen, 0x5801f0, and dies): one appears at the start position.
                let thief = settler("THIEF");
                if m.units(move |_, k| k == thief).iter().all(|u| s4_sim::spiral::grid_distance(u.1 - 480, u.2 - 262) > 20) {
                    m.ask(move |w| {
                        if let Some((x, y, _)) = land_near(w, 480, 262, 0) {
                            s4_sim::create::add_settlers_around_ex(w, x, y, 1, thief, 1, 1);
                        }
                    });
                }
            } else if s1 == 2 && s3 == 0 {
                m.gather(SWORDSMAN_3, 3, 111, 466);
            } else if s3 == 1 {
                // The player has no land yet: the dropped settlers' place (499, 268) taken.
                if m.claim(499, 268, 3) {
                    for g in ["WATER", "BREAD", "FISH", "MEAT"] {
                        m.stock(good(g), 35, 499, 268);
                    }
                }
                if let Some(to) = m.water_near(520, 270) {
                    m.order(m.vehicles_of(1, 2).into_iter().map(|v| v.0).collect(), to.0, to.1);
                }
            }
            if m.tick() % 50_000 < 200 || m.tick() < 1500 {
                let thieves = m.units(|_, k| k == 48);
                eprintln!("MCD2_roman4 tick {}: vars {v:?}, ferries {:?}, thieves {thieves:?}", m.tick(), m.vehicles_of(1, 2));
            }
        })
    });
}

/// MCD2_maya2: player 3's small guard towers down to none (`Vars.Save1` = 2, conquest); then a
/// unit within 7 of the priest at (241, 136) (`Save5`) and the dark land down to exactly 3
/// cells with 5 dark nodes (`darkLandCheck`, `Save2`).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_maya2_played_to_won() {
    won("MCD2_maya2", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            let v = m.var_list();
            if v.first() == Some(&2) {
                m.gather(SWORDSMAN_3, 3, 241, 136);
            }
            if v.get(4) == Some(&1) {
                let left = m.ask(|w| {
                    let side = (w.grid.size() >> 4) as i32;
                    (0..side * side).filter(|&c| w.animals.cells.get(c % side, c / side).dark() >= 5).count()
                });
                if left > 3 {
                    m.dark_cells(5, 120);
                }
            }
            if m.tick() % 50_000 < 200 {
                eprintln!("MCD2_maya2 tick {}: vars {v:?}", m.tick());
            }
        })
    });
}

/// MD_Bonus: three tasks count `Vars.Save3` to 3: at the pig circle (32 of (596, 132)) 200 meat
/// with a READY slaughterhouse and animal ranch there; the priest found (32 of (514, 453)) and
/// player 2's big temple at (315, 266) gone; player 3 lost (conquest).
#[test]
#[ignore = "slow: a whole mission"]
fn md_bonus_played_to_won() {
    let (slaughter, ranch) = (k("Buildings", "SLAUGHTERHOUSE"), k("Buildings", "ANIMALRANCH"));
    won("MD_Bonus", |m| {
        // The script's `Tick` runs `CreateRoman` (thousands of player 4's soldiers) on every tick
        // once the player has 15 swordsmen 3 and 15 bowmen 3: `A or B or C and Vars.Save8 ==
        // 151` binds as `A or B or (C and …)` in Lua. So no bowmen in the waves.
        m.bowmen.set(false);
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            // Not soldiers: the conquest sends those elsewhere.
            m.squads(settler("THIEF"), 2, &[(596, 132), (514, 453)]);
            if m.claim(596, 132, 6) {
                for b in [slaughter, ranch] {
                    if !m.ask(move |w| w.exists_building_in_area(1, b, 596, 132, 32, 1)) {
                        m.place(596, 132, b as u8);
                    }
                }
                let (hx, hy) = m.home();
                m.stock(good("MEAT"), 210, hx, hy);
            }
            if m.tick() % 50_000 < 200 {
                eprintln!("MD_Bonus tick {}: vars {:?}", m.tick(), m.var_list());
            }
        })
    });
}

/// MCD2_roman2: the Trojan and the Viking ferry at (612, 278) and (663, 319) shot below 37 hit
/// points (`Vehicles.GetHealth`) and then a thief within 15 of each (`Vars.Save3` / `Save4` =
/// 2); unseen by the towers and ships (`Save1` / `Save2` below 2), the ferries and war ships
/// kept, within 60 minutes. A war ship of the player shoots each ferry and turns away below 37.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_roman2_played_to_won() {
    won("MCD2_roman2", |m| {
        m.play(400_000, 100, &|m| {
            // The player's one tower (246, 725) is attacked early: lost, the player is too.
            m.defend();
            let v = m.var_list();
            for (i, (fx, fy)) in [(612, 278), (663, 319)].into_iter().enumerate() {
                let stage = v.get(2 + i).copied().unwrap_or(-1);
                // Made next to the ferry: the way from home passes the patrols.
                let ships = m.fleet(1, 1, (fx, fy), (fx, fy));
                if stage == 0 {
                    m.order(ships, fx, fy);
                    break;
                }
                if stage == 1 {
                    let (hx, hy) = m.home();
                    if let Some(w) = m.water_near(hx, hy) {
                        m.order(ships, w.0, w.1);
                    }
                    m.gather(settler("THIEF"), 1, fx, fy);
                    break;
                }
            }
            if m.tick() % 1_000 < 100 {
                let hp = m.ask(|w| [(612, 278), (663, 319)].map(|(x, y)| s4_sim::vehicles::scripted::health_at(w, x, y)));
                let (mil, lost) = m.ask(|w| (w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == 1 && MILITARY.contains(&e.kind)).map(|(i, e)| (i, e.x(), e.y())).collect::<Vec<_>>(), w.default_player_lost(1)));
                eprintln!("MCD2_roman2 tick {}: vars {v:?}, ferry hit points {hp:?}, warships 4 {:?} 5 {:?}, player 1's military {mil:?} (default lost {lost})", m.tick(), m.vehicles_of(4, 1), m.vehicles_of(5, 1));
            }
            if m.tick() > 6_000 && m.tick() < 7_500 {
                let checks = m.eval("Game.Time() .. ' ' .. Vehicles.Amount(4,Vehicles.FERRY) .. ' ' .. Vehicles.Amount(5,Vehicles.FERRY) .. ' ' .. Vehicles.Amount(4,Vehicles.WARSHIP) .. ' ' .. Vehicles.Amount(5,Vehicles.WARSHIP) .. ' ' .. Game.HasPlayerLost(1)");
                eprintln!("MCD2_roman2 tick {}: time, ferries 4 / 5, war ships 4 / 5, lost: {checks}", m.tick());
            }
        })
    });
}

/// AO_Viking3: hold out until `gTimeOut` (120 minutes) with the allies 2 and 3 standing and
/// each above 8 carriers (else `EnemyPlayersLost(5)`); the conquest goes on, the allies' towers
/// are defended ([`Mission::defend`]).
#[test]
#[ignore = "slow: a whole mission"]
fn ao_viking3_played_to_won() {
    won("AO_Viking3", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            if m.tick() % 10_000 < 200 {
                let state = m.ask(|w| {
                    [2u8, 3].map(|p| {
                        let carriers = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == p && e.kind == 1 && !e.is_dead()).count();
                        let military = w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == p && MILITARY.contains(&e.kind)).count();
                        (p, carriers, military)
                    })
                });
                eprintln!("AO_Viking3 tick {}: allies (player, carriers, military) {state:?}", m.tick());
            }
        })
    });
}

/// AO_maya2: no dark land within 25 of (187, 270) and within 8 of (170, 288) (the Dark Tribe's
/// land around its temple); the conquest goes on.
#[test]
#[ignore = "slow: a whole mission"]
fn ao_maya2_played_to_won() {
    won("AO_maya2", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            if !m.green(170, 288, 8, 60) {
                return;
            }
            m.green(187, 270, 25, 150);
        })
    });
}

/// MCD2_roman5: the towers around the two wonders gone (`Vars.Save1` / `Save2` count to 30) and
/// player 2's castle at (333, 286) gone. The wonders' circles (22 around (179, 241) and
/// (525, 232)) kill the player's settlers and war machines inside now and then (`kill1` /
/// `kill2`): war machines just outside them shoot the towers (the catapult's own search,
/// 0x5999c0); the conquest does the rest.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_roman5_played_to_won() {
    type Wonder = ((i32, i32), [(i32, i32); 4]);
    const WONDERS: [Wonder; 2] = [((179, 241), [(165, 230), (182, 230), (177, 254), (194, 254)]), ((525, 232), [(510, 218), (527, 218), (539, 242), (522, 242)])];
    won("MCD2_roman5", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            for ((cx, cy), towers) in WONDERS {
                for (tx, ty) in towers {
                    let standing = m.enemy_military().iter().any(|e| s4_sim::spiral::grid_distance(e.2 - tx, e.3 - ty) <= 3);
                    if !standing {
                        continue;
                    }
                    // Two machines on the ray from the centre through the tower, 25 out.
                    let (dx, dy) = (tx - cx, ty - cy);
                    let d = s4_sim::spiral::grid_distance(dx, dy).max(1);
                    let (px, py) = (cx + dx * 25 / d, cy + dy * 25 / d);
                    let near = m.vehicles_of(1, 4).iter().filter(|v| s4_sim::spiral::grid_distance(v.1 - px, v.2 - py) <= 6).count();
                    if near < 2 {
                        m.ask(move |w| {
                            for (x, y) in s4_sim::spiral::walk(w.grid, px, py, 5) {
                                if s4_sim::spiral::grid_distance(x - cx, y - cy) > 23 && s4_sim::vehicles::scripted::add_vehicle(w, x, y, 1, 4, -1, 100, 0) != 0 {
                                    break;
                                }
                            }
                        });
                    }
                }
            }
            if m.tick() % 20_000 < 200 {
                eprintln!("MCD2_roman5 tick {}: vars {:?}, war machines {:?}", m.tick(), m.var_list(), m.vehicles_of(1, 4));
            }
        })
    });
}

/// MCD2_trojan5: a squad leader at the entrance (20 of (473, 201), `Vars.Save1` = 2); 10
/// soldiers at each switch, 25 of (356, 159) and (595, 278) (`Save1` = 3); a unit near the
/// eye-catcher field (25 of (379, 445), `Save2` = 1); an eye catcher at each of the twelve
/// places (`Save2` = 2); a squad leader within 6 of (223, 436) (`Save2` = 3) wins.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_trojan5_played_to_won() {
    const EYES: [(i32, i32); 12] = [(385, 438), (395, 458), (362, 438), (378, 467), (340, 431), (360, 471), (332, 474), (311, 421), (304, 471), (279, 414), (268, 464), (250, 409)];
    let first = k("Buildings", "EYECATCHER01");
    won("MCD2_trojan5", |m| {
        m.play(400_000, 200, &|m| {
            let v = m.var_list();
            let (s1, s2) = (v.first().copied().unwrap_or(-1), v.get(1).copied().unwrap_or(-1));
            let squad = settler("SQUADLEADER");
            if s1 <= 1 {
                m.gather(squad, 1, 473, 201);
            } else if s1 == 2 {
                m.squads(SWORDSMAN_3, 12, &[(356, 159), (595, 278)]);
            } else if s2 == 0 {
                m.gather(SWORDSMAN_3, 3, 379, 445);
            } else if s2 == 1 {
                for (i, &(x, y)) in EYES.iter().enumerate() {
                    let kind = first + i as i32;
                    let done = m.ask(move |w| (first..first + 12).any(|t| w.exists_building_in_area(1, t, x, y, 8, 2)));
                    if !done && m.claim(x, y, 3) {
                        m.place(x, y, kind as u8);
                    }
                }
            } else if s2 == 2 {
                m.gather(squad, 1, 223, 436);
            }
            if m.tick() % 20_000 < 200 {
                eprintln!("MCD2_trojan5 tick {}: vars {v:?}", m.tick());
            }
        })
    });
}

/// XMD3_viking1: 200 bread and 200 meat and the land within 10 of (149, 412) (player 3's)
/// owned. The player starts without land: a castle at home first (from its first READY
/// military building on the default lost check runs, `Vars.Save8`), the goods there; the
/// conquest takes the rest.
#[test]
#[ignore = "slow: a whole mission"]
fn xmd3_viking1_played_to_won() {
    won("XMD3_viking1", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            let (hx, hy) = m.home();
            if m.claim(hx, hy, 4) {
                for g in ["BREAD", "MEAT"] {
                    m.stock(good(g), 210, hx, hy);
                }
            }
            m.claim(149, 412, 10);
            if m.tick() % 20_000 < 200 {
                let owners = m.ask(|w| s4_sim::spiral::walk(w.grid, 149, 412, 10).map(|(x, y)| w.owner_of(w.grid.index(x as u32, y as u32))).fold([0usize; 9], |mut c, o| {
                    c[o as usize % 9] += 1;
                    c
                }));
                eprintln!("XMD3_viking1 tick {}: bread meat owned lost {}, land owners near (149, 412) {owners:?}", m.tick(), m.eval("Goods.Amount(1,Goods.BREAD) .. ' ' .. Goods.Amount(1,Goods.MEAT) .. ' ' .. Game.IsAreaOwned(1,149,412,10) .. ' ' .. Game.HasPlayerLost(1)"));
            }
        })
    });
}

/// MCD2_trojan3: 150 stones (`Goods.Amount`) before the time limit (90 minutes), the player not
/// lost.
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_trojan3_played_to_won() {
    won("MCD2_trojan3", |m| {
        m.play(400_000, 200, &|m| {
            m.defend();
            let (hx, hy) = m.home();
            m.stock(good("STONE"), 160, hx, hy);
        })
    });
}

/// MCD2_trojan4: a priest within 7 of (253, 209).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_trojan4_played_to_won() {
    won("MCD2_trojan4", |m| m.play(400_000, 200, &|m| m.gather(settler("PRIEST"), 1, 253, 209)));
}

/// MCD2_viking5: any unit of player 1 within 8 of (331, 325).
#[test]
#[ignore = "slow: a whole mission"]
fn mcd2_viking5_played_to_won() {
    won("MCD2_viking5", |m| {
        m.conquer_and(400_000, 200, 30, 30, &|m| {
            // Player 1's units within 30 of (331, 325) die until more than 5 of the 8 big
            // temples are gone (`deathZone`, `Vars.Save1`).
            if m.var_list().first().is_some_and(|&v| v > 5) {
                m.gather(SWORDSMAN_3, 10, 331, 325);
            }
        })
    });
}

/// dark08: a priest within 60 of (594, 155) wins (`Settlers.AmountInArea(1, PRIEST, 594, 155,
/// 60)`). The shortcut: the priest appears on the circle's land as the other drivers' units do
/// ([`Mission::muster`]). The player's way is played by [`dark08_by_sea`].
#[test]
#[ignore = "slow: a whole mission"]
fn dark08_played_to_won() {
    won("dark08", |m| m.play(200_000, 200, &|m| m.gather(settler("PRIEST"), 1, 594, 155)));
}

/// dark08 by sea, the player's way: no way of any ground leads from the start to the circle, but
/// the home land has a second coast in the east (near (455, 571)) on the sea (ship sector 1) that
/// also washes the north-west coast of the land around the circle. A ferry of player 1 (made on
/// that sea within 9 of the home land, as a shipyard there would launch it) takes a priest to
/// (498, 239) and unloads him (5009). He lands on land 2, which reaches within 60 of the circle
/// by itself; the circle's centre lies on land 18 behind a ridge of 0x80 / 0x81 ground, which the
/// terrain spell (3, kind 0: 0x80 / 0x81 / 0x23 → 0x20) melts (the script's "PRIEST MUST GO TO
/// THE NORTH AND SMELT THE SNOW"; [`dark08_melt_joins_the_circle`]). Won at tick 5 401 (about 350 s of wall time:
/// some 65 ms a tick on this map).
#[test]
#[ignore = "slow: a whole mission"]
fn dark08_by_sea() {
    won("dark08", |m| {
        let home = m.home();
        let route = m.ask(move |w| sea_route(w, home, (594, 155), 60));
        eprintln!("dark08: home {home:?}, sea route (launch, landing) {route:?}");
        let Some((launch, (lx, ly))) = route else { panic!("dark08: no sea route") };
        let priest = settler("PRIEST");
        let ferry = std::cell::Cell::new(0u16);
        let unloaded = std::cell::Cell::new(false);
        m.play(200_000, 200, &|m| {
            if !unloaded.get() {
                if ferry.get() == 0 {
                    let f = m.vehicle(launch.0, launch.1, 2);
                    assert!(f != 0, "dark08: no ferry at {launch:?}");
                    m.ask(move |w| s4_sim::vehicles::scripted::add_settlers_to_ferry(w, f as i32, priest as i32, 1));
                    ferry.set(f);
                }
                let f = ferry.get();
                match m.ask(move |w| w.entities.by_index(f as u32).map(|e| (e.x(), e.y()))) {
                    Some((x, y)) if s4_sim::spiral::grid_distance(x - lx, y - ly) <= 3 => {
                        m.command(s4_sim::Command::VehicleUnload { vehicle: f as u32 });
                        unloaded.set(true);
                        eprintln!("dark08 tick {}: the ferry at ({x}, {y}) unloads", m.tick());
                    }
                    Some(_) => m.order(vec![f], lx, ly),
                    None => panic!("dark08: the ferry is gone (tick {})", m.tick()),
                }
                return;
            }
            // The priest's land (a tiling sector) and its node nearest to the circle.
            let goal = m.ask(move |w| {
                let (id, x, y) = w.entities.iter().find(|(_, e)| e.owner == 1 && e.kind == priest && e.obj_type == ObjType::Settler && !e.is_dead() && e.flags & s4_sim::entity::flag::ATTACHED == 0).map(|(id, e)| (id, e.x(), e.y()))?;
                let land = w.tiling.sector_of(w.grid.checked_index(x, y)?);
                let to = s4_sim::spiral::walk(w.grid, 594, 155, 60).find(|&(a, b)| w.grid.checked_index(a, b).is_some_and(|i| w.tiling.sector_of(i) == land))?;
                Some((id, land, to))
            });
            if m.tick() % 10_000 < 200 {
                eprintln!("dark08 tick {}: the priest, his land, its node nearest to the circle {goal:?}", m.tick());
            }
            if let Some((id, _, (x, y))) = goal {
                m.order(vec![id], x, y);
            }
        })
    });
}

/// dark08: the terrain spell (3) of player 1 (Roman), cast on the ridge of 0x80 / 0x81 ground at
/// (654, 191) between land 2 (the sea landing's) and land 18 (the circle's centre), melts it to
/// rock (kind 0, 0x4f39d0: the cast counts as failed, no mana, but its rings run) and the two
/// lands become one.
#[test]
fn dark08_melt_joins_the_circle() {
    let Some(m) = Mission::start("dark08") else { return };
    let land = |m: &Mission, x: i32, y: i32| m.ask(move |w| w.tiling.sector_of(w.grid.index(x as u32, y as u32)));
    assert_ne!(land(&m, 658, 191), land(&m, 649, 191), "two lands before");
    assert_eq!(m.ask(|w| w.layers.ground[w.grid.index(654, 191)]), 0x80);
    assert_eq!(m.ask(|w| s4_sim::magic::cast_spell(w, 1, 3, 654, 191, 1, 0)), 0, "kind 0");
    m.sim.step(300);
    assert_eq!(m.ask(|w| w.layers.ground[w.grid.index(654, 191)]), 0x20, "melted to rock");
    assert_eq!(land(&m, 658, 191), land(&m, 649, 191), "one land after");
}

/// A sea route from the home land to the land that a walk over open land leads from to within
/// `r` of `target`: (where a ship can be made within 9 of the home land, where it lands within 9
/// of the target's land: the ferry unloads with land within 10), both on one ship sector, the
/// landing nearest to `target`.
fn sea_route(w: &s4_sim::World, home: (i32, i32), target: (i32, i32), r: u8) -> Option<((i32, i32), (i32, i32))> {
    use s4_sim::vehicles::ship;
    let size = w.grid.size() as i32;
    let open = |i: usize| w.layers.flags[i] & s4_sim::grid::flag::BLOCKED_LAND == 0;
    let fill = |seeds: Vec<(i32, i32)>| {
        let mut seen = vec![false; (size * size) as usize];
        let mut q = std::collections::VecDeque::new();
        for (a, b) in seeds {
            if let Some(i) = w.grid.checked_index(a, b)
                && open(i)
                && !seen[i]
            {
                seen[i] = true;
                q.push_back((a, b));
            }
        }
        while let Some((x, y)) = q.pop_front() {
            for (dx, dy) in s4_sim::tiling::NEIGHBORS {
                if let Some(i) = w.grid.checked_index(x + dx, y + dy)
                    && !seen[i]
                    && open(i)
                {
                    seen[i] = true;
                    q.push_back((x + dx, y + dy));
                }
            }
        }
        seen
    };
    let goal = fill(s4_sim::spiral::walk(w.grid, target.0, target.1, r).collect());
    let ours = fill(vec![home]);
    let next_to = |land: &[bool], a: i32, b: i32| s4_sim::spiral::walk(w.grid, a, b, 9).any(|(c, d)| w.grid.checked_index(c, d).is_some_and(|i| land[i]));
    let water: Vec<(i32, i32, i32)> = (0..size * size)
        .map(|n| (n % size, n / size))
        .filter(|&(a, b)| ship::can_place(w, a, b))
        .map(|(a, b)| (a, b, ship::sector(w, w.grid.checked_index(a, b))))
        .collect();
    let landing = water.iter().filter(|&&(a, b, _)| next_to(&goal, a, b)).filter(|&&(_, _, s)| water.iter().any(|&(c, d, t)| t == s && next_to(&ours, c, d))).min_by_key(|&&(a, b, _)| s4_sim::spiral::grid_distance(a - target.0, b - target.1))?;
    let launch = water.iter().filter(|&&(c, d, t)| t == landing.2 && next_to(&ours, c, d)).min_by_key(|&&(c, d, _)| s4_sim::spiral::grid_distance(c - landing.0, d - landing.1))?;
    Some(((launch.0, launch.1), (landing.0, landing.1)))
}

/// One ignored test per mission: conquered to Won within `ticks`.
macro_rules! conquered {
    ($($name:ident: $map:literal, $ticks:expr;)*) => {$(
        #[test]
        #[ignore = "slow: a whole mission"]
        fn $name() {
            conquered($map, $ticks);
        }
    )*};
}

conquered! {
    roman01_played_to_won: "roman01", 60_000;
    roman02_played_to_won: "roman02", 60_000;
    roman03_played_to_won: "roman03", 60_000;
    viking01_played_to_won: "viking01", 60_000;
    viking02_played_to_won: "viking02", 60_000;
    viking03_played_to_won: "viking03", 60_000;
    maya01_played_to_won: "maya01", 60_000;
    maya02_played_to_won: "maya02", 60_000;
    maya03_played_to_won: "maya03", 60_000;
    dark01_played_to_won: "dark01", 60_000;
    dark02_played_to_won: "dark02", 60_000;
    dark03_played_to_won: "dark03", 400_000;
    dark04_played_to_won: "dark04", 400_000;
    dark05_played_to_won: "dark05", 400_000;
    dark06_played_to_won: "dark06", 60_000;
    dark07_played_to_won: "dark07", 400_000;
    dark09_played_to_won: "dark09", 400_000;
    dark10_played_to_won: "dark10", 400_000;
    dark11_played_to_won: "dark11", 400_000;
    dark12_played_to_won: "dark12", 400_000;
}

// The add-on missions (Mission CD, Mission Disk, Gold Edition, Trojans): the generic driver first.
conquered! {
    ao_viking1_played_to_won: "AO_Viking1", 400_000;
    ao_maya1_played_to_won: "AO_maya1", 400_000;
    ao_maya3_played_to_won: "AO_maya3", 400_000;
    ao_roman2_played_to_won: "AO_roman2", 400_000;
    ao_roman4_played_to_won: "AO_roman4", 400_000;
    ao_trojan02_played_to_won: "AO_trojan02", 400_000;
    ao_trojan05_played_to_won: "AO_trojan05", 400_000;
    ao_trojan06_played_to_won: "AO_trojan06", 400_000;
    ao_trojan08_played_to_won: "AO_trojan08", 400_000;
    ao_trojan12_played_to_won: "AO_trojan12", 400_000;
    md_conflict1_played_to_won: "MD_conflict1", 400_000;
    md_conflict3_played_to_won: "MD_conflict3", 400_000;
    md_maya1_played_to_won: "MD_maya1", 400_000;
    md_maya2_played_to_won: "MD_maya2", 400_000;
    md_maya3_played_to_won: "MD_maya3", 400_000;
    md_maya4_played_to_won: "MD_maya4", 400_000;
    md_maya5_played_to_won: "MD_maya5", 400_000;
    md_roman1_played_to_won: "MD_roman1", 400_000;
    md_roman3_played_to_won: "MD_roman3", 400_000;
    md_roman4_played_to_won: "MD_roman4", 400_000;
    md_roman5_played_to_won: "MD_roman5", 400_000;
    md_viking2_played_to_won: "MD_viking2", 400_000;
    md_viking4_played_to_won: "MD_viking4", 400_000;
    xmd3_maya1_played_to_won: "XMD3_maya1", 400_000;
    xmd3_maya2_played_to_won: "XMD3_maya2", 400_000;
    xmd3_maya3_played_to_won: "XMD3_maya3", 400_000;
    xmd3_maya4_played_to_won: "XMD3_maya4", 400_000;
    xmd3_roman1_played_to_won: "XMD3_roman1", 400_000;
    xmd3_roman2_played_to_won: "XMD3_roman2", 400_000;
    xmd3_roman3_played_to_won: "XMD3_roman3", 400_000;
    xmd3_roman4_played_to_won: "XMD3_roman4", 400_000;
    xmd3_trojan1_played_to_won: "XMD3_trojan1", 400_000;
    xmd3_trojan2_played_to_won: "XMD3_trojan2", 400_000;
    xmd3_trojan3_played_to_won: "XMD3_trojan3", 400_000;
    xmd3_trojan4_played_to_won: "XMD3_trojan4", 400_000;
    xmd3_viking2_played_to_won: "XMD3_viking2", 400_000;
    xmd3_viking3_played_to_won: "XMD3_viking3", 400_000;
    xmd3_viking4_played_to_won: "XMD3_viking4", 400_000;
}

/// Diagnostic: the players, races and the enemy military buildings' kinds of a mission.
#[test]
#[ignore = "diagnostic"]
fn mission_players() {
    let maps = std::env::var("S4_MISSION").unwrap_or_else(|_| "dark08,dark07".into());
    for map in maps.split(',') {
        let Some(m) = Mission::start(map) else { return };
        let info = m.ask(|w| {
            let players: Vec<(i32, u8, u8)> = (1..=w.number_of_players()).map(|p| (p, w.race_of(p as u8), w.alliances.alliance_id[p as usize])).collect();
            let mil: Vec<(u16, u8, u16, u8, i32, i32)> = w
                .entities
                .iter()
                .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner != 1 && MILITARY.contains(&e.kind))
                .map(|(id, e)| (id, e.owner, e.kind, e.race, e.x(), e.y()))
                .collect();
            (players, mil)
        });
        eprintln!("{map}: players {:?}; military {:?}", info.0, info.1);
        let ais: Vec<u8> = m.ask(|w| (1..=8).filter(|&p| w.ai.is_ai(p)).collect());
        let garrisons: Vec<(u8, u16, bool)> = m.ask(|w| w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && MILITARY.contains(&e.kind)).map(|(i, e)| (e.owner, i, s4_sim::buildings::have_inhabitant(w, i))).collect());
        eprintln!("{map}: military (owner, id, inhabited) {garrisons:?}");
        eprintln!("{map}: AI players {ais:?}; setup {:?}", crate::frontend::setup::campaign_setup(&s4_viewer::scene::resolve_map(&format!("Campaign/{map}")), 0));
        for _ in 0..12 {
            m.sim.step(250);
            let mine: Vec<(u16, i32, i32, bool, u8)> = m.ask(|w| {
                w.entities
                    .iter()
                    .filter(|(_, e)| e.obj_type == ObjType::Building && e.owner == 1 && MILITARY.contains(&e.kind))
                    .map(|(i, e)| (i, e.x(), e.y(), w.grid.checked_index(e.x(), e.y()).is_some_and(|n| w.layers.flags[n] & 4 != 0), w.grid.checked_index(e.x(), e.y()).map_or(0, |n| w.owner_of(n))))
                    .collect()
            });
            eprintln!("{map} tick {}: player 1's military (id, x, y, dark, land owner) {mine:?}", m.tick());
            let lost: Vec<(i32, bool, usize)> = m.ask(|w| (1..=w.number_of_players()).map(|p| (p, w.default_player_lost(p), w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Building && e.owner as i32 == p && MILITARY.contains(&e.kind)).count())).collect());
            eprintln!("{map} tick {}: outcome {}, (player, default lost, military) {lost:?}", m.tick(), m.outcome());
        }
    }
}



/// Diagnostic: settlers sent far (`S4_MISSION`, `S4_TO` "x,y", `S4_KINDS` "31,34").
#[test]
#[ignore = "diagnostic"]
fn mission_walk() {
    let map = std::env::var("S4_MISSION").unwrap_or_else(|_| "AO_Viking4".into());
    let to: Vec<i32> = std::env::var("S4_TO").unwrap_or_else(|_| "83,183".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let kinds: Vec<u16> = std::env::var("S4_KINDS").unwrap_or_else(|_| "31,34".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let Some(m) = Mission::start(&map) else { return };
    let (hx, hy) = m.home();
    let mut ids = Vec::new();
    let mut dest = (to[0], to[1]);
    for &k in &kinds {
        let (v, d) = m.muster(k, 3, to[0], to[1]).unwrap();
        ids.extend(v);
        dest = d;
    }
    let sp = m.ask({
        let (x, y) = (to[0], to[1]);
        move |w| spawn_point(w, x, y, 10)
    });
    eprintln!("home ({hx}, {hy}) sector {}, to {to:?} sector {}, free node {dest:?} sector {}, spawn point {sp:?}", m.sector(hx, hy), m.sector(to[0], to[1]), m.sector(dest.0, dest.1));
    m.command(s4_sim::Command::SendTo { x: dest.0 as u16, y: dest.1 as u16, mode: 0, entities: ids.clone() });
    for _ in 0..8 {
        m.sim.step(500);
        let st = m.ask({
            let ids = ids.clone();
            move |w| ids.iter().filter_map(|&i| w.entities.by_index(i as u32).map(|e| (i, e.kind, e.x(), e.y(), e.flags))).collect::<Vec<_>>()
        });
        eprintln!("tick {}: {st:x?}", m.tick());
    }
}

/// Diagnostic: a ferry sent far (`S4_MISSION`, `S4_TO` "x,y"): its node, walk state and task.
#[test]
#[ignore = "diagnostic"]
fn mission_sail() {
    let map = std::env::var("S4_MISSION").unwrap_or_else(|_| "MCD2_maya4".into());
    let to: Vec<i32> = std::env::var("S4_TO").unwrap_or_else(|_| "24,870".into()).split(',').map(|v| v.parse().unwrap()).collect();
    let Some(m) = Mission::start(&map) else { return };
    let dest = m.water_near(to[0], to[1]).unwrap();
    let ferries = m.fleet(2, 1, m.home(), dest);
    eprintln!("home {:?}, destination water {dest:?}, ferries {:?}", m.home(), m.vehicles_of(1, 2));
    m.order(ferries.clone(), dest.0, dest.1);
    let mut last = (0, 0);
    for _ in 0..120 {
        m.sim.step(1000);
        let id = ferries[0];
        let st = m.ask(move |w| {
            let e = w.entities.by_index(id as u32).map(|e| (e.x(), e.y()));
            let v = s4_sim::vehicles::get(w, id).map(|v| (v.task, v.listed, v.walking.as_ref().map(|s| (s.state, s.walk_to & 0xffff, s.walk_to >> 16))));
            (e, v)
        });
        let at = st.0.unwrap_or((0, 0));
        if at != last {
            eprintln!("tick {}: {st:?}", m.tick());
            last = at;
        }
    }
    eprintln!("tick {}: end at {last:?}", m.tick());
}

/// Diagnostic: a script ferry for player `S4_PLAYER` sent by `AI.NewSquad(p, CMD_MOVE_AND_STAY)`
/// (`S4_MISSION`, `S4_AT` "x,y", `S4_TO` "x,y").
#[test]
#[ignore = "diagnostic"]
fn mission_squad_ferry() {
    let map = std::env::var("S4_MISSION").unwrap_or_else(|_| "MCD2_trojan1".into());
    let p: i32 = std::env::var("S4_PLAYER").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let at = std::env::var("S4_AT").unwrap_or_else(|_| "638,647".into());
    let to = std::env::var("S4_TO").unwrap_or_else(|_| "756,756".into());
    let Some(m) = Mission::start(&map) else { return };
    m.sim.step(10);
    let ais: Vec<(usize, bool)> = m.ask(|w| w.ai.players.iter().enumerate().map(|(i, a)| (i, a.is_some())).collect());
    let before: Vec<u16> = m.vehicles_of(p as u8, 2).into_iter().map(|v| v.0).collect();
    let a: Vec<i32> = at.split(',').map(|v| v.parse().unwrap()).collect();
    let (wx, wy) = m.water_near(a[0], a[1]).unwrap();
    m.sim.console(&format!("Vehicles.AddVehicle({wx},{wy},{p},Vehicles.FERRY,0,0,1) AI.NewSquad({p},AI.CMD_MOVE_AND_STAY,{to})"));
    m.sim.step(2);
    let ferry = m.vehicles_of(p as u8, 2).into_iter().find(|v| !before.contains(&v.0));
    eprintln!("AI players {ais:?}; new ferry {ferry:?}");
    let tv: Vec<i32> = to.split(',').map(|v| v.parse().unwrap()).collect();
    let secs = m.ask(move |w| {
        let s = |x: i32, y: i32| s4_sim::vehicles::ship::sector(w, w.grid.checked_index(x, y));
        (s(wx, wy), s(tv[0], tv[1]), s(433, 300), s(440, 302))
    });
    eprintln!("ship sectors: start {}, destination {}, (433, 300) {}, (440, 302) {}", secs.0, secs.1, secs.2, secs.3);
    let Some((id, _, _)) = ferry else { return };
    let wt = m.ask(move |w| w.entities.by_index(id as u32).map(|e| e.flags & 0xf));
    let res = m.ask(move |w| w.ai.players.get(p as usize).and_then(|a| a.as_ref()).map(|a| a.body.reservoirs.iter().map(|r| r.members.len()).collect::<Vec<_>>()));
    eprintln!("warrior type {wt:?}; reservoirs {res:?}");
    let steps: usize = std::env::var("S4_STEPS").ok().and_then(|v| v.parse().ok()).unwrap_or(10);
    for _ in 0..steps {
        m.sim.step(500);
        let st = m.ask(move |w| {
            let e = w.entities.by_index(id as u32).map(|e| (e.x(), e.y()));
            let xy = |p: i32| (p & 0xffff, p >> 16);
            let v = s4_sim::vehicles::get(w, id).map(|v| (v.task, v.walking.as_ref().map(|s| (s.state, xy(s.walk_to), xy(s.coarse), xy(s.fine), s.fine_tries, s.coarse_waypoints.count))));
            (e, v)
        });
        eprintln!("tick {}: {st:?}", m.tick());
    }
}

/// Diagnostic: AO_trojan01's ships (home water, the target's water, where they get to).
#[test]
#[ignore = "diagnostic"]
fn trojan01_ships() {
    let Some(m) = Mission::start("AO_trojan01") else { return };
    let (hx, hy) = m.home();
    let home = m.water_near(hx, hy).unwrap();
    let to = m.water_near(480, 113).unwrap();
    let secs = m.ask(move |w| {
        let s = |(x, y): (i32, i32)| s4_sim::vehicles::ship::sector(w, w.grid.checked_index(x, y));
        (s(home), s(to))
    });
    eprintln!("home water {home:?} sector {}, target water {to:?} sector {}", secs.0, secs.1);
    let ships = m.fleet(1, 2, (hx, hy), to);
    m.order(ships.clone(), to.0, to.1);
    for _ in 0..10 {
        m.sim.step(1000);
        eprintln!("tick {}: ships {:?}", m.tick(), m.vehicles_of(1, 1));
    }
}

/// Diagnostic: a mission's conquest with a look at the last target every 20 000 ticks
/// (`S4_MISSION`).
#[test]
#[ignore = "diagnostic"]
fn mission_last_target() {
    let map = std::env::var("S4_MISSION").unwrap_or_else(|_| "MD_conflict3".into());
    let Some(m) = Mission::start(&map) else { return };
    m.conquer_and(400_000, 200, 30, 30, &|m| {
        if m.tick() % 20_000 >= 200 {
            return;
        }
        let Some(&(id, owner, x, y, _)) = m.enemy_military().first() else { return };
        let info = m.ask(move |w| {
            let e = w.entities.by_index(id as u32).unwrap();
            let near: Vec<(u16, u16, i32, i32, u32)> = w.entities.iter().filter(|(_, s)| s.obj_type == ObjType::Settler && s.owner == 1 && !s.is_dead() && s4_sim::spiral::grid_distance(s.x() - x, s.y() - y) <= 12).map(|(i, s)| (i, s.kind, s.x(), s.y(), s.flags)).take(6).collect();
            (e.kind, e.flags, w.grid.size(), w.tiling.sector_of(w.grid.index(x as u32, y as u32)), near)
        });
        eprintln!("{} tick {}: target {id} of {owner} at ({x}, {y}): kind, flags, map size, sector, player 1's settlers within 12 {info:x?}", m.name, m.tick());
    });
}

/// Diagnostic: dark08's way from player 1's start to the circle (594, 155) if snow were land:
/// a breadth-first search over land and the race's `TerrainSrc` ground (0x5X), with the snow
/// nodes and the water it would need listed.
#[test]
#[ignore = "diagnostic"]
fn dark08_snow_way() {
    let Some(m) = Mission::start("dark08") else { return };
    let (hx, hy) = m.home();
    let res = m.ask(move |w| {
        let size = w.grid.size() as i32;
        let src = (w.rules.magic.terrain_src.get(w.race_of(1) as usize).copied().unwrap_or(0) & 0xf0) as u8;
        let land = |i: usize| w.layers.flags[i] & s4_sim::grid::flag::BLOCKED_LAND == 0;
        // `S4_ANY_GROUND`: every node that is not water counts (which ground blocks the way?).
        let any = std::env::var("S4_ANY_GROUND").is_ok();
        let snow = |i: usize| if any { w.layers.ground[i] > 8 } else { w.layers.ground[i] & 0xf0 == src };
        let mut from = vec![u32::MAX; (size * size) as usize];
        let start = (hy * size + hx) as usize;
        from[start] = start as u32;
        let mut q = std::collections::VecDeque::from([(hx, hy)]);
        let goal = |x: i32, y: i32| s4_sim::spiral::grid_distance(x - 594, y - 155) <= 60;
        while let Some((x, y)) = q.pop_front() {
            if goal(x, y) {
                let mut path = vec![(x, y)];
                let mut at = (y * size + x) as u32;
                while from[at as usize] != at {
                    at = from[at as usize];
                    path.push(((at as i32) % size, (at as i32) / size));
                }
                path.reverse();
                let snowy: Vec<(i32, i32, u8)> = path.iter().copied().filter(|&(a, b)| !land((b * size + a) as usize)).map(|(a, b)| (a, b, w.layers.ground[(b * size + a) as usize])).collect();
                return (src, path.len(), snowy.len(), snowy.into_iter().step_by(5).take(40).collect::<Vec<_>>());
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if !(0..size).contains(&nx) || !(0..size).contains(&ny) {
                    continue;
                }
                let i = (ny * size + nx) as usize;
                if from[i] == u32::MAX && (land(i) || snow(i)) {
                    from[i] = (y * size + x) as u32;
                    q.push_back((nx, ny));
                }
            }
        }
        (src, 0, 0, Vec::new())
    });
    eprintln!("dark08: home ({hx}, {hy}); snow class {:#x}; a way of {} nodes, {} of them blocked: {:?}", res.0, res.1, res.2, res.3);
    let race = m.ask(|w| (w.race_of(1), w.rules.magic.terrain_src, w.rules.magic.terrain_dst));
    eprintln!("dark08: player 1's race, TerrainSrc, TerrainDst {race:?}");
}

/// Diagnostic: dark08's seas: the ship sectors that touch player 1's land and those that touch
/// land within 60 of the circle (594, 155), with a node of each.
#[test]
#[ignore = "diagnostic"]
fn dark08_seas() {
    let Some(m) = Mission::start("dark08") else { return };
    let (hx, hy) = m.home();
    let res = m.ask(move |w| {
        use s4_sim::vehicles::ship;
        let size = w.grid.size() as i32;
        let home_land = land_near(w, hx, hy, 0).map_or(0, |l| l.2);
        let mut by_home: std::collections::BTreeMap<i32, (i32, i32)> = Default::default();
        let mut by_circle: std::collections::BTreeMap<i32, (i32, i32, u32)> = Default::default();
        for n in 0..size * size {
            let (x, y) = (n % size, n / size);
            if !ship::can_place(w, x, y) {
                continue;
            }
            let sec = ship::sector(w, w.grid.checked_index(x, y));
            for (a, b) in s4_sim::spiral::walk(w.grid, x, y, 10) {
                let Some(i) = w.grid.checked_index(a, b) else { continue };
                if w.layers.flags[i] & s4_sim::grid::flag::BLOCKED_LAND != 0 {
                    continue;
                }
                let land = w.tiling.sector_of(i) as u32;
                if land == home_land {
                    by_home.entry(sec).or_insert((x, y));
                }
                if s4_sim::spiral::grid_distance(a - 594, b - 155) <= 60 {
                    by_circle.entry(sec).or_insert((x, y, land));
                }
            }
        }
        (home_land, by_home, by_circle)
    });
    eprintln!("dark08: home land {}; seas by the home land {:?}; seas by the circle's land {:?}", res.0, res.1, res.2);
}

/// Diagnostic: the wall time per 100 ticks of a mission without a driver (`S4_MISSION`).
#[test]
#[ignore = "diagnostic"]
fn mission_speed() {
    let map = std::env::var("S4_MISSION").unwrap_or_else(|_| "MD_Bonus".into());
    let Some(m) = Mission::start(&map) else { return };
    m.sim.step(10);
    eprintln!("vars: '{}'", m.vars());
    m.sim.console("Debug.ShowTextMessage('hello')");
    m.sim.step(1);
    eprintln!("requests: {:?}", m.tutorial.lock().unwrap().requests);
    let t0 = std::time::Instant::now();
    let enemies = m.enemy_military();
    eprintln!("enemy_military {:?}", t0.elapsed());
    let t0 = std::time::Instant::now();
    let targets = m.targets(&enemies);
    eprintln!("targets {:?}", t0.elapsed());
    let t0 = std::time::Instant::now();
    let ours = m.soldiers();
    eprintln!("soldiers {:?} ({})", t0.elapsed(), ours.len());
    let &(_, _, tx, ty, _) = targets.first().unwrap();
    let t0 = std::time::Instant::now();
    let land = m.land_of(tx, ty);
    eprintln!("land_of {:?}", t0.elapsed());
    let t0 = std::time::Instant::now();
    let sp = m.ask(move |w| spawn_point(w, tx, ty, 12));
    eprintln!("spawn_point {:?} {sp:?} land {land}", t0.elapsed());
    for _ in 0..4 {
        let by: Vec<((u8, u16, u8), usize)> = m.ask(|w| {
            let mut c: std::collections::HashMap<(u8, u16, u8), usize> = Default::default();
            for (_, e) in w.entities.iter() {
                *c.entry((e.obj_type as u8, e.kind, e.owner)).or_default() += 1;
            }
            let mut v: Vec<_> = c.into_iter().collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.1));
            v.truncate(12);
            v
        });
        eprintln!("{map} tick {}: most entities (obj type, kind, owner) {by:?}", m.tick());
        m.sim.step(50);
    }
    for _ in 0..30 {
        let t0 = std::time::Instant::now();
        m.sim.step(100);
        eprintln!("{map} tick {}: {:?} per 100 ticks, {} entities", m.tick(), t0.elapsed(), m.ask(|w| w.entities.iter().count()));
    }
}

/// Diagnostic: each player's vehicle counts (0x1549a8c) and listed vehicles at the start of a mission.
#[test]
#[ignore = "diagnostic"]
fn mission_vehicles() {
    let map = std::env::var("S4_MISSION").unwrap_or_else(|_| "MCD2_roman2".into());
    let Some(m) = Mission::start(&map) else { return };
    m.sim.step(200);
    let info = m.ask(|w| {
        (1..=w.number_of_players())
            .map(|p| {
                let i = p as usize * 6;
                let counts: Vec<(u16, u16)> = (1..=5).map(|t| (w.vehicles.building[i + t], w.vehicles.finished[i + t])).collect();
                let listed: Vec<usize> = (1..=5).map(|t| s4_sim::vehicles::iter(w, p as u8, t as u16).count()).collect();
                (p, w.alliances.alliance_id[p as usize], counts, listed)
            })
            .collect::<Vec<_>>()
    });
    for i in info {
        eprintln!("{map}: {i:?}");
    }
}

/// Diagnostic: how fast gardeners light dark land (dark05's farm at (82, 385)).
#[test]
#[ignore = "diagnostic"]
fn gardener_rate() {
    let Some(m) = Mission::start("dark05") else { return };
    let (tx, ty) = (82, 385);
    let land = m.land_of(tx, ty);
    let (fx, fy) = m.dark_frontier(tx, ty, land);
    m.reinforce_at(tx, ty, 30, &[(GARDENER, 20)]);
    let dark = |m: &Mission| m.ask(move |w| s4_sim::spiral::walk(w.grid, fx, fy, 20).filter(|&(x, y)| w.grid.checked_index(x, y).is_some_and(|n| w.layers.flags[n] & 4 != 0)).count());
    let auras = m.ask(|w| auras(w));
    eprintln!("frontier ({fx}, {fy}) at distance {} from the farm; auras {auras:?}", s4_sim::spiral::grid_distance(fx - tx, fy - ty));
    let g: Vec<(u16, i32, i32)> = m.gardeners();
    eprintln!("gardeners at {:?}", &g[..g.len().min(4)]);
    m.command(s4_sim::Command::SendTo { x: fx as u16, y: fy as u16, mode: 0, entities: g.iter().map(|s| s.0).collect() });
    for t in 0..10 {
        let before = dark(&m);
        m.sim.step(1000);
        let jobs = m.ask(|w| {
            let mut c = std::collections::BTreeMap::new();
            for (_, e) in w.entities.iter().filter(|(_, e)| e.obj_type == ObjType::Settler && e.owner == 1 && e.kind == GARDENER && !e.is_dead()) {
                let k = match &e.data { s4_sim::entity::EntityData::Settler(d) => (d.job.list_id, d.job.task), _ => (0, 0) };
                *c.entry(k).or_insert(0) += 1;
            }
            c
        });
        eprintln!("t {}: dark within 20 {before} → {}, gardeners by (list, task) {jobs:?}; at {:?}", (t + 1) * 1000, dark(&m), &m.gardeners()[..2.min(m.gardeners().len())]);
    }
}






/// Diagnostic: can a player get to each mission's targets? For every campaign map (or
/// `S4_MISSION`): the other teams' military buildings at the start and the drivers' explicit
/// targets ([`DRIVER_TARGETS`]), each classed by the cheapest way from player 1's lands (its
/// start and its buildings): `walk` (open land), `sea` (ferries: open water where ships can be
/// made within 9 of both lands, any number of hops), `melt` (also over 0x80 / 0x81 ground,
/// which every race's terrain spell turns into rock), or `NONE`. Writes `out/audio/reach.txt`.
#[test]
#[ignore = "diagnostic: every campaign map"]
fn mission_reach() {
    let dir = s4_formats::game_dir().join("Map").join("Campaign");
    let Ok(rd) = std::fs::read_dir(&dir) else { return };
    let mut maps: Vec<String> = rd.filter_map(|e| e.ok()?.path().file_stem()?.to_str().map(str::to_string)).collect();
    maps.sort();
    if let Ok(only) = std::env::var("S4_MISSION") {
        maps.retain(|m| only.split(',').any(|o| o == m));
    }
    use std::fmt::Write as _;
    let mut report = String::new();
    for map in maps {
        let Some(m) = Mission::start(&map) else { continue };
        let mut targets: Vec<(String, i32, i32)> = m.enemy_military().into_iter().map(|e| (format!("military of {}", e.1), e.2, e.3)).collect();
        targets.extend(DRIVER_TARGETS.iter().filter(|t| t.0 == map).map(|t| (format!("driver {}", t.3), t.1, t.2)));
        let home = m.home();
        let rows = m.ask(move |w| targets.into_iter().map(|(what, x, y)| (what, x, y, reach_class(w, home, (x, y)))).collect::<Vec<_>>());
        let worst = rows.iter().map(|r| r.3).max().unwrap_or(0);
        let mut by: std::collections::BTreeMap<u8, usize> = Default::default();
        for r in &rows {
            *by.entry(r.3).or_default() += 1;
        }
        let who = m.ask(|w| {
            let mut k: std::collections::BTreeMap<u8, usize> = Default::default();
            for (_, e) in w.entities.iter().filter(|(_, e)| e.owner == 1 && !e.is_dead()) {
                *k.entry(e.obj_type as u8).or_default() += 1;
            }
            k
        });
        let _ = writeln!(report, "{map}: worst {}; {by:?}; home {home:?}; player 1's entities by obj type {who:?}", CLASSES[worst as usize]);
        for r in rows.iter().filter(|r| r.3 >= 2) {
            let _ = writeln!(report, "    {} at ({}, {}): {}", r.0, r.1, r.2, CLASSES[r.3 as usize]);
        }
        eprintln!("{map}: worst {}", CLASSES[worst as usize]);
    }
    std::fs::write(concat!(env!("CARGO_MANIFEST_DIR"), "/../../out/audio/reach.txt"), report).unwrap();
}

const CLASSES: [&str; 4] = ["walk", "sea", "melt", "NONE"];

/// The drivers' own target points (missions.rs call sites): (map, x, y, what).
const DRIVER_TARGETS: &[(&str, i32, i32, &str)] = &[
    ("AO_roman3", 438, 555, "gather"),
    ("AO_settle1", 395, 246, "area_stock"),
    ("AO_settle3", 443, 263, "green"),
    ("AO_trojan03", 352, 359, "green / gather"),
    ("AO_trojan04", 562, 545, "claim"),
    ("AO_trojan07", 108, 502, "green"),
    ("AO_maya4", 154, 591, "claim"),
    ("MD_settle1", 88, 665, "gather"),
    ("MD_conflict2", 641, 680, "claim"),
    ("MD_viking3", 469, 101, "claim (port)"),
    ("MD_viking5", 332, 358, "claim"),
    ("MD_roman2", 105, 130, "claim"),
    ("AO_trojan10", 316, 212, "gather priest"),
    ("AO_trojan11", 146, 109, "gather"),
    ("AO_Viking2", 87, 281, "gather"),
    ("AO_Viking4", 83, 183, "gather"),
    ("MCD2_roman1", 295, 64, "ferry_out"),
    ("MCD2_roman3", 260, 279, "gather"),
    ("MCD2_viking3", 13, 225, "ferry_out"),
    ("MCD2_maya3", 255, 255, "squads priest"),
    ("MCD2_maya3", 408, 450, "squads priest"),
    ("MCD2_trojan1", 638, 647, "gather"),
    ("MCD2_maya5", 177, 239, "squads priest"),
    ("MCD2_maya5", 523, 227, "squads priest"),
    ("MCD2_maya5", 473, 498, "claim temple"),
    ("MCD2_roman4", 111, 466, "gather"),
    ("MCD2_roman4", 499, 268, "claim"),
    ("MCD2_maya2", 241, 136, "gather"),
    ("MD_Bonus", 596, 132, "squads thief / claim"),
    ("MD_Bonus", 514, 453, "squads thief"),
    ("AO_maya2", 170, 288, "green"),
    ("AO_maya2", 187, 270, "green"),
    ("MCD2_trojan5", 473, 201, "gather"),
    ("MCD2_trojan5", 356, 159, "squads"),
    ("MCD2_trojan5", 595, 278, "squads"),
    ("MCD2_trojan5", 379, 445, "gather"),
    ("MCD2_trojan5", 223, 436, "gather"),
    ("XMD3_viking1", 149, 412, "claim"),
    ("MCD2_trojan4", 253, 209, "gather priest"),
    ("MCD2_viking5", 331, 325, "gather"),
    ("dark08", 594, 155, "gather priest"),
];

/// 0 walk, 1 sea, 2 melt, 3 none: from player 1's start and buildings to the land at `t`.
fn reach_class(w: &s4_sim::World, home: (i32, i32), t: (i32, i32)) -> u8 {
    use s4_sim::vehicles::ship;
    let size = w.grid.size() as i32;
    let n = (size * size) as usize;
    let blocked = |i: usize| w.layers.flags[i] & s4_sim::grid::flag::BLOCKED_LAND != 0;
    // Land components: plain open land, and with 0x80 / 0x81 ground melted.
    let comps = |melt: bool| {
        let pass = |i: usize| !blocked(i) || melt && matches!(w.layers.ground[i], 0x80 | 0x81);
        let mut c = vec![u32::MAX; n];
        let mut next = 0;
        for s in 0..n {
            if c[s] != u32::MAX || !pass(s) {
                continue;
            }
            let mut q = vec![s];
            c[s] = next;
            while let Some(i) = q.pop() {
                let (x, y) = (i as i32 % size, i as i32 / size);
                for (dx, dy) in s4_sim::tiling::NEIGHBORS {
                    if let Some(j) = w.grid.checked_index(x + dx, y + dy)
                        && c[j] == u32::MAX
                        && pass(j)
                    {
                        c[j] = next;
                        q.push(j);
                    }
                }
            }
            next += 1;
        }
        c
    };
    // The component of a point: its own or the nearest passable node's within 10.
    let comp_at = |c: &[u32], x: i32, y: i32| s4_sim::spiral::walk(w.grid, x, y, 10).filter_map(|(a, b)| w.grid.checked_index(a, b)).map(|i| c[i]).find(|&k| k != u32::MAX);
    let starts = |c: &[u32]| {
        let mut v: Vec<u32> = comp_at(c, home.0, home.1).into_iter().collect();
        for (_, e) in w.entities.iter() {
            if e.owner == 1 && e.obj_type == ObjType::Building && !e.is_dead() && let Some(k) = comp_at(c, e.x(), e.y()) {
                v.push(k);
            }
        }
        v
    };
    let plain = comps(false);
    let from = starts(&plain);
    let water_goal = !w.grid.checked_index(t.0, t.1).is_some_and(|i| !blocked(i) || w.layers.ground[i] > 7);
    let goal = if water_goal { u32::MAX } else { comp_at(&plain, t.0, t.1).unwrap_or(u32::MAX - 1) };
    if from.contains(&goal) {
        return 0;
    }
    // Seas: per ship sector, the land components within 9 of its open water.
    let mut seas: std::collections::BTreeMap<i32, std::collections::BTreeSet<u32>> = Default::default();
    for y in 0..size {
        for x in 0..size {
            if (x + y) % 3 != 0 || !ship::can_place(w, x, y) {
                continue;
            }
            let s = ship::sector(w, w.grid.checked_index(x, y));
            let e = seas.entry(s).or_default();
            for (a, b) in s4_sim::spiral::walk(w.grid, x, y, 9) {
                if let Some(i) = w.grid.checked_index(a, b)
                    && plain[i] != u32::MAX
                {
                    e.insert(plain[i]);
                }
            }
        }
    }
    // A water goal: the ship sector of the open water nearest to it.
    let goal_sea = water_goal.then(|| s4_sim::spiral::walk(w.grid, t.0, t.1, 20).find(|&(a, b)| ship::can_place(w, a, b)).map(|(a, b)| ship::sector(w, w.grid.checked_index(a, b)))).flatten();
    // Player 1's ships start on their seas.
    let own_seas: std::collections::BTreeSet<i32> = ship::ships(w, 1).filter_map(|id| w.entities.by_index(id as u32)).map(|e| ship::sector(w, w.grid.checked_index(e.x(), e.y()))).collect();
    let mut have: std::collections::BTreeSet<u32> = from.into_iter().collect();
    for s in &own_seas {
        if let Some(l) = seas.get(s) {
            have.extend(l.iter().copied());
        }
    }
    loop {
        let before = have.len();
        for lands in seas.values() {
            if lands.iter().any(|l| have.contains(l)) {
                have.extend(lands.iter().copied());
            }
        }
        if have.contains(&goal) || goal_sea.is_some_and(|g| own_seas.contains(&g) || seas.get(&g).is_some_and(|l| l.iter().any(|x| have.contains(x)))) {
            return 1;
        }
        if have.len() == before {
            break;
        }
    }
    let melted = comps(true);
    let goal = comp_at(&melted, t.0, t.1);
    // Melted lands joined with every land already reached (walk or sea).
    let reached: std::collections::BTreeSet<u32> = (0..n).filter(|&i| plain[i] != u32::MAX && have.contains(&plain[i])).map(|i| melted[i]).collect();
    if goal.is_some_and(|g| reached.contains(&g)) {
        return 2;
    }
    3
}
