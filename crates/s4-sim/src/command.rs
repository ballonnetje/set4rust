//! Player commands: the deterministic game messages 5001..5052 (`NET_LOGIC_MSG_*`).
//!
//! In the original a command is a `CEvn_Logic` (event id, owner player at byte +30, `wParam`,
//! `lParam`, optional `u16` array at +24 with its byte length at +28). The network engine delivers
//! it for a later valid tick and every peer runs it at the start of that tick in
//! `CInputProcessor::Process` → `CNetInputProcessor::Process` (release 0x4f6240), whose switch defines
//! how each field is read. [`Command`] is that switch as a type: one variant per message, fields
//! named after what the handler does with them. Senders (for naming) are the `CInputProcessor`
//! UI handlers and the AI (`CEcoManager`).
//!
//! [`NetMessage`] is the original shape. The conversion is lossless both ways:
//! [`PlayerCommand::from_message`] yields a typed variant only when re-encoding it reproduces the
//! message exactly (the handler ignores some bits, e.g. `lParam` of a crush); anything else
//! becomes [`Command::Other`], which [`Command::interpret`] reads the way the handler does.
//!
//! Unpacking helpers in the original: `UnpackHiValue` = `v >> 16`, `UnpackLoValue` = `v & 0xFFFF`,
//! `UnpackSignedLoValue` = `v as i16`, positions are `Y16X16` (x low, y high).

/// A message as the original sends it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetMessage {
    /// `m_iEventId` (5001..5052 for logic messages).
    pub kind: u32,
    /// Owner player (`CEvn_Logic` byte +30).
    pub player: u8,
    pub wparam: u32,
    pub lparam: u32,
    /// The `u16` array behind `m_iData` (entity ids for 5014/5021/5041); empty = null pointer.
    pub data: Vec<u16>,
}

/// A command issued by `player`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PlayerCommand {
    pub player: u8,
    pub command: Command,
}

/// Messages the handler only reports as "Unsupported message" (debug assert) and otherwise ignores.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unsupported {
    ForceGoto = 5015,
    StandGround = 5016,
    SetWaypoint = 5017,
    Patrol = 5018,
    InjuredOut = 5019,
    Escort = 5020,
    EntityToSelection = 5022,
    SelectKind = 5023,
    SelectType = 5024,
    SearchUnit = 5026,
    StandardProduction = 5036,
    SetGoodSwitch = 5037,
    SetJobPercent = 5038,
    GroupToSelection = 5042,
    SelectGroup = 5043,
    SetGoodPriority = 5045,
    SetTrade = 5046,
}

impl Unsupported {
    const ALL: [Unsupported; 17] = [
        Unsupported::ForceGoto,
        Unsupported::StandGround,
        Unsupported::SetWaypoint,
        Unsupported::Patrol,
        Unsupported::InjuredOut,
        Unsupported::Escort,
        Unsupported::EntityToSelection,
        Unsupported::SelectKind,
        Unsupported::SelectType,
        Unsupported::SearchUnit,
        Unsupported::StandardProduction,
        Unsupported::SetGoodSwitch,
        Unsupported::SetJobPercent,
        Unsupported::GroupToSelection,
        Unsupported::SelectGroup,
        Unsupported::SetGoodPriority,
        Unsupported::SetTrade,
    ];

    fn from_kind(kind: u32) -> Option<Unsupported> {
        Self::ALL.into_iter().find(|u| *u as u32 == kind)
    }
}

