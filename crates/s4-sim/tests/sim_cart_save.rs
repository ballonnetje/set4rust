//! Savegame oracle for the `CCart` records (chunk 166, class 57; `vehicles::save::parse_cart`,
//! load ctor 0x55afd0). Every cart's tail decodes exactly and holds:
//! * the build record it repeats (+0x6c..=+0x7c) equals the `CVehicle` one;
//! * +0x110..=+0x112, +0xbc, +0xbd and the settlers' "inside" flags are bools;
//! * the slots (0x55f3d0: the element size 12, the count) are six `{good, amount, +8}` (+8 is never written: stack garbage) with goods below 43 and amounts 0..=8, a
//!   slot with an amount names its good (0x55c690 / 0x55c920);
//! * a cart that is not a foundation cart has no settlers or goods lists; a foundation cart's
//!   +0xb8 / +0xb9 count its pioneers (type 0x2f) / carriers (type 1) inside, and its +0xbf is
//!   the sum of the needed goods (4 boards + 8 stones + 2 hammers, 0x55c360);
//! * the trade state +0x120 is 0..=4, and a cart with a source or target is a trade cart.
//!
//! Skipped without savegames.

use std::path::{Path, PathBuf};

use s4_sim::vehicles::save;

fn saves() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("USERPROFILE") else { return vec![] };
    let mut stack = vec![Path::new(&home).join("Documents").join("TheSettlers4").join("Save")];
    let mut v = Vec::new();
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "sav") {
                v.push(p);
            }
        }
    }
    v.sort();
    v
}

#[test]
fn cart_records_decode_and_hold() {
    let files = saves();
    if files.is_empty() {
        eprintln!("skipping: no savegames");
        return;
    }
    let (mut carts, mut foundation, mut trading, mut loaded, mut v2) = (0, 0, 0, 0, 0);
    let mut failures = Vec::new();
    for f in &files {
        let Some((vs, _, _)) = save::load(f).unwrap() else { continue };
        for v in vs.iter().filter(|v| v.kind == 5) {
            carts += 1;
            let w = format!("{} cart {}", f.display(), v.id);
            if v.class != 57 {
                failures.push(format!("{w}: class {}", v.class));
            }
            let Some(c) = save::parse_cart(&v.tail) else {
                failures.push(format!("{w}: tail {:?}", v.tail));
                continue;
            };
            if c.version == 2 {
                v2 += 1;
            }
            if c.b6c != v.b6c || c.s70 != v.s70 || c.f7c != v.f7c {
                failures.push(format!("{w}: build {:?} {:?} {} vs {:?} {:?} {}", c.b6c, c.s70, c.f7c, v.b6c, v.s70, v.f7c));
            }
            let bools = [c.b110[0], c.b110[1], c.b110[2], c.b8[4], c.b8[5]].into_iter().chain(c.settlers.iter().map(|s| s[1]));
            if bools.into_iter().any(|b| !(0..=1).contains(&b)) {
                failures.push(format!("{w}: bools {:?} {:?} {:?}", c.b110, c.b8, c.settlers));
            }
            if c.slots_version != 12 || c.slots.len() != 6 {
                failures.push(format!("{w}: slots v{} {:?}", c.slots_version, c.slots));
            }
            for s in &c.slots {
                if !(0..43).contains(&s[0]) || !(0..=8).contains(&s[1]) || (s[1] != 0 && s[0] == 0) {
                    failures.push(format!("{w}: slot {s:?}"));
                }
            }
            if c.slots.iter().any(|s| s[1] != 0) {
                loaded += 1;
            }
            let found = c.b8[4] != 0;
            if found {
                foundation += 1;
                let inside = |k: i64| c.settlers.iter().filter(|s| s[1] != 0 && s[3] == k).count() as i64;
                if inside(0x2f) != c.b8[0] || inside(1) != c.b8[1] {
                    failures.push(format!("{w}: inside {} / {} vs {:?}", inside(0x2f), inside(1), c.b8));
                }
                let needed: i64 = c.goods.iter().map(|g| g[0]).sum();
                if c.b8[7] != 14 || (c.goods.len() == 3 && needed > 14) {
                    failures.push(format!("{w}: goods {:?} needed {}", c.goods, c.b8[7]));
                }
            } else if !c.settlers.is_empty() || !c.goods.is_empty() {
                failures.push(format!("{w}: not a foundation cart but {:?} {:?}", c.settlers, c.goods));
            }
            let [_, source, target, state] = c.i114;
            if !(0..=4).contains(&state) || (found && (source != 0 || target != 0)) {
                failures.push(format!("{w}: trade {:?} foundation {found}", c.i114));
            }
            if source != 0 || target != 0 {
                trading += 1;
            }
        }
    }
    eprintln!("{carts} carts: {foundation} foundation carts, {trading} trading, {loaded} loaded, {v2} of version 2");
    for x in failures.iter().take(20) {
        eprintln!("  {x}");
    }
    assert!(carts > 0, "no carts in the saves");
    assert!(failures.is_empty(), "{} mismatches", failures.len());
}
