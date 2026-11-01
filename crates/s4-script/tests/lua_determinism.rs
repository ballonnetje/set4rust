//! Lua's string hashing is seeded per state (Lua 5.2 `makeseed`). The stock seed mixes time and
//! addresses, so `pairs()` over string keys visited them in an order that differed between
//! states, runs and clients — a desync for map scripts. The vendored Lua (`vendor/lua-src`) uses
//! a fixed seed; two states must iterate the same table identically.

use mlua::Lua;

fn order(lua: &Lua) -> Vec<String> {
    lua.load(
        r#"
        local t = {}
        for i = 1, 300 do t["key" .. i] = i; t[string.rep("x", i % 50) .. i] = i end
        local out = {}
        for k in pairs(t) do out[#out + 1] = k end
        return out
        "#,
    )
    .eval::<Vec<String>>()
    .unwrap()
}

#[test]
fn two_states_iterate_string_keys_identically() {
    // Allocations in between move the second state elsewhere in memory.
    let a = Lua::new();
    let _pad: Vec<Vec<u8>> = (0..64).map(|i| vec![0u8; 4096 + i]).collect();
    let b = Lua::new();
    let (oa, ob) = (order(&a), order(&b));
    assert_eq!(oa.len(), 600);
    assert_eq!(oa, ob);
    // And a third, created after the first two are gone.
    drop((a, b));
    assert_eq!(order(&Lua::new()), oa);
}

/// The only shipped script whose result depends on table-iteration order is
/// `MCD2_trojan4.txt` `goods.dropGoods`: `next(_goods, index)` over a 9-item list constructor,
/// two `Game.Random` draws per pile. In the release's Lua 3.2 a table is one hash part:
/// `luaH_new` (0x6832d0) sizes it to the first of {5, 11, 23, ..} (0x1029e48) above n·3/2, a
/// number's main slot is `(int)n % size` (0x6830f0 / 0x683140), the rehash (0x6834e0) grows it
/// to the first above 2·used+2, and `next` (0x681560 → 0x67a5a0) walks the slots upwards. Nine
/// keys end in a 23-slot table at slots 1..9 (also when it grows from 5 or 11), so the
/// original visits 1..9 ascending. Lua 5.2's array part gives the same.
#[test]
fn next_walks_a_list_constructor_in_index_order() {
    let lua = Lua::new();
    let keys = lua
        .load(
            r#"
            local t = { {a=1},{a=2},{a=3},{a=4},{a=5},{a=6},{a=7},{a=8},{a=9} }
            local out, k = {}, next(t, nil)
            while k do out[#out + 1] = k; k = next(t, k) end
            return out
            "#,
        )
        .eval::<Vec<i64>>()
        .unwrap();
    assert_eq!(keys, (1..=9).collect::<Vec<_>>());
}