/// One `CNetInputProcessor::Process` case. Entity ids are the raw values from the message (the
/// handler resolves them with `GetAliveBuildingPtrOfPlayer` & co. at execution time).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    /// 5001 `CBuilding::SetWorkingAreaPackedXY` (UI `SetWorkingArea`, AI `ChangeWorkingAreaTo`).
    SetWorkingArea { building: u32, x: u16, y: u16 },
    /// 5002 `CBuilding::CrushBuilding`.
    CrushBuilding { building: u32 },
    /// 5003 `CBuilding::Switch` (production on/off).
    SwitchBuilding { building: u32 },
    /// 5004 building-site role vtable +36 (UI `PrioBuildingSite`); only while not built up.
    PrioritizeBuildingSite { building: u32 },
    /// 5005 `CTradingBuildingRole::SetTradeTarget` (UI `SelectTradeDest`).
    SetTradeTarget { building: u32, target: u32 },
    /// 5006 `CTradingBuildingRole::TransportGood(good, amount)`.
    TransportGood { building: u16, good: u16, amount: u32 },
    /// 5007 `CTradingBuildingRole::TradeWith(value)`.
    TradeWith { building: u32, value: u32 },
    /// 5008 `CTradingBuildingRole::TradeGood(value)`.
    TradeGood { building: u32, value: u32 },
    /// 5009 vehicle vtable +152 (UI `VehicleUnload`).
    VehicleUnload { vehicle: u32 },
    /// 5010 `CCatapult::RequestAmmoDonkey` (UI `CatapultLoad`).
    CatapultLoad { vehicle: u32 },
    /// 5011 `CCart::CreateSettlement`.
    CreateSettlement { cart: u32 },
    /// 5012 `CCart::CreateFoundationCart`.
    CreateFoundationCart { cart: u32 },
    /// 5013 `CCart::CreateNormalCart` (UI `Morph2Cart`).
    CreateNormalCart { cart: u32 },
    /// 5014 group manager vtable +40 `(count, ids, x, y, mode)` (UI `SendTo`).
    SendTo { x: u16, y: u16, mode: u32, entities: Vec<u16> },
    /// 5021 `CSettler::ChangeType(carrier)` for each own specialist (warrior type 7) standing in
    /// an own eco-sector (UI `MorphToCarrier`).
    MorphToCarrier { entities: Vec<u16> },
    /// 5025 `CBuildingMgr::CheckForBuild(x, y, player, type, 1)` (UI `Build`, AI `ConstructBuilding`).
    Build { building: u32, x: u16, y: u16 },
    /// 5027 `CEcoSector::ChangeMinMaxValues(kind, value)` (UI `UnitReservation`).
    SetUnitReservation { eco_sector: u16, kind: u16, value: u32 },
    /// 5028 `CEcoSector::ChangeBuildingSupplyPrio(good, building, (i8)priority)`.
    SetBuildingSupplyPriority { eco_sector: u16, good: u16, building: u16, priority: i16 },
    /// 5029 `CEcoSectorMgr::ChangeTransportPrio(good, delta)` (UI `TransportPrioChanged`).
    SetTransportPriority { eco_sector: u32, good: u16, delta: i16 },
    /// 5030 `CWorkshopBuildingRole::CancelCurrentProduction` (UI `DestroyBuildupVehicle`).
    CancelProduction { building: u32 },
    /// 5031 building event 0x12, only when `value == -1` (UI `FillTower`).
    FillTower { building: u32, value: u32 },
    /// 5032 building event 0x13 `(kind, amount)` (UI `EmptyTower`).
    EmptyTower { building: u16, kind: u16, amount: u32 },
    /// 5033 `CEcoSector::TakeToolOrder` (building type 21) / `TakeWeaponOrder` (22) (UI `UrgentProduction`).
    ProductionOrder { building: u16, good: u16, amount: u32 },
    /// 5034 `CEcoSector::SetWeaponPercentage` + reset of the produced-weapon counters.
    SetWeaponPercentage { building: u32, percent: u32 },
    /// 5035 `CEcoSector::SetWeaponAutoProduction(value != 1)`.
    SetWeaponAutoProduction { building: u32, value: u32 },
    /// 5039 `CEcoSector::ChangeSpecialistWish` (UI `SpecialistProduction`).
    SetSpecialistWish { eco_sector: u16, specialist: u16, value: u32 },
    /// 5040 `CWorkshopBuildingRole::TakeOrder` (UI `VehicleProduction`).
    WorkshopOrder { building: u16, item: u16, amount: u32 },
    /// 5041 group manager: clear group, then add each ready own entity (UI `DefineGroup`).
    DefineGroup { group: u32, entities: Vec<u16> },
    /// 5044 `CStorageBuildingRole::SwitchGood` (UI `StorageGoodSwitch`).
    StorageGoodSwitch { building: u16, good: u16, value: u32 },
    /// 5047 `CDarkTempleRole::TakeOrder` (Dark Tribe) or `CEcoSector::TakeSoldierOrder` (UI `UnitProduction`).
    SoldierOrder { building: u16, kind: u16, amount: u32 },
    /// 5048 priest (settler type 45) entity event 3 `(spell, player, target)` (UI `SelectSpellDest`).
    CastSpell { priest: u16, spell: u16, x: u16, y: u16 },
    /// 5049 `CVehicleMgr::DeleteVehicle` for an own wheeler (UI `DestroyWarMachine`).
    DestroyVehicle { vehicle: u32 },
    /// 5050 `CTradingBuildingRole::ChangeTradeStatus(player, value != 0)` (UI `SetTradePlayerStatus`).
    SetTradeStatus { building: u16, player: u16, value: u32 },
    /// 5051 `CHJBMgr` vtable +12 `(player)`. Purpose not researched.
    HjbStart,
    /// 5052 `CHJBMgr` vtable +16 `(player, value)` (`CHJBMgr::ProcessHJBRequest`). Purpose not researched.
    HjbRequest { value: u32 },
    /// 4009 (0xfa9, the id the release's kick sends): player `player` left the network game; it
    /// is a computer player from now on (`control` 2 or 3). [ours] Injected by the network layer
    /// on every peer at the same tick: the release does it locally (0x5a2da0 → event 0x39).
    PlayerLeft { player: u8, control: u8 },
    /// A message the handler ignores (see [`Unsupported`]).
    Unsupported(Unsupported),
    /// Any message whose fields are not in canonical form, or an unknown id.
    Other { kind: u32, wparam: u32, lparam: u32, data: Vec<u16> },
}

