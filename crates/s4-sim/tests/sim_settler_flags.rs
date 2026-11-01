//! The flags the soldier and specialist role Inits set at creation (`create::add_settler`).

use s4_sim::entity::flag;
use s4_sim::{Grid, Rules, World};

#[test]
fn soldiers_and_specialists_are_selectable() {
    let Ok(rules) = Rules::load_default() else { return };
    let g = Grid::new(64).unwrap();
    let mut w = World::new(g, rules);
    w.layers.ground.fill(16);
    w.init_tiling(&vec![1; g.len()]);
    let kinds: Vec<(u16, u8)> = (1..67).map(|k| (k, w.rules.settler_role(0, k))).collect();
    let pick = |role: u8| kinds.iter().find(|&&(_, r)| r == role).map(|&(k, _)| k).unwrap();
    for (role, want) in [(1, 0), (7, flag::VULNERABLE_MASK | flag::SELECTABLE), (12, flag::VULNERABLE_MASK | flag::SELECTABLE)] {
        let kind = pick(role);
        let s = s4_sim::create::add_settler(&mut w, 10 + role as i32, 10, 1, kind).unwrap();
        let f = w.entities.by_index(s as u32).unwrap().flags;
        assert_eq!(f & (flag::VULNERABLE_MASK | flag::SELECTABLE), want, "role {role}, settler {kind}");
    }
}