const fn hi(v: u32) -> u16 {
    (v >> 16) as u16
}
const fn lo(v: u32) -> u16 {
    v as u16
}
const fn pack(hi: u16, lo: u16) -> u32 {
    (hi as u32) << 16 | lo as u32
}

/// `(kind, wparam, lparam, data)`.
type Fields = (u32, u32, u32, Vec<u16>);

impl Command {
    /// The `CNetInputProcessor::Process` case id.
    pub fn kind(&self) -> u32 {
        self.encode().0
    }

    fn encode(&self) -> Fields {
        use Command::*;
        let n = Vec::new;
        match self {
            SetWorkingArea { building, x, y } => (5001, *building, pack(*y, *x), n()),
            CrushBuilding { building } => (5002, *building, 0, n()),
            SwitchBuilding { building } => (5003, *building, 0, n()),
            PrioritizeBuildingSite { building } => (5004, *building, 0, n()),
            SetTradeTarget { building, target } => (5005, *building, *target, n()),
            TransportGood { building, good, amount } => (5006, pack(*building, *good), *amount, n()),
            TradeWith { building, value } => (5007, *building, *value, n()),
            TradeGood { building, value } => (5008, *building, *value, n()),
            VehicleUnload { vehicle } => (5009, *vehicle, 0, n()),
            CatapultLoad { vehicle } => (5010, *vehicle, 0, n()),
            CreateSettlement { cart } => (5011, *cart, 0, n()),
            CreateFoundationCart { cart } => (5012, *cart, 0, n()),
            CreateNormalCart { cart } => (5013, *cart, 0, n()),
            SendTo { x, y, mode, entities } => (5014, pack(*y, *x), *mode, entities.clone()),
            MorphToCarrier { entities } => (5021, 0, 0, entities.clone()),
            Build { building, x, y } => (5025, *building, pack(*y, *x), n()),
            SetUnitReservation { eco_sector, kind, value } => (5027, pack(*eco_sector, *kind), *value, n()),
            SetBuildingSupplyPriority { eco_sector, good, building, priority } => {
                (5028, pack(*good, *building), pack(*eco_sector, *priority as u16), n())
            }
            SetTransportPriority { eco_sector, good, delta } => (5029, pack(*good, *delta as u16), *eco_sector, n()),
            CancelProduction { building } => (5030, *building, 0, n()),
            FillTower { building, value } => (5031, *building, *value, n()),
            EmptyTower { building, kind, amount } => (5032, pack(*building, *kind), *amount, n()),
            ProductionOrder { building, good, amount } => (5033, pack(*building, *good), *amount, n()),
            SetWeaponPercentage { building, percent } => (5034, *building, *percent, n()),
            SetWeaponAutoProduction { building, value } => (5035, *building, *value, n()),
            SetSpecialistWish { eco_sector, specialist, value } => (5039, pack(*eco_sector, *specialist), *value, n()),
            WorkshopOrder { building, item, amount } => (5040, pack(*building, *item), *amount, n()),
            DefineGroup { group, entities } => (5041, *group, 0, entities.clone()),
            StorageGoodSwitch { building, good, value } => (5044, pack(*building, *good), *value, n()),
            SoldierOrder { building, kind, amount } => (5047, pack(*building, *kind), *amount, n()),
            CastSpell { priest, spell, x, y } => (5048, pack(*priest, *spell), pack(*y, *x), n()),
            DestroyVehicle { vehicle } => (5049, *vehicle, 0, n()),
            SetTradeStatus { building, player, value } => (5050, pack(*building, *player), *value, n()),
            HjbStart => (5051, 0, 0, n()),
            HjbRequest { value } => (5052, *value, 0, n()),
            PlayerLeft { player, control } => (4009, *player as u32, *control as u32, n()),
            Unsupported(u) => (*u as u32, 0, 0, n()),
            Other { kind, wparam, lparam, data } => (*kind, *wparam, *lparam, data.clone()),
        }
    }

    /// How `CNetInputProcessor::Process` reads a message: fields the handler does not use are
    /// dropped. `None` for ids outside its switch (the `default:` case, a debug report).
    pub fn interpret(kind: u32, w: u32, l: u32, data: &[u16]) -> Option<Command> {
        use Command::*;
        let d = || data.to_vec();
        Some(match kind {
            5001 => SetWorkingArea { building: w, x: lo(l), y: hi(l) },
            5002 => CrushBuilding { building: w },
            5003 => SwitchBuilding { building: w },
            5004 => PrioritizeBuildingSite { building: w },
            5005 => SetTradeTarget { building: w, target: l },
            5006 => TransportGood { building: hi(w), good: lo(w), amount: l },
            5007 => TradeWith { building: w, value: l },
            5008 => TradeGood { building: w, value: l },
            5009 => VehicleUnload { vehicle: w },
            5010 => CatapultLoad { vehicle: w },
            5011 => CreateSettlement { cart: w },
            5012 => CreateFoundationCart { cart: w },
            5013 => CreateNormalCart { cart: w },
            5014 => SendTo { x: lo(w), y: hi(w), mode: l, entities: d() },
            5021 => MorphToCarrier { entities: d() },
            5025 => Build { building: w, x: lo(l), y: hi(l) },
            5027 => SetUnitReservation { eco_sector: hi(w), kind: lo(w), value: l },
            5028 => SetBuildingSupplyPriority { eco_sector: hi(l), good: hi(w), building: lo(w), priority: lo(l) as i16 },
            5029 => SetTransportPriority { eco_sector: l, good: hi(w), delta: lo(w) as i16 },
            5030 => CancelProduction { building: w },
            5031 => FillTower { building: w, value: l },
            5032 => EmptyTower { building: hi(w), kind: lo(w), amount: l },
            5033 => ProductionOrder { building: hi(w), good: lo(w), amount: l },
            5034 => SetWeaponPercentage { building: w, percent: l },
            5035 => SetWeaponAutoProduction { building: w, value: l },
            5039 => SetSpecialistWish { eco_sector: hi(w), specialist: lo(w), value: l },
            5040 => WorkshopOrder { building: hi(w), item: lo(w), amount: l },
            5041 => DefineGroup { group: w, entities: d() },
            5044 => StorageGoodSwitch { building: hi(w), good: lo(w), value: l },
            5047 => SoldierOrder { building: hi(w), kind: lo(w), amount: l },
            5048 => CastSpell { priest: hi(w), spell: lo(w), x: lo(l), y: hi(l) },
            5049 => DestroyVehicle { vehicle: w },
            5050 => SetTradeStatus { building: hi(w), player: lo(w), value: l },
            5051 => HjbStart,
            5052 => HjbRequest { value: w },
            4009 if w <= 0xff && l <= 0xff => PlayerLeft { player: w as u8, control: l as u8 },
            _ => Unsupported(self::Unsupported::from_kind(kind)?),
        })
    }

    /// The command as the handler will execute it: [`Command::Other`] resolved through
    /// [`Command::interpret`] (`None` if the handler ignores the id).
    pub fn canonical(&self) -> Option<Command> {
        match self {
            Command::Other { kind, wparam, lparam, data } => Command::interpret(*kind, *wparam, *lparam, data),
            c => Some(c.clone()),
        }
    }
}

impl PlayerCommand {
    pub fn new(player: u8, command: Command) -> PlayerCommand {
        PlayerCommand { player, command }
    }

    /// Lossless: typed if the message is in canonical form, [`Command::Other`] otherwise.
    pub fn from_message(m: &NetMessage) -> PlayerCommand {
        let command = Command::interpret(m.kind, m.wparam, m.lparam, &m.data)
            .filter(|c| c.encode() == (m.kind, m.wparam, m.lparam, m.data.clone()))
            .unwrap_or_else(|| Command::Other { kind: m.kind, wparam: m.wparam, lparam: m.lparam, data: m.data.clone() });
        PlayerCommand { player: m.player, command }
    }

    pub fn to_message(&self) -> NetMessage {
        let (kind, wparam, lparam, data) = self.command.encode();
        NetMessage { kind, player: self.player, wparam, lparam, data }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::Random16;

    fn samples() -> Vec<Command> {
        use Command::*;
        vec![
            SetWorkingArea { building: 9, x: 100, y: 200 },
            CrushBuilding { building: 9 },
            SwitchBuilding { building: 9 },
            PrioritizeBuildingSite { building: 9 },
            SetTradeTarget { building: 9, target: 12 },
            TransportGood { building: 9, good: 3, amount: 8 },
            TradeWith { building: 9, value: 2 },
            TradeGood { building: 9, value: 2 },
            VehicleUnload { vehicle: 9 },
            CatapultLoad { vehicle: 9 },
            CreateSettlement { cart: 9 },
            CreateFoundationCart { cart: 9 },
            CreateNormalCart { cart: 9 },
            SendTo { x: 1, y: 2, mode: 3, entities: vec![7, 8, 9] },
            MorphToCarrier { entities: vec![7] },
            Build { building: 1, x: 300, y: 301 },
            SetUnitReservation { eco_sector: 2, kind: 1, value: 5 },
            SetBuildingSupplyPriority { eco_sector: 2, good: 3, building: 4, priority: -1 },
            SetTransportPriority { eco_sector: 2, good: 3, delta: -1 },
            CancelProduction { building: 9 },
            FillTower { building: 9, value: u32::MAX },
            EmptyTower { building: 9, kind: 1, amount: 2 },
            ProductionOrder { building: 9, good: 4, amount: 1 },
            SetWeaponPercentage { building: 9, percent: 50 },
            SetWeaponAutoProduction { building: 9, value: 1 },
            SetSpecialistWish { eco_sector: 1, specialist: 47, value: 3 },
            WorkshopOrder { building: 9, item: 2, amount: 1 },
            DefineGroup { group: 3, entities: vec![10, 11] },
            StorageGoodSwitch { building: 9, good: 5, value: 1 },
            SoldierOrder { building: 9, kind: 29, amount: 4 },
            CastSpell { priest: 20, spell: 1, x: 5, y: 6 },
            DestroyVehicle { vehicle: 9 },
            SetTradeStatus { building: 9, player: 2, value: 1 },
            HjbStart,
            HjbRequest { value: 7 },
            PlayerLeft { player: 3, control: 3 },
            Unsupported(self::Unsupported::Patrol),
        ]
    }

    #[test]
    fn typed_commands_round_trip() {
        for c in samples() {
            let pc = PlayerCommand::new(3, c);
            let m = pc.to_message();
            assert_eq!(PlayerCommand::from_message(&m), pc, "{m:?}");
        }
    }

    #[test]
    fn every_message_round_trips() {
        // Random fields for every id 5000..5053, including non-canonical ones.
        let mut r = Random16::new(1, 0);
        let mut w32 = || r.rand() << 16 | r.rand();
        for kind in 5000..=5053u32 {
            for i in 0..64 {
                let (w, l) = match i % 4 {
                    0 => (0, 0),
                    1 => (w32() & 0xFFFF, 0),
                    _ => (w32(), w32()),
                };
                let data = if i % 3 == 0 { vec![] } else { vec![(w & 0xFFFF) as u16, 7] };
                let m = NetMessage { kind, player: (i % 9) as u8, wparam: w, lparam: l, data };
                let pc = PlayerCommand::from_message(&m);
                assert_eq!(pc.to_message(), m);
            }
        }
    }

    #[test]
    fn non_canonical_is_other_but_interprets_like_the_handler() {
        let m = NetMessage { kind: 5002, player: 1, wparam: 9, lparam: 1234, data: vec![] };
        let pc = PlayerCommand::from_message(&m);
        assert!(matches!(pc.command, Command::Other { .. }));
        assert_eq!(pc.command.canonical(), Some(Command::CrushBuilding { building: 9 }));
        let unknown = NetMessage { kind: 4000, player: 1, wparam: 0, lparam: 0, data: vec![] };
        assert_eq!(PlayerCommand::from_message(&unknown).command.canonical(), None);
    }
}
