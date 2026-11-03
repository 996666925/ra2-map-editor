use anyhow::{Context, Result, ensure};
use ra2_map_document::{
    document::{MapDocument, Record, Target, Value},
    mission::{self, Kind},
    new_map::MAX_PLAYERS,
    player_starts, startup_owner,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    #[serde(default, rename = "crateLottery")]
    pub crate_lottery: Option<crate_lottery::CrateLotteryRules>,
    pub version: u32,
    pub id: String,
    #[serde(default)]
    pub mode: MissionMode,
    #[serde(default)]
    pub player: String,
    #[serde(default, rename = "houseDefinitions")]
    pub house_definitions: Vec<HouseDefinition>,
    #[serde(default, rename = "aiTriggers")]
    pub ai_triggers: Vec<AiTriggerDefinition>,
    #[serde(default)]
    pub basic: Option<MissionBasicOptions>,
    #[serde(default, rename = "specialFlags")]
    pub special_flags: Option<MissionSpecialFlagsOptions>,
    #[serde(default)]
    pub lighting: Option<MissionLightingOptions>,
    #[serde(default)]
    pub ranking: Option<MissionRankingOptions>,
    #[serde(default)]
    pub waypoints: Vec<WaypointDefinition>,
    #[serde(default, rename = "startOwners")]
    pub start_owners: Vec<StartOwnerDefinition>,
    #[serde(default)]
    pub texts: Vec<TextDefinition>,
    #[serde(default)]
    pub assets: Vec<ra2_map_document::campaign::Asset>,
    #[serde(default, rename = "nativeDefinitions")]
    pub native_definitions: Vec<NativeDefinition>,
    #[serde(default)]
    pub aliases: Vec<Alias>,
    #[serde(default)]
    pub origins: Vec<Origin>,
    pub teams: Vec<Team>,
    #[serde(default, rename = "taskForces")]
    pub task_forces: Vec<TaskForce>,
    #[serde(default)]
    pub scripts: Vec<Script>,
    #[serde(default)]
    pub tags: Vec<Tag>,
    #[serde(default)]
    pub bindings: Vec<Binding>,
    pub flags: Vec<Flag>,
    pub rules: Vec<Rule>,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MissionMode {
    #[default]
    SinglePlayer,
    Multiplayer,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TeamOwner {
    House { id: String },
    StartPlayer { id: u32 },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ActionHouse {
    Country { id: String },
    StartPlayer { id: u32 },
    RetainedHouse { id: String },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HouseDefinition {
    pub id: String,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub country: Option<Reference>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default, rename = "uiName")]
    pub ui_name: Option<Reference>,
    #[serde(default)]
    pub edge: Option<HouseEdge>,
    #[serde(default, rename = "playerControl")]
    pub player_control: Option<bool>,
    #[serde(default)]
    pub credits: Option<i32>,
    #[serde(default)]
    pub iq: Option<i32>,
    #[serde(default, rename = "techLevel")]
    pub tech_level: Option<i32>,
    #[serde(default)]
    pub allies: Option<Vec<Reference>>,
    #[serde(default, rename = "basePlan")]
    pub base_plan: Option<BasePlanOptions>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BasePlanOptions {
    pub nodes: Vec<BasePlanNode>,
    #[serde(default, rename = "percentBuilt")]
    pub percent_built: Option<i32>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HouseEdge {
    Automatic,
    North,
    East,
    South,
    West,
    Air,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AiTriggerDefinition {
    pub id: String,
    pub primary: Reference,
    #[serde(default)]
    pub owner: Option<Reference>,
    pub condition: i32,
    #[serde(default, rename = "objectType")]
    pub object_type: Option<Reference>,
    #[serde(default)]
    pub amount: Option<i32>,
    #[serde(default)]
    pub comparison: Option<i32>,
    #[serde(default)]
    pub weight: Option<u32>,
    #[serde(default, rename = "minWeight")]
    pub min_weight: Option<u32>,
    #[serde(default, rename = "maxWeight")]
    pub max_weight: Option<u32>,
    #[serde(default)]
    pub multiplayer: Option<bool>,
    #[serde(default)]
    pub side: Option<u8>,
    #[serde(default)]
    pub secondary: Option<Reference>,
    #[serde(default)]
    pub difficulties: Option<Vec<Difficulty>>,
    #[serde(default)]
    pub enabled: Option<bool>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MissionBasicOptions {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default, rename = "homeCell")]
    pub home_cell: Option<Waypoint>,
    #[serde(default, rename = "altHomeCell")]
    pub alt_home_cell: Option<Waypoint>,
    #[serde(default)]
    pub intro: Option<Reference>,
    #[serde(default)]
    pub brief: Option<Reference>,
    #[serde(default)]
    pub action: Option<Reference>,
    #[serde(default)]
    pub theme: Option<Reference>,
    #[serde(default)]
    pub win: Option<Reference>,
    #[serde(default)]
    pub lose: Option<Reference>,
    #[serde(default, rename = "postScore")]
    pub post_score: Option<Reference>,
    #[serde(default, rename = "preMapSelect")]
    pub pre_map_select: Option<Reference>,
    #[serde(default, rename = "endOfGame")]
    pub end_of_game: Option<bool>,
    #[serde(default, rename = "oneTimeOnly")]
    pub one_time_only: Option<bool>,
    #[serde(default, rename = "skipScore")]
    pub skip_score: Option<bool>,
    #[serde(default, rename = "ignoreGlobalAiTriggers")]
    pub ignore_global_ai_triggers: Option<bool>,
    #[serde(default, rename = "freeRadar")]
    pub free_radar: Option<bool>,
    #[serde(default, rename = "fillSilos")]
    pub fill_silos: Option<bool>,
    #[serde(default, rename = "tiberiumGrowthEnabled")]
    pub tiberium_growth_enabled: Option<bool>,
    #[serde(default, rename = "iceGrowthEnabled")]
    pub ice_growth_enabled: Option<bool>,
    #[serde(default, rename = "veinGrowthEnabled")]
    pub vein_growth_enabled: Option<bool>,
    #[serde(default, rename = "trainCrate")]
    pub train_crate: Option<bool>,
    #[serde(default, rename = "truckCrate")]
    pub truck_crate: Option<bool>,
    #[serde(default, rename = "timerInherit")]
    pub timer_inherit: Option<bool>,
    #[serde(default, rename = "carryOverMoneyPercent")]
    pub carry_over_money_percent: Option<f64>,
    #[serde(default, rename = "carryOverCap")]
    pub carry_over_cap: Option<i32>,
    #[serde(default, rename = "skipMapSelect")]
    pub skip_map_select: Option<bool>,
    #[serde(default, rename = "nextScenario")]
    pub next_scenario: Option<String>,
    #[serde(default, rename = "altNextScenario")]
    pub alt_next_scenario: Option<String>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionRankingOptions {
    #[serde(default)]
    pub par_time: Option<RankingParTime>,
    #[serde(default)]
    pub under_par_title: Option<Reference>,
    #[serde(default)]
    pub under_par_message: Option<Reference>,
    #[serde(default)]
    pub over_par_title: Option<Reference>,
    #[serde(default)]
    pub over_par_message: Option<Reference>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionSpecialFlagsOptions {
    #[serde(default)]
    pub inert: Option<bool>,
    #[serde(default)]
    pub fog_of_war: Option<bool>,
    #[serde(default)]
    pub harvester_immune: Option<bool>,
    #[serde(default)]
    pub destroyable_bridges: Option<bool>,
    #[serde(default)]
    pub tiberium_grows: Option<bool>,
    #[serde(default)]
    pub tiberium_spreads: Option<bool>,
    #[serde(default)]
    pub initial_veteran: Option<bool>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionLightingOptions {
    #[serde(default)]
    pub normal: Option<MissionLightingProfile>,
    #[serde(default)]
    pub ion: Option<MissionLightingProfile>,
    #[serde(default)]
    pub dominator: Option<MissionLightingProfile>,
    #[serde(default)]
    pub dominator_ambient_change_rate: Option<f64>,
    #[serde(default)]
    pub nuke_ambient_change_rate: Option<f64>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MissionLightingProfile {
    #[serde(default)]
    pub ambient: Option<f64>,
    #[serde(default)]
    pub red: Option<f64>,
    #[serde(default)]
    pub green: Option<f64>,
    #[serde(default)]
    pub blue: Option<f64>,
    #[serde(default)]
    pub ground: Option<f64>,
    #[serde(default)]
    pub level: Option<f64>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RankingParTime {
    #[serde(default)]
    pub easy: Option<u32>,
    #[serde(default)]
    pub normal: Option<u32>,
    #[serde(default)]
    pub hard: Option<u32>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WaypointDefinition {
    pub id: u32,
    pub x: u16,
    pub y: u16,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StartOwnerDefinition {
    pub object: Reference,
    pub player: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TextDefinition {
    pub id: String,
    pub label: String,
    pub value: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BasePlanNode {
    Building {
        building: Reference,
        x: u16,
        y: u16,
        #[serde(default)]
        rebuild: Option<bool>,
    },
    Control {
        control: i32,
        x: u16,
        y: u16,
    },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub kind: String,
    pub id: String,
}
impl Reference {
    pub(crate) fn check(&self, kind: &str) -> Result<&str> {
        ensure!(
            self.kind == kind,
            "引用 {} 应为 {kind}，实际为 {}",
            self.id,
            self.kind
        );
        Ok(&self.id)
    }
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Waypoint {
    pub kind: String,
    pub id: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub r#type: Reference,
    pub count: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Team {
    pub id: String,
    pub house: TeamOwner,
    #[serde(default)]
    pub units: Option<Vec<Unit>>,
    #[serde(default)]
    pub orders: Option<Vec<Order>>,
    #[serde(default, rename = "taskForce")]
    pub task_force: Option<Reference>,
    #[serde(default)]
    pub script: Option<Reference>,
    pub waypoint: Waypoint,
    #[serde(default)]
    pub tag: Option<Reference>,
    #[serde(default)]
    pub max: Option<i32>,
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default, rename = "veteranLevel")]
    pub veteran_level: Option<i32>,
    #[serde(default)]
    pub group: Option<i32>,
    #[serde(default)]
    pub autocreate: Option<bool>,
    #[serde(default)]
    pub full: Option<bool>,
    #[serde(default)]
    pub recruiter: Option<bool>,
    #[serde(default, rename = "isBaseDefense")]
    pub is_base_defense: Option<bool>,
    #[serde(default)]
    pub suicide: Option<bool>,
    #[serde(default)]
    pub aggressive: Option<bool>,
    #[serde(default)]
    pub reinforce: Option<bool>,
    #[serde(default, rename = "areTeamMembersRecruitable")]
    pub are_team_members_recruitable: Option<bool>,
    #[serde(default, rename = "onTransOnly")]
    pub on_trans_only: Option<bool>,
    #[serde(default, rename = "avoidThreats")]
    pub avoid_threats: Option<bool>,
    #[serde(default)]
    pub annoyance: Option<bool>,
    #[serde(default, rename = "guardSlower")]
    pub guard_slower: Option<bool>,
    #[serde(default, rename = "transportsReturnOnUnload")]
    pub transports_return_on_unload: Option<bool>,
    #[serde(default, rename = "mindControlDecision")]
    pub mind_control_decision: Option<i32>,
    #[serde(default)]
    pub droppod: Option<bool>,
    #[serde(default, rename = "onlyTargetHouseEnemy")]
    pub only_target_house_enemy: Option<bool>,
    #[serde(default)]
    pub loadable: Option<bool>,
    #[serde(default)]
    pub whiner: Option<bool>,
    #[serde(default, rename = "useTransportOrigin")]
    pub use_transport_origin: Option<bool>,
    #[serde(default, rename = "transportWaypoint")]
    pub transport_waypoint: Option<Waypoint>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tag {
    pub id: String,
    pub rule: Reference,
    #[serde(default)]
    pub repeat: Option<u8>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Binding {
    Object { object: Reference, tag: Reference },
    Cell { x: u16, y: u16, tag: Reference },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskForce {
    pub id: String,
    pub units: Vec<Unit>,
    #[serde(default = "default_group")]
    pub group: i32,
}
fn default_group() -> i32 {
    -1
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Script {
    pub id: String,
    pub orders: Vec<Order>,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Order {
    FollowFriendlies,
    IdleAnimation { animation: u8 },
    Scout,
    FireSale,
    CenterCamera { velocity: i32 },
    AttackEnemyBuilding { building: Reference, mode: u8 },
    MoveToEnemyBuilding { building: Reference, mode: u8 },
    MoveToOwnBuilding { building: Reference, mode: u8 },
    ChronoshiftToBuilding { building: Reference, mode: u8 },
    PlayAnimation { animation: Reference, loops: u16 },
    GatherAtEnemyBase,
    RegroupAtBase,
    IronCurtain,
    ChronoshiftToQuarry { quarry: u8 },
    AttackWaypointBuilding { waypoint: Waypoint },
    EnterGrinder,
    EnterTankBunker,
    EnterBioReactor,
    EnterBattleBunker,
    EnterNearestOccupiable,
    ChangeCountry { country: Reference },
    ChangeScript { script: Reference },
    Attack { quarry: u8 },
    MoveToCell { x: u16, y: u16 },
    Mission { mission: u8 },
    Face { direction: u8 },
    Unload { mode: u8 },
    SpyWaypoint { waypoint: Waypoint },
    Label { label: String },
    Jump { label: String },
    WaitForever,
    Deploy,
    Scatter,
    Panic,
    Calm,
    Load,
    WaitLoaded,
    Next,
    Succeed,
    KillMembers,
    RemoveMembers,
    WinPlayer,
    LosePlayer,
    SetFlag { flag: Reference },
    ClearFlag { flag: Reference },
    SetLocal { flag: VariableReference },
    ClearLocal { flag: VariableReference },
    SetGlobal { flag: VariableReference },
    ClearGlobal { flag: VariableReference },
    PlaySound { sound: Reference },
    PlayMovie { movie: Reference },
    PlayMusic { theme: Reference },
    Flash { frames: i32 },
    MoveTo { waypoint: Waypoint },
    AttackWaypoint { waypoint: Waypoint },
    Patrol { waypoint: Waypoint },
    Guard { seconds: u32 },
    Hunt,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Flag {
    pub id: String,
    pub initial: bool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub when: Event,
    pub once: bool,
    pub actions: Vec<Action>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub owner: Option<Reference>,
    #[serde(default = "default_difficulties")]
    pub difficulties: Vec<Difficulty>,
    #[serde(default)]
    pub linked: Option<Reference>,
    #[serde(default, rename = "transferOnCapture")]
    pub transfer_on_capture: bool,
}
fn default_enabled() -> bool {
    true
}
fn default_difficulties() -> Vec<Difficulty> {
    vec![Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "camelCase")]
pub enum Difficulty {
    Easy,
    Normal,
    Hard,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VariableReference {
    pub kind: String,
    pub id: u32,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Event {
    DamagedByHouse {
        object: Reference,
        house: Reference,
    },
    BuildingBuilt {
        building: Reference,
    },
    VehicleBuilt {
        r#type: Reference,
    },
    InfantryBuilt {
        r#type: Reference,
    },
    AircraftBuilt {
        r#type: Reference,
    },
    BuildingExists {
        building: Reference,
    },
    BuildingAbsent {
        building: Reference,
    },
    SpyDisguisedAsInfantry {
        r#type: Reference,
    },
    RandomTime {
        seconds: u32,
    },
    CreditsAtLeast {
        amount: u32,
    },
    CreditsAtMost {
        amount: u32,
    },
    BuildingsLost {
        count: u32,
    },
    UnitsLost {
        count: u32,
    },
    NoFactories,
    AmbientAtMost {
        value: i32,
    },
    AmbientAtLeast {
        value: i32,
    },
    CountryDiscovered {
        country: Reference,
    },
    CountryUnitsDestroyed {
        country: Reference,
    },
    CountryBuildingsDestroyed {
        country: Reference,
    },
    CountryAllDestroyed {
        country: Reference,
    },
    CountryNavyDestroyed {
        country: Reference,
    },
    CountryLandDestroyed {
        country: Reference,
    },
    LowPower {
        country: Reference,
    },
    FullPower {
        country: Reference,
    },
    CrossedHorizontal {
        country: Reference,
    },
    CrossedVertical {
        country: Reference,
    },
    EnteredOrOverflown {
        country: Reference,
    },
    SpyDisguisedAsCountry {
        country: Reference,
    },
    TeamLeftMap {
        team: Reference,
    },
    Discovered {
        object: Reference,
    },
    Attacked {
        object: Reference,
    },
    DestroyedAny {
        object: Reference,
    },
    Selected {
        object: Reference,
    },
    SpotlightDetected {
        object: Reference,
    },
    FirstDamagedBySource {
        object: Reference,
    },
    HalfHealthBySource {
        object: Reference,
    },
    RedHealthBySource {
        object: Reference,
    },
    FirstDamaged {
        object: Reference,
    },
    HalfHealth {
        object: Reference,
    },
    RedHealth {
        object: Reference,
    },
    DestroyedOrRemoved {
        object: Reference,
    },
    PickedUpCrate {
        object: Reference,
    },
    AnyCratePickedUp,
    Entered {
        country: Reference,
    },
    EnteredZone {
        country: Reference,
    },
    All {
        events: Vec<Event>,
    },
    Any {
        events: Vec<Event>,
    },
    ScenarioTime {
        seconds: u32,
    },
    EnabledTime {
        seconds: u32,
    },
    CountdownElapsed,
    TagNotification,
    BridgeDestroyed,
    NearWaypoint {
        object: Reference,
        waypoint: Waypoint,
    },
    TechnoExists {
        r#type: Reference,
        count: u32,
    },
    TechnoAbsent {
        r#type: Reference,
    },
    Destroyed {
        object: Reference,
    },
    FlagSet {
        flag: Reference,
    },
    FlagClear {
        flag: Reference,
    },
    LocalSet {
        flag: VariableReference,
    },
    LocalClear {
        flag: VariableReference,
    },
    GlobalSet {
        flag: VariableReference,
    },
    GlobalClear {
        flag: VariableReference,
    },
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Action {
    SetSpotlightBehavior {
        mode: u8,
    },
    PlayAnimationAt {
        animation: Reference,
        at: Waypoint,
    },
    ExplosionAt {
        weapon: Reference,
        at: Waypoint,
    },
    CreateParticleSystem {
        system: Reference,
        at: Waypoint,
    },
    SetAmbient {
        percent: i32,
    },
    SetAmbientStep {
        value: f32,
    },
    SetAmbientRate {
        value: f32,
    },
    SetLightingRed {
        percent: i32,
    },
    SetLightingGreen {
        percent: i32,
    },
    SetLightingBlue {
        percent: i32,
    },
    CombatFlash {
        at: Waypoint,
        strength: u8,
    },
    DamageBridgeAt {
        at: Waypoint,
    },
    MeteorShower {
        at: Waypoint,
        intensity: i32,
    },
    ReduceTiberiumAt {
        at: Waypoint,
    },
    IonBlastAt {
        at: Waypoint,
    },
    NukeAt {
        at: Waypoint,
    },
    ChemicalStrikeAt {
        at: Waypoint,
    },
    SetVeinGrowth {
        enabled: bool,
    },
    SetTiberiumGrowth {
        enabled: bool,
    },
    SetIceGrowth {
        enabled: bool,
    },
    ToggleTrainCrate,
    ClearSmudges,
    MarkParticlesDone {
        at: Waypoint,
    },
    FireSale {
        house: ActionHouse,
    },
    EnableAutocreate {
        house: ActionHouse,
    },
    SetAutoBaseBuilding {
        enabled: bool,
    },
    MakeAlly {
        house: ActionHouse,
    },
    MakeEnemy {
        house: ActionHouse,
    },
    SetAiTriggerRatio {
        value: i32,
    },
    SetAircraftRatio {
        value: i32,
    },
    SetInfantryRatio {
        value: i32,
    },
    SetVehicleRatio {
        value: i32,
    },
    SetSuperTargetKind {
        value: i32,
    },
    SetTechLevel {
        r#type: Reference,
        level: i32,
    },
    Cheer {
        house: ActionHouse,
    },
    EndScenario,
    DeleteRule {
        rule: Reference,
    },
    DeleteTag {
        tag: Reference,
    },
    PlayMovie {
        movie: Reference,
    },
    PlayRadarMovie {
        movie: Reference,
        pause: bool,
    },
    PlaySoundAt {
        sound: Reference,
        at: Waypoint,
    },
    PlaySoundAtRandomWaypoint {
        sound: Reference,
    },
    StopSoundAt {
        at: Waypoint,
    },
    LockInput,
    UnlockInput,
    PauseScene {
        duration: u32,
    },
    ChronoScreen {
        duration: u32,
    },
    TalkBubble {
        team: Reference,
        bubble: u8,
    },
    ClearTalkBubble,
    DestroyAttached,
    SellAttachedBuildings,
    SetAttachedBuildingsOnline {
        online: bool,
    },
    MakeAttachedInfantryPsycho,
    WakeAttached,
    WakeOtherHouseSleepers,
    WakeHouseHarmless,
    WakeGroup {
        group: i32,
    },
    CreateBuilding {
        building: Reference,
        at: Waypoint,
    },
    ReleaseGarrison,
    DestroyHouseObjects {
        house: ActionHouse,
        subset: u8,
    },
    ControlHouseBuildings {
        house: ActionHouse,
        restore: bool,
    },
    RestoreInitialObjects {
        house: ActionHouse,
        #[serde(rename = "buildingsOnly")]
        buildings_only: bool,
    },
    ChangeAllHouseObjectsOwner {
        house: ActionHouse,
    },
    FlashHouseBuildings {
        building: Reference,
        frames: i32,
    },
    GrantSuperWeapon {
        weapon: Reference,
        #[serde(rename = "oneShot")]
        one_shot: bool,
    },
    SetSuperCharge {
        weapon: Reference,
        percent: u8,
    },
    SetSuperRecharge {
        weapon: Reference,
        frames: u32,
    },
    ResetSuperRecharge {
        weapon: Reference,
    },
    ResetSuperWeapon {
        weapon: Reference,
    },
    SetSuperTarget {
        at: Waypoint,
    },
    ClearSuperTarget,
    SetSuperDefense {
        at: Waypoint,
    },
    ClearSuperDefense,
    LightningAt {
        at: Waypoint,
    },
    StartLightningStorm {
        frames: u32,
    },
    StopLightningStorm,
    LightningStormAt {
        at: Waypoint,
    },
    ChronoshiftHouse {
        at: Waypoint,
    },
    IronCurtainAt {
        at: Waypoint,
    },
    CenterCamera {
        at: Waypoint,
        velocity: i32,
    },
    JumpCamera {
        at: Waypoint,
    },
    HomeCamera,
    RevealAt {
        at: Waypoint,
    },
    RevealZone {
        at: Waypoint,
    },
    ConcealAt {
        at: Waypoint,
    },
    GrowShroud,
    ReshroudMap,
    VisibleMapArea {
        x: u16,
        y: u16,
        width: u16,
        height: u16,
    },
    RadarEvent {
        event: u8,
        at: Waypoint,
    },
    SidebarTab {
        tab: u8,
    },
    FlashCameo {
        r#type: Reference,
        frames: i32,
    },
    RadarOutage {
        frames: i32,
    },
    AllToHunt {
        house: ActionHouse,
    },
    StartProduction {
        house: ActionHouse,
    },
    EnableAi {
        house: ActionHouse,
    },
    DisableAi {
        house: ActionHouse,
    },
    ChangeAttachedOwner {
        house: ActionHouse,
    },
    WinCountry {
        country: Reference,
    },
    LoseCountry {
        country: Reference,
    },
    PlaySound {
        sound: Reference,
    },
    PlayMusic {
        theme: Reference,
    },
    PlayEva {
        eva: Reference,
    },
    DeleteTeam {
        team: Reference,
    },
    ReinforceDefault {
        team: Reference,
    },
    ChronoReinforce {
        team: Reference,
        at: Waypoint,
    },
    FlashTeam {
        team: Reference,
        frames: i32,
    },
    RevealMap,
    SetAlternateBase {
        at: Waypoint,
    },
    ClearAlternateBase,
    DropZone {
        at: Waypoint,
    },
    CreateCrate {
        #[serde(rename = "crate")]
        reward: u8,
        at: Waypoint,
    },
    EnableRule {
        rule: Reference,
    },
    DisableRule {
        rule: Reference,
    },
    ForceRule {
        rule: Reference,
    },
    Reinforce {
        team: Reference,
        at: Waypoint,
    },
    Recruit {
        team: Reference,
    },
    Message {
        text: Reference,
    },
    SetCountdown {
        seconds: u32,
    },
    StartCountdown,
    StopCountdown,
    ExtendCountdown {
        seconds: u32,
    },
    ShortenCountdown {
        seconds: u32,
    },
    SetCountdownTitle {
        text: Reference,
    },
    SetFlag {
        flag: Reference,
    },
    ClearFlag {
        flag: Reference,
    },
    SetLocal {
        flag: VariableReference,
    },
    ClearLocal {
        flag: VariableReference,
    },
    SetGlobal {
        flag: VariableReference,
    },
    ClearGlobal {
        flag: VariableReference,
    },
    WinPlayer,
    LosePlayer,
}

pub use crate::catalog::Catalog;

#[path = "conditions.rs"]
mod conditions;
#[path = "crate_lottery.rs"]
mod crate_lottery;
#[path = "definitions.rs"]
mod definitions;
#[path = "extra_actions.rs"]
mod extra_actions;
#[path = "references.rs"]
mod references;
#[path = "rules.rs"]
mod rules;
#[path = "scenario.rs"]
mod scenario;
#[path = "tags.rs"]
mod tags;

const META: &str = "EditorMissionScript";
const MAX_MANAGED_FIELDS: usize = 16384;
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Field {
    section: String,
    key: String,
    value: String,
    before: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Attachment {
    object: String,
    type_id: String,
    before: String,
    after: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    mission: String,
    fields: Vec<Field>,
    attachments: Vec<Attachment>,
    #[serde(default)]
    waypoints: Vec<WaypointDefinition>,
    #[serde(default)]
    start_owners: Vec<ManagedStartOwner>,
    #[serde(default)]
    rebuilds: Vec<ManagedRebuild>,
    flags: BTreeMap<String, u32>,
    #[serde(default)]
    texts: BTreeMap<String, String>,
    #[serde(default)]
    assets: Vec<ra2_map_document::campaign::Asset>,
    #[serde(default)]
    context: Option<BuildContext>,
    #[serde(default)]
    aliases: Vec<ManagedAlias>,
    #[serde(default)]
    sources: Vec<SourceMapping>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDefinition {
    pub kind: String,
    pub id: String,
    pub native: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Alias {
    pub kind: String,
    pub id: String,
    pub target: String,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Origin {
    pub kind: String,
    pub id: String,
    pub module: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManagedAlias {
    kind: String,
    id: String,
    target: String,
    value: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SourceMapping {
    pub kind: String,
    pub logical: String,
    pub native: String,
    pub module: Option<String>,
}

/// Host supplied provenance. The script evaluator cannot populate this record.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BuildContext {
    pub root: String,
    pub entry: String,
    pub sdk: u64,
    pub profile: String,
    pub inputs: BTreeMap<String, u64>,
    #[serde(default)]
    pub registries: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign: Option<CampaignBuildContext>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CampaignBuildContext {
    /// Campaign entry relative to BuildContext::root.
    pub entry: String,
    /// Output mission filename; a script may be reused by several missions.
    pub mission: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManagedStartOwner {
    object: String,
    type_id: String,
    before_owner: String,
    after_owner: String,
    before_tag: String,
    after_tag: String,
    player: u8,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManagedRebuild {
    object: String,
    type_id: String,
    before: String,
    after: String,
}

/// Immutable prepared map bytes are applied only to the exact input revision.
pub struct Build {
    pub document: u64,
    pub state: u64,
    pub bytes: Vec<u8>,
    pub field_count: usize,
    pub rule_count: usize,
    pub team_count: usize,
}
impl Build {
    pub fn apply(&self, doc: &mut MapDocument) -> Result<bool> {
        ensure!(
            (doc.id, doc.state_id) == (self.document, self.state),
            "地图已变化，请重新构建任务"
        );
        doc.replace_bytes(self.bytes.clone(), "应用 TypeScript 任务")
    }
}

fn object(doc: &MapDocument, id: &str) -> Result<(u64, Record, usize)> {
    let (section, key) = id.split_once(':').context("对象引用必须是 section:key")?;
    let column = startup_owner::tag_column(section).context("对象不支持 Tag")?;
    let matches: Vec<_> = doc
        .records()
        .into_iter()
        .filter(|(_, r)| r.section == section && r.key == key)
        .collect();
    ensure!(matches.len() == 1, "对象 {id} 不存在或有歧义");
    let (record_id, record) = matches.into_iter().next().unwrap();
    ensure!(
        doc.base
            .raw
            .entries(section)
            .filter(|(_, r)| r.key.as_deref() == Some(key))
            .count()
            <= 1,
        "对象 {id} 存在重复记录，请先消歧"
    );
    Ok((record_id, record, column))
}

fn read_manifest(doc: &MapDocument) -> Result<Manifest> {
    let entries = doc.ini_fields(META);
    if entries.is_empty() {
        return Ok(Manifest::default());
    }
    ensure!(
        doc.ini_text(META, "Version") == "1",
        "不支持的任务脚本元数据版本"
    );
    let count: usize = doc
        .ini_text(META, "Chunks")
        .parse()
        .context("任务归属清单损坏")?;
    ensure!(count <= 32768, "任务归属清单过大");
    let mut hex = String::new();
    for index in 0..count {
        hex.push_str(&doc.ini_text(META, &format!("Data{index}")));
    }
    ensure!(
        hex.len().is_multiple_of(2) && hex.is_ascii(),
        "任务归属清单编码损坏"
    );
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16))
        .collect::<std::result::Result<_, _>>()?;
    let result: Manifest = serde_json::from_slice(&bytes)?;
    validate_assets(&result.assets)?;
    ensure!(
        result.aliases.len() <= 2048 && result.sources.len() <= 4096,
        "别名/来源清单超过容量"
    );
    if let Some(context) = &result.context {
        ensure!(
            context.inputs.len() <= 256
                && context.root.len() <= 32768
                && context.entry.len() <= 32768
                && context.inputs.keys().all(|p| p.len() <= 32768),
            "构建记录超出容量"
        );
    }
    ensure!(
        result.version == 1
            && result.fields.len() <= MAX_MANAGED_FIELDS
            && result.attachments.len() <= 1024
            && result.waypoints.len() <= 672
            && result.start_owners.len() <= 1024
            && result.rebuilds.len() <= ra2_map_document::base_nodes::MAX_NODES,
        "任务归属清单无效"
    );
    let mut waypoint_ids = BTreeSet::new();
    ensure!(result.texts.len() <= 1024, "任务文本清单过大");
    let mut text_labels = BTreeSet::new();
    ensure!(
        result.texts.iter().all(|(label, value)| label.len() == 12
            && label.starts_with("TSTX")
            && label[4..].bytes().all(|b| b.is_ascii_hexdigit())
            && value.len() <= 65536
            && !value.contains('\0')
            && text_labels.insert(label.to_ascii_uppercase())),
        "任务文本清单无效"
    );
    ensure!(
        result
            .waypoints
            .iter()
            .all(|waypoint| (30..=701).contains(&waypoint.id) && waypoint_ids.insert(waypoint.id)),
        "任务航点归属清单无效"
    );
    let mut start_owner_objects = BTreeSet::new();
    ensure!(
        result.start_owners.iter().all(|owner| {
            owner.player < 30 && start_owner_objects.insert(owner.object.as_str())
        }),
        "任务起点对象归属清单无效"
    );
    let mut rebuild_objects = BTreeSet::new();
    ensure!(
        result
            .rebuilds
            .iter()
            .all(|entry| entry.object.starts_with("Structures:")
                && matches!(entry.after.as_str(), "0" | "1")
                && rebuild_objects.insert(entry.object.as_str())),
        "任务建筑重建归属清单无效"
    );
    for field in &result.fields {
        let valid = match field.section.as_str() {
            "Basic" => matches!(
                field.key.as_str(),
                "Player"
                    | "MultiplayerOnly"
                    | "Name"
                    | "HomeCell"
                    | "AltHomeCell"
                    | "Intro"
                    | "Brief"
                    | "Action"
                    | "Theme"
                    | "Win"
                    | "Lose"
                    | "PostScore"
                    | "PreMapSelect"
                    | "EndOfGame"
                    | "OneTimeOnly"
                    | "SkipScore"
                    | "Author"
                    | "IgnoreGlobalAITriggers"
                    | "FreeRadar"
                    | "FillSilos"
                    | "TiberiumGrowthEnabled"
                    | "IceGrowthEnabled"
                    | "VeinGrowthEnabled"
                    | "TrainCrate"
                    | "TruckCrate"
                    | "TimerInherit"
                    | "CarryOverMoney"
                    | "CarryOverCap"
                    | "SkipMapSelect"
                    | "NextScenario"
                    | "AltNextScenario"
            ),
            "Ranking" => scenario::is_ranking_key(&field.key),
            "SpecialFlags" => scenario::is_special_flag_key(&field.key),
            "Lighting" => scenario::is_lighting_key(&field.key),
            s if crate_lottery::is_managed_field(s, &field.key) => true,
            "Houses" => field.key.parse::<u32>().is_ok() && field.value.starts_with("TSHS"),
            "VariableNames" => field.key.parse::<u8>().is_ok_and(|n| n < 100),
            "CellTags" => field.key.parse::<u32>().is_ok(),
            s if mission::ids(doc, Kind::House).iter().any(|id| id == s) => {
                matches!(
                    field.key.as_str(),
                    "Country"
                        | "Color"
                        | "PlayerControl"
                        | "IQ"
                        | "Credits"
                        | "TechLevel"
                        | "Allies"
                        | "NodeCount"
                        | "PercentBuilt"
                        | "UIName"
                        | "Edge"
                ) || field.key.parse::<usize>().is_ok()
                    || (s.starts_with("TSHS") && matches!(s.len(), 19 | 20) && field.key == "Name")
            }
            "Triggers"
            | "Events"
            | "Actions"
            | "Tags"
            | "TaskForces"
            | "ScriptTypes"
            | "TeamTypes"
            | "AITriggerTypes"
            | "AITriggerTypesEnable" => field.key.starts_with("TS"),
            s => s.starts_with("TS") && s.len() == 20,
        };
        ensure!(
            valid,
            "任务归属清单包含非托管字段 {}/{}",
            field.section,
            field.key
        );
    }
    Ok(result)
}
fn restore(doc: &mut MapDocument, old: &Manifest) -> Result<()> {
    for rebuild in &old.rebuilds {
        let (id, record, _) = object(doc, &rebuild.object)?;
        ensure!(
            record.section == "Structures"
                && record.type_id() == rebuild.type_id
                && record.fields().get(8) == Some(&rebuild.after),
            "建筑 {} 的重建配置已被手工修改",
            rebuild.object
        );
        doc.set(
            Target::Record(id),
            Some(Value::Record(
                record.patched(&[(8, rebuild.before.clone())])?,
            )),
        )?;
    }
    for attachment in &old.attachments {
        let (id, record, column) = object(doc, &attachment.object)?;
        ensure!(
            record.type_id() == attachment.type_id
                && record.fields().get(column) == Some(&attachment.after),
            "对象 {} 的任务绑定已被修改，请先处理冲突",
            attachment.object
        );
        doc.set(
            Target::Record(id),
            Some(Value::Record(
                record.patched(&[(column, attachment.before.clone())])?,
            )),
        )?;
    }
    for owner in &old.start_owners {
        let (id, record, column) = object(doc, &owner.object)?;
        ensure!(
            record.type_id() == owner.type_id
                && record.fields().first() == Some(&owner.after_owner)
                && record.fields().get(column) == Some(&owner.after_tag)
                && startup_owner::get(doc, &record) == Some(owner.player),
            "对象 {} 的开局归属已被修改，请先处理冲突",
            owner.object
        );
        startup_owner::set(doc, id, None)?;
        let restored = doc.records().remove(&id).context("开局归属对象已不存在")?;
        doc.set(
            Target::Record(id),
            Some(Value::Record(restored.patched(&[
                (0, owner.before_owner.clone()),
                (column, owner.before_tag.clone()),
            ])?)),
        )?;
    }
    for waypoint in &old.waypoints {
        let matches: Vec<_> = doc
            .records()
            .into_iter()
            .filter(|(_, record)| {
                record.section == "Waypoints" && record.key == waypoint.id.to_string()
            })
            .collect();
        ensure!(
            matches.len() == 1,
            "任务生成航点 {} 已被删除或重复",
            waypoint.id
        );
        let (_, record) = matches.into_iter().next().unwrap();
        ensure!(
            record.cell() == Some([waypoint.x, waypoint.y]),
            "任务生成航点 {} 已被手工移动，请先处理冲突",
            waypoint.id
        );
    }
    // Extra fields added by another tool must not become orphaned on rebuild.
    let generated_sections: BTreeSet<_> = old
        .fields
        .iter()
        .filter(|f| {
            f.section.starts_with("TS") && !crate_lottery::is_managed_field(&f.section, &f.key)
        })
        .map(|f| &f.section)
        .collect();
    for section in generated_sections {
        ensure!(
            doc.ini_entries(section).iter().all(|(_, key, _)| old
                .fields
                .iter()
                .any(|f| &f.section == section && &f.key == key)),
            "生成项 {section} 存在手工新增字段，请先处理冲突"
        );
    }
    for field in &old.fields {
        ensure!(
            doc.ini_value(&field.section, &field.key).as_deref() == Some(field.value.as_bytes()),
            "生成项 {}/{} 已被手工修改，请先处理冲突",
            field.section,
            field.key
        );
        doc.set_ini(
            &field.section,
            &field.key,
            field.before.as_ref().map(|s| s.as_bytes().to_vec()),
        )?;
    }
    Ok(())
}

fn identifier(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "逻辑名称须为 1～64 位 ASCII 字母、数字或下划线：{id}"
    );
    Ok(())
}
fn physical(mission: &str, logical: &str, kind: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for b in mission.bytes().chain(*b"/").chain(logical.bytes()) {
        hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    let mut id = format!("TS{kind}{hash:016X}");
    if kind == "HS" {
        // ReadHousesList uses a 0x14-byte buffer, including its terminator.
        // Other definition readers admit the existing 20-byte identities.
        id.truncate(19);
    }
    id
}
fn text_label(mission: &str, logical: &str) -> String {
    let mut hash = 0x811c9dc5u32;
    for byte in mission.bytes().chain(*b"/").chain(logical.bytes()) {
        hash = (hash ^ u32::from(byte)).wrapping_mul(0x01000193);
    }
    format!("TSTX{hash:08X}")
}

/// Saved SDK resource declarations, validated before IO prepares sidecar files.
pub fn declared_texts(doc: &MapDocument) -> Result<BTreeMap<String, String>> {
    Ok(read_manifest(doc)?.texts)
}

pub fn build_context(doc: &MapDocument) -> Result<Option<BuildContext>> {
    Ok(read_manifest(doc)?.context)
}
pub fn source_mappings(doc: &MapDocument) -> Result<Vec<SourceMapping>> {
    Ok(read_manifest(doc)?.sources)
}
pub fn check_product(doc: &MapDocument) -> Result<()> {
    let manifest = read_manifest(doc)?;
    let mut scratch = doc.fork();
    restore(&mut scratch, &manifest)?;
    for alias in &manifest.aliases {
        let value = match alias.kind.as_str() {
            "object" => object(&scratch, &alias.target)?.1.fields().join(","),
            "waypoint" => scratch.ini_text("Waypoints", &alias.target),
            _ => anyhow::bail!("任务别名类型无效"),
        };
        ensure!(
            value == alias.value,
            "别名 {} 的目标记录已变化，请重新选择并构建",
            alias.id
        );
    }
    Ok(())
}

/// Native numeric operands depend on physical registration order, not display lists.
pub fn registry_fingerprints(bytes: &[u8]) -> Result<BTreeMap<String, u64>> {
    let raw = ra2_map_document::raw_ini::RawIni::parse(bytes.to_vec().into())?;
    let mut result = BTreeMap::new();
    for section in [
        "Countries",
        "BuildingTypes",
        "VehicleTypes",
        "InfantryTypes",
        "AircraftTypes",
        "SuperWeaponTypes",
        "Animations",
        "WeaponTypes",
        "ParticleSystems",
        "Warheads",
        "ScriptTypes",
        "Houses",
        "Waypoints",
    ] {
        let mut hash = 0xcbf29ce484222325u64;
        for (i, line) in raw.entries(section) {
            for b in line
                .key
                .as_ref()
                .unwrap()
                .bytes()
                .chain([0])
                .chain(raw.value(i).iter().copied())
                .chain([0])
            {
                hash = (hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
            }
        }
        result.insert(section.into(), hash);
    }
    Ok(result)
}
pub fn declared_assets(doc: &MapDocument) -> Result<Vec<ra2_map_document::campaign::Asset>> {
    Ok(read_manifest(doc)?.assets)
}

pub fn validate_assets(assets: &[ra2_map_document::campaign::Asset]) -> Result<()> {
    ensure!(assets.len() <= 128, "任务资源不能超过 128 项");
    let mut names = BTreeSet::new();
    for asset in assets {
        ra2_map_document::campaign::filename(&asset.name)?;
        ensure!(
            names.insert(asset.name.to_ascii_lowercase()),
            "资源输出名重复：{}",
            asset.name
        );
        ensure!(
            !asset.source.is_empty()
                && asset.source.len() <= 1024
                && !asset.source.contains(['\\', ':', '\0'])
                && asset
                    .source
                    .split('/')
                    .all(|p| !matches!(p, "" | "." | "..")),
            "资源须为任务目录内相对路径：{}",
            asset.source
        );
        let name = asset.name.to_ascii_lowercase();
        if name.ends_with(".ini") {
            ensure!(
                matches!(name.as_str(), "soundmd.ini" | "evamd.ini" | "thememd.ini"),
                "任务媒体只接受 soundmd/evamd/thememd.ini；规则/美术 INI 请通过活动资源配置加载，确保原生序号一致"
            );
        }
        ensure!(
            !matches!(
                name.as_str(),
                "battlemd.ini"
                    | "missionmd.ini"
                    | "mapselmd.ini"
                    | "ra2md.csf"
                    | "config.toml"
                    | "manifest.json"
                    | "dependencies.json"
                    | "install.txt"
                    | "spawnmap.ini"
                    | "spawn.ini"
                    | "editor-session.json"
            ),
            "资源不能覆盖生成文件：{name}"
        );
        ensure!(
            name == "subtitlemd.txt"
                || matches!(
                    name.rsplit('.').next().unwrap_or(""),
                    "wav"
                        | "aud"
                        | "ogg"
                        | "wma"
                        | "bik"
                        | "vqa"
                        | "pcx"
                        | "pal"
                        | "shp"
                        | "vxl"
                        | "hva"
                        | "vpl"
                        | "ini"
                        | "fnt"
                        | "bag"
                        | "idx"
                ),
            "任务资源仅接受媒体或游戏数据文件：{name}"
        );
    }
    Ok(())
}
fn seconds(value: u32) -> Result<String> {
    ensure!(value <= i32::MAX as u32 / 15, "时间超出逻辑帧范围");
    Ok(value.to_string())
}
fn waypoint(value: &Waypoint, catalog: &Catalog) -> Result<String> {
    ensure!(
        value.kind == "waypoint" && catalog.waypoints.contains(&value.id),
        "航点 {} 不存在或类型错误",
        value.id
    );
    mission::schema::waypoint_token(value.id)
}
fn named(map: &BTreeMap<String, String>, reference: &Reference, kind: &str) -> Result<String> {
    map.get(reference.check(kind)?)
        .cloned()
        .with_context(|| format!("未定义的 {kind}：{}", reference.id))
}
fn action_row(opcode: i32, value: Option<String>, tail: Option<String>) -> Vec<String> {
    let mut row = mission::schema::row(false, opcode).expect("known action schema");
    if let Some(value) = value {
        row[2] = value;
    }
    if let Some(tail) = tail {
        row[7] = tail;
    }
    row
}

pub fn prepare(doc: &MapDocument, plan: &Mission, catalog: &Catalog) -> Result<Build> {
    prepare_with_context(doc, plan, catalog, None)
}

pub fn prepare_with_context(
    doc: &MapDocument,
    plan: &Mission,
    catalog: &Catalog,
    mut context: Option<BuildContext>,
) -> Result<Build> {
    validate_assets(&plan.assets)?;
    ensure!(
        context.as_ref().is_none_or(|c| c.inputs.len() <= 256),
        "任务构建依赖超过 256 个路径"
    );
    let authored_rule_count = plan.rules.len();
    let lowered = conditions::lower(plan)?;
    let plan = lowered.as_ref().unwrap_or(plan);
    ensure!(matches!(plan.version, 1 | 2), "不支持的任务 SDK 版本");
    identifier(&plan.id)?;
    let trigger_owner = match plan.mode {
        MissionMode::SinglePlayer => {
            ensure!(
                catalog.houses.contains(&plan.player),
                "玩家 House {} 不存在",
                plan.player
            );
            plan.player.as_str()
        }
        MissionMode::Multiplayer => {
            ensure!(
                plan.player.is_empty(),
                "多人任务不能指定固定的 player House"
            );
            "Neutral"
        }
    };
    ensure!(
        plan.rules.len() <= 1024
            && plan.teams.len() <= 256
            && plan.house_definitions.len() <= 256
            && plan.ai_triggers.len() <= 256
            && plan.waypoints.len() <= 672
            && plan.start_owners.len() <= 1024
            && plan.flags.len() <= 100
            && plan.task_forces.len() <= 256
            && plan.scripts.len() <= 256,
        "任务定义超过容量"
    );
    ensure!(
        plan.tags.len() <= 1024 && plan.bindings.len() <= 1024,
        "Tag/绑定定义超过容量"
    );
    let old = read_manifest(doc)?;
    let mut scratch = doc.fork();
    restore(&mut scratch, &old)?;
    if plan.scripts.iter().any(|script| {
        script
            .orders
            .iter()
            .any(|order| matches!(order, Order::ChangeScript { .. }))
    }) || plan
        .teams
        .iter()
        .filter_map(|team| team.orders.as_ref())
        .flatten()
        .any(|order| matches!(order, Order::ChangeScript { .. }))
    {
        // Materialize unmanaged edits before inserting ordinal-dependent rows.
        // Otherwise a pending manual key sorts among new generated keys on the
        // first build, then occupies a different physical slot on the rebuild.
        let bytes = scratch.snapshot().bytes()?;
        let map = vera20k_core::map::map_file::load_from_bytes(bytes.clone())?;
        let profile = scratch.resource_profile.clone();
        scratch = MapDocument::new(ra2_map_document::document::Baseline::parse(
            bytes.into(),
            &map,
        )?);
        scratch.resource_profile = profile;
    }
    if !catalog.type_projection.is_empty() {
        let current = registry_fingerprints(&scratch.snapshot().bytes()?)?;
        ensure!(
            catalog
                .type_projection
                .iter()
                .all(|(section, hash)| current.get(section) == Some(hash)),
            "地图类型注册表已变化，请保存并重新加载场景资源后刷新 SDK 类型"
        );
    }
    let aliases = references::aliases(&scratch, plan, &old.aliases)?;
    let old_waypoint_ids: BTreeSet<_> = old.waypoints.iter().map(|waypoint| waypoint.id).collect();
    let mut effective_catalog = catalog.clone();
    let mut declared_texts = BTreeMap::new();
    ensure!(plan.texts.len() <= 1024, "任务文本超过 1024 项");
    for text in &plan.texts {
        identifier(&text.id)?;
        let expected = text_label(&plan.id, &text.id);
        ensure!(
            text.label == expected && !text.value.contains('\0') && text.value.len() <= 65536,
            "文本 {} 标签无效、含 NUL 或超过 64 KiB",
            text.id
        );
        ensure!(
            !catalog
                .strings
                .iter()
                .any(|label| label.eq_ignore_ascii_case(&expected))
                || old.texts.contains_key(&expected),
            "任务文本标签与已有 CSF 冲突：{expected}"
        );
        ensure!(
            declared_texts
                .insert(expected.clone(), text.value.clone())
                .is_none(),
            "文本重复或稳定标签冲突：{}",
            text.id
        );
        effective_catalog.strings.push(expected);
    }
    effective_catalog
        .waypoints
        .retain(|id| !old_waypoint_ids.contains(id));
    let mut generated_waypoints = Vec::new();
    let mut waypoint_ids = BTreeSet::new();
    for waypoint in &plan.waypoints {
        ensure!(
            (30..=701).contains(&waypoint.id) && waypoint_ids.insert(waypoint.id),
            "新任务航点编号须唯一且位于 30～701"
        );
        ensure!(
            !effective_catalog.waypoints.contains(&waypoint.id),
            "任务航点 {} 与地图已有航点冲突",
            waypoint.id
        );
        ensure!(
            old_waypoint_ids.contains(&waypoint.id)
                || !scratch.records().values().any(|record| {
                    record.section == "Waypoints" && record.key.parse::<u32>() == Ok(waypoint.id)
                }),
            "任务航点 {} 与地图实际记录冲突",
            waypoint.id
        );
        ensure!(
            scratch.terrain([waypoint.x, waypoint.y]).is_some(),
            "任务航点 {} 坐标 ({},{}) 超出地图",
            waypoint.id,
            waypoint.x,
            waypoint.y
        );
        scratch.waypoint(waypoint.id, [waypoint.x, waypoint.y])?;
        effective_catalog.waypoints.push(waypoint.id);
        generated_waypoints.push(WaypointDefinition {
            id: waypoint.id,
            x: waypoint.x,
            y: waypoint.y,
        });
    }
    for waypoint in old
        .waypoints
        .iter()
        .filter(|waypoint| !waypoint_ids.contains(&waypoint.id))
    {
        let (record_id, _) = scratch
            .records()
            .into_iter()
            .find(|(_, record)| {
                record.section == "Waypoints" && record.key == waypoint.id.to_string()
            })
            .context("任务生成航点在恢复阶段意外丢失")?;
        scratch.set(Target::Record(record_id), None)?;
    }
    effective_catalog.waypoints.sort_unstable();
    effective_catalog.waypoints.dedup();
    let catalog = &effective_catalog;
    let mut projected_houses = scratch.fork();
    let mut generated_house_slots = BTreeMap::new();
    let mut desired = BTreeMap::<(String, String), String>::new();
    let mut put = |section: &str, key: &str, value: String| -> Result<()> {
        ensure!(
            value.len() <= 480,
            "生成字段 {section}/{key} 超过 480 字节，请拆分规则"
        );
        ensure!(
            desired
                .insert((section.into(), key.into()), value)
                .is_none(),
            "生成 ID 或字段冲突：{section}/{key}"
        );
        ensure!(
            desired.len() <= MAX_MANAGED_FIELDS,
            "任务生成字段超过归属清单容量，请拆分任务"
        );
        Ok(())
    };
    let mut house_ids: BTreeMap<String, String> = catalog
        .houses
        .iter()
        .map(|id| (id.clone(), id.clone()))
        .collect();
    let mut configured_houses = BTreeSet::new();
    for definition in &plan.house_definitions {
        identifier(&definition.id)?;
        ensure!(
            configured_houses.insert(definition.id.clone()),
            "House {} 在任务中重复声明",
            definition.id
        );
        if definition.create {
            ensure!(
                !house_ids.contains_key(&definition.id),
                "新 House {} 与地图现有 House 重名",
                definition.id
            );
            house_ids.insert(
                definition.id.clone(),
                physical(&plan.id, &definition.id, "HS"),
            );
        } else {
            ensure!(
                mission::ids(&scratch, Kind::House).contains(&definition.id),
                "要配置的 House {} 不在当前地图中",
                definition.id
            );
        }
    }
    let mut occupied_house_slots: BTreeSet<u32> = scratch
        .ini_entries("Houses")
        .into_iter()
        .filter_map(|(_, key, _)| key.parse::<u32>().ok())
        .collect();
    for definition in &plan.house_definitions {
        let house_id = &house_ids[&definition.id];
        if definition.create {
            let country = definition
                .country
                .as_ref()
                .context("新 House 必须指定 Country")?;
            catalog.resource_id(country, crate::catalog::ResourceKind::Country)?;
            let index = (0..=u32::MAX)
                .find(|index| !occupied_house_slots.contains(index))
                .context("Houses 注册表已满")?;
            occupied_house_slots.insert(index);
            put("Houses", &index.to_string(), house_id.clone())?;
            generated_house_slots.insert(index.to_string(), house_id.clone());
            put(house_id, "Name", definition.id.clone())?;
            put(house_id, "Country", country.id.clone())?;
            let color = definition.color.as_deref().unwrap_or("Gold");
            ensure!(
                catalog.house_colors.iter().any(|known| known == color),
                "House {} 的颜色方案 {} 不在当前 Rules [Colors] 目录中",
                definition.id,
                color
            );
            put(house_id, "Color", color.into())?;
            put(
                house_id,
                "PlayerControl",
                if definition.player_control.unwrap_or(false) {
                    "yes"
                } else {
                    "no"
                }
                .into(),
            )?;
            put(house_id, "IQ", definition.iq.unwrap_or(0).to_string())?;
            let credits = definition.credits.unwrap_or(100);
            ensure!(
                credits.checked_mul(100).is_some(),
                "House {} 的 Credits 乘以原生比例 100 后溢出",
                definition.id
            );
            put(house_id, "Credits", credits.to_string())?;
            put(
                house_id,
                "TechLevel",
                definition.tech_level.unwrap_or(10).to_string(),
            )?;
            let allies = definition.allies.as_deref().unwrap_or(&[]);
            let allies = if definition.allies.is_none() {
                vec![Reference {
                    kind: "house".into(),
                    id: definition.id.clone(),
                }]
            } else {
                allies.to_vec()
            };
            let names = allies
                .iter()
                .map(|ally| {
                    house_ids
                        .get(ally.check("house")?)
                        .cloned()
                        .with_context(|| format!("House {} 的 Allies 引用未定义", ally.id))
                })
                .collect::<Result<Vec<_>>>()?;
            put(house_id, "Allies", names.join(","))?;
        } else {
            if let Some(country) = &definition.country {
                catalog.resource_id(country, crate::catalog::ResourceKind::Country)?;
                put(house_id, "Country", country.id.clone())?;
            }
            if let Some(color) = &definition.color {
                ensure!(
                    catalog.house_colors.iter().any(|known| known == color),
                    "House {} 的颜色方案 {} 不在当前 Rules [Colors] 目录中",
                    definition.id,
                    color
                );
                put(house_id, "Color", color.clone())?;
            }
            if let Some(player_control) = definition.player_control {
                put(
                    house_id,
                    "PlayerControl",
                    if player_control { "yes" } else { "no" }.into(),
                )?;
            }
            if let Some(credits) = definition.credits {
                ensure!(
                    credits.checked_mul(100).is_some(),
                    "House {} 的 Credits 乘以原生比例 100 后溢出",
                    definition.id
                );
                put(house_id, "Credits", credits.to_string())?;
            }
            if let Some(iq) = definition.iq {
                put(house_id, "IQ", iq.to_string())?;
            }
            if let Some(tech_level) = definition.tech_level {
                put(house_id, "TechLevel", tech_level.to_string())?;
            }
            if let Some(allies) = &definition.allies {
                let names = allies
                    .iter()
                    .map(|ally| {
                        house_ids
                            .get(ally.check("house")?)
                            .cloned()
                            .with_context(|| {
                                format!("House {} 的 Allies 引用未定义", definition.id)
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                put(house_id, "Allies", names.join(","))?;
            }
        }
        scenario::write_house_presentation(house_id, definition, catalog, &mut put)?;
        if let Some(country) = &definition.country {
            projected_houses.set_ini(house_id, "Country", Some(country.id.as_bytes().to_vec()))?;
        }
        if let Some(base_plan) = &definition.base_plan {
            scenario::write_base_plan(house_id, base_plan, &scratch, catalog, &mut put)?;
        }
    }
    if plan.mode == MissionMode::SinglePlayer {
        put("Basic", "Player", plan.player.clone())?;
    }
    put(
        "Basic",
        "MultiplayerOnly",
        if plan.mode == MissionMode::Multiplayer {
            "yes"
        } else {
            "no"
        }
        .into(),
    )?;
    if let Some(basic) = &plan.basic {
        scenario::write_basic(basic, catalog, &mut put)?;
    }
    if let Some(lottery) = &plan.crate_lottery {
        ensure!(
            plan.basic.as_ref().and_then(|basic| basic.truck_crate) != Some(false),
            "crateLottery 需要 TruckCrate，不能配置 truckCrate:false"
        );
        if plan
            .basic
            .as_ref()
            .and_then(|basic| basic.truck_crate)
            .is_none()
        {
            put("Basic", "TruckCrate", "yes".into())?;
        }
        crate_lottery::write(lottery, &scratch, catalog, &mut put)?;
    }
    if let Some(flags) = &plan.special_flags {
        scenario::write_special_flags(flags, plan.mode, &mut put)?;
    }
    if let Some(lighting) = &plan.lighting {
        scenario::write_lighting(lighting, &mut put)?;
    }
    if let Some(ranking) = &plan.ranking {
        ensure!(
            plan.mode == MissionMode::SinglePlayer,
            "结算评级仅支持单人任务"
        );
        scenario::write_ranking(ranking, catalog, &mut put)?;
    }
    for (key, value) in generated_house_slots {
        projected_houses.set_ini("Houses", &key, Some(value.into_bytes()))?;
    }
    let roster_ini = vera20k_core::rules::ini_parser::IniFile::from_bytes(
        &projected_houses.snapshot().bytes()?,
    )?;
    let roster = vera20k_core::map::houses::parse_house_roster(&roster_ini, &[], None);
    let mut house_indices = BTreeMap::new();
    for (i, house) in roster.houses.iter().enumerate() {
        ensure!(
            house_indices.insert(house.name.clone(), i).is_none(),
            "House 注册表存在重复名称"
        );
    }
    let mut flags = BTreeMap::new();
    if !plan.flags.is_empty() {
        for trigger in mission::ids(&scratch, Kind::Trigger) {
            for (section, is_event) in [("Events", true), ("Actions", false)] {
                let rows = mission::decode_list(&scratch.ini_text(section, &trigger), is_event)?;
                ensure!(
                    rows.iter().all(|r| r
                        .first()
                        .and_then(|op| op.parse().ok())
                        .is_some_and(|op| mission::schema::opcode(is_event, op).is_some())),
                    "触发器 {trigger} 含未知扩展；无法安全自动分配局部标记"
                );
            }
        }
        for script in mission::ids(&scratch, Kind::Script) {
            ensure!(
                scratch
                    .ini_fields(&script)
                    .iter()
                    .filter(|(k, _)| k.parse::<usize>().is_ok())
                    .all(|(_, v)| String::from_utf8_lossy(v)
                        .split(',')
                        .next()
                        .and_then(|s| s.parse().ok())
                        .is_some_and(|op| mission::schema::scripts::opcode(op).is_some())),
                "脚本 {script} 含未知扩展；无法安全自动分配局部标记"
            );
        }
    }
    let mut used: BTreeSet<u32> = mission::ids(&scratch, Kind::Variable)
        .iter()
        .filter_map(|n| n.parse().ok())
        .collect();
    used.extend(
        mission::references(&scratch)
            .iter()
            .filter(|r| r.to.0 == Kind::Variable)
            .filter_map(|r| r.to.1.parse::<u32>().ok()),
    );
    for flag in &plan.flags {
        identifier(&flag.id)?;
        ensure!(!flags.contains_key(&flag.id), "重复标记 {}", flag.id);
        let slot = old
            .flags
            .get(&flag.id)
            .copied()
            .filter(|n| !used.contains(n))
            .or_else(|| (0..100).find(|n| !used.contains(n)))
            .context("局部变量 0～99 已满")?;
        used.insert(slot);
        flags.insert(flag.id.clone(), slot.to_string());
        put(
            "VariableNames",
            &slot.to_string(),
            format!("{}_{},{}", plan.id, flag.id, u8::from(flag.initial)),
        )?;
    }
    let mut task_forces = references::existing(&scratch, plan, catalog, "taskForce")?;
    for definition in &plan.task_forces {
        identifier(&definition.id)?;
        let id = physical(&plan.id, &definition.id, "FT");
        ensure!(
            task_forces
                .insert(definition.id.clone(), id.clone())
                .is_none(),
            "重复特遣部队 {}",
            definition.id
        );
        definitions::task_force(
            &id,
            &definition.id,
            &definition.units,
            definition.group,
            catalog,
            &mut put,
        )?;
    }
    let mut scripts = references::existing(&scratch, plan, catalog, "script")?;
    for definition in &plan.scripts {
        identifier(&definition.id)?;
        let id = physical(&plan.id, &definition.id, "DS");
        ensure!(
            scripts.insert(definition.id.clone(), id.clone()).is_none(),
            "重复行为脚本 {}",
            definition.id
        );
    }
    let script_indices = definitions::script_indices(&scratch, plan, catalog)?;
    for definition in &plan.scripts {
        let id = &scripts[&definition.id];
        definitions::script(
            id,
            &definition.id,
            &definition.orders,
            &definitions::ScriptContext {
                doc: &scratch,
                catalog,
                mode: plan.mode,
                flags: &flags,
                scripts: &scripts,
                script_indices: &script_indices,
            },
            &mut put,
        )?;
    }
    let mut graph = rules::Graph::new(plan)?;
    graph
        .ids
        .extend(references::existing(&scratch, plan, catalog, "rule")?);
    let mut tags = tags::Tags::new(plan, &graph, &mut put)?;
    tags.ids
        .extend(references::existing(&scratch, plan, catalog, "tag")?);
    let mut teams = references::existing(&scratch, plan, catalog, "team")?;
    for team in &plan.teams {
        identifier(&team.id)?;
        ensure!(!teams.contains_key(&team.id), "重复小队 {}", team.id);
        let owner = match &team.house {
            TeamOwner::House { id } => {
                let house = house_ids
                    .get(id)
                    .with_context(|| format!("小队 {} 的 House {id} 未声明或不存在", team.id))?;
                if let Some(definition) = roster.houses.iter().find(|h| &h.name == house) {
                    let default_country = catalog
                        .fixed_country_indices
                        .iter()
                        .find(|(_, i)| **i == 0)
                        .map(|(id, _)| id.as_str());
                    let raw_country = definition.country.as_deref().or(default_country);
                    if let Some(country) = raw_country {
                        let resolve = |token: &str| {
                            catalog
                                .country_names
                                .get(&token.to_ascii_lowercase())
                                .map(String::as_str)
                                .or_else(|| {
                                    if catalog.country_names.is_empty() {
                                        catalog
                                            .countries
                                            .iter()
                                            .find(|c| c.eq_ignore_ascii_case(token))
                                            .map(String::as_str)
                                    } else {
                                        None
                                    }
                                })
                        };
                        let resolved = resolve(country).with_context(|| {
                            format!(
                                "小队 {} 的 House {id} 的 Country {country} 无有效原生国家映射",
                                team.id
                            )
                        })?;
                        let first = roster.houses.iter().find(|h| {
                            h.country.as_deref().or(default_country).and_then(resolve)
                                == Some(resolved)
                        });
                        ensure!(
                            first.is_some_and(|h| &h.name == house),
                            "小队 {} 的 House {id} 与在先 House 共享 Country {country}；原生小队只选择第一个 House，不能表达此归属",
                            team.id
                        );
                        country.to_string()
                    } else {
                        // Preserve legacy sectionless/default-country inputs.
                        house.clone()
                    }
                } else {
                    house.clone()
                }
            }
            TeamOwner::StartPlayer { id } => {
                ensure!(
                    plan.mode == MissionMode::Multiplayer,
                    "按起点选择玩家仅支持多人任务"
                );
                ensure!(
                    *id < u32::from(MAX_PLAYERS) && catalog.waypoints.contains(id),
                    "小队 {} 的玩家起点必须是地图已有的 0～29 号航点",
                    team.id
                );
                player_starts::house_token(*id).context("玩家起点选择器无效")?
            }
        };
        let tf = match (&team.units, &team.task_force) {
            (Some(units), None) => {
                let id = physical(&plan.id, &team.id, "TF");
                definitions::task_force(&id, &team.id, units, -1, catalog, &mut put)?;
                id
            }
            (None, Some(reference)) => named(&task_forces, reference, "taskForce")?,
            _ => anyhow::bail!("小队 {} 必须且只能选择 units 或 taskForce", team.id),
        };
        let sc = match (&team.orders, &team.script) {
            (Some(orders), None) => {
                let id = physical(&plan.id, &team.id, "SC");
                definitions::script(
                    &id,
                    &team.id,
                    orders,
                    &definitions::ScriptContext {
                        doc: &scratch,
                        catalog,
                        mode: plan.mode,
                        flags: &flags,
                        scripts: &scripts,
                        script_indices: &script_indices,
                    },
                    &mut put,
                )?;
                id
            }
            (None, Some(reference)) => named(&scripts, reference, "script")?,
            _ => anyhow::bail!("小队 {} 必须且只能选择 orders 或 script", team.id),
        };
        let tm = physical(&plan.id, &team.id, "TM");
        put("TeamTypes", &tm, tm.clone())?;
        for (key, value) in [
            ("Name", team.id.clone()),
            ("House", owner),
            ("TaskForce", tf),
            ("Script", sc),
            ("Waypoint", waypoint(&team.waypoint, catalog)?),
        ] {
            put(&tm, key, value)?;
        }
        if let Some(tag) = &team.tag {
            put(&tm, "Tag", named(&tags.ids, tag, "tag")?)?;
        }
        for (key, value) in [
            ("Max", team.max.unwrap_or(1)),
            ("Priority", team.priority.unwrap_or(5)),
            ("VeteranLevel", team.veteran_level.unwrap_or(1)),
            ("Group", team.group.unwrap_or(-1)),
            (
                "MindControlDecision",
                team.mind_control_decision.unwrap_or(0),
            ),
            ("TechLevel", 0),
        ] {
            put(&tm, key, value.to_string())?;
        }
        ensure!(
            (0..=3).contains(&team.veteran_level.unwrap_or(1)),
            "小队 {} 的 veteranLevel 须为 0～3",
            team.id
        );
        for (key, value) in [
            ("Autocreate", team.autocreate.unwrap_or(false)),
            ("Full", team.full.unwrap_or(true)),
            ("Recruiter", team.recruiter.unwrap_or(false)),
            ("IsBaseDefense", team.is_base_defense.unwrap_or(false)),
            ("Suicide", team.suicide.unwrap_or(false)),
            ("Aggressive", team.aggressive.unwrap_or(false)),
            ("Reinforce", team.reinforce.unwrap_or(true)),
            (
                "AreTeamMembersRecruitable",
                team.are_team_members_recruitable.unwrap_or(true),
            ),
            ("OnTransOnly", team.on_trans_only.unwrap_or(false)),
            ("AvoidThreats", team.avoid_threats.unwrap_or(false)),
            ("Annoyance", team.annoyance.unwrap_or(false)),
            ("GuardSlower", team.guard_slower.unwrap_or(false)),
            (
                "TransportsReturnOnUnload",
                team.transports_return_on_unload.unwrap_or(false),
            ),
            ("Droppod", team.droppod.unwrap_or(false)),
            (
                "OnlyTargetHouseEnemy",
                team.only_target_house_enemy.unwrap_or(false),
            ),
            ("Loadable", team.loadable.unwrap_or(false)),
            ("Whiner", team.whiner.unwrap_or(false)),
            (
                "UseTransportOrigin",
                team.use_transport_origin.unwrap_or(false),
            ),
            ("Prebuild", false),
        ] {
            put(&tm, key, if value { "yes" } else { "no" }.into())?;
        }
        if let Some(wp) = &team.transport_waypoint {
            put(&tm, "TransportWaypoint", waypoint(wp, catalog)?)?;
        }
        teams.insert(team.id.clone(), tm);
    }
    scenario::write_ai_triggers(plan, &teams, catalog, &mut put)?;
    let mut attachments = Vec::new();
    let mut start_owner_objects = BTreeSet::new();
    for assignment in &plan.start_owners {
        let object_id = assignment.object.check("object")?;
        ensure!(
            assignment.player < 30 && catalog.waypoints.contains(&assignment.player),
            "对象 {} 的玩家起点须为地图已有的 0～29 航点",
            object_id
        );
        ensure!(
            catalog.objects.iter().any(|known| known == object_id),
            "对象 {} 不在当前地图对象目录中",
            object_id
        );
        ensure!(
            start_owner_objects.insert(object_id.to_owned()),
            "对象 {object_id} 重复指定开局归属"
        );
        ensure!(
            !plan.bindings.iter().any(|binding| matches!(
                binding,
                Binding::Object { object, .. } if object.id == object_id
            )),
            "对象 {object_id} 不能同时指定开局归属和 Tag 绑定"
        );
        ensure!(
            plan.mode == MissionMode::Multiplayer,
            "按起点分配对象仅支持多人任务"
        );
    }
    let mut bound_objects = BTreeMap::new();
    for rule in &plan.rules {
        ensure!(
            (1..=64).contains(&rule.actions.len()),
            "规则 {} 需要 1～64 个动作",
            rule.id
        );
        let tr = physical(&plan.id, &rule.id, "TR");
        let root = graph.roots[rule.id.as_str()];
        let tag = tags.roots[root].clone();
        let events = rules::Events {
            doc: &scratch,
            catalog,
            flags: &flags,
            teams: &teams,
            mode: plan.mode,
            house_ids: &house_ids,
            house_indices: &house_indices,
            attachments: &mut attachments,
            bound: &mut bound_objects,
            tag: &tag,
        }
        .encode(&rule.when)
        .map_err(|error| anyhow::anyhow!("规则 {} 的条件无效：{error:#}", rule.id))?;
        let mut actions = Vec::new();
        for action in &rule.actions {
            actions.push(match action {
                Action::DeleteRule { rule } => {
                    action_row(12, Some(named(&graph.ids, rule, "rule")?), None)
                }
                Action::DeleteTag { tag } => {
                    action_row(70, Some(named(&tags.ids, tag, "tag")?), None)
                }
                Action::AllToHunt { house }
                | Action::StartProduction { house }
                | Action::EnableAi { house }
                | Action::DisableAi { house }
                | Action::ChangeAttachedOwner { house } => {
                    let opcode = match action {
                        Action::AllToHunt { .. } => 6,
                        Action::StartProduction { .. } => 3,
                        Action::EnableAi { .. } => 74,
                        Action::DisableAi { .. } => 75,
                        _ => 14,
                    };
                    action_row(
                        opcode,
                        Some(rules::action_house(house, plan.mode, catalog)?),
                        None,
                    )
                }
                Action::WinCountry { country } | Action::LoseCountry { country } => {
                    ensure!(
                        plan.mode == MissionMode::SinglePlayer,
                        "winCountry/loseCountry 仅支持单人结果流程"
                    );
                    action_row(
                        if matches!(action, Action::WinCountry { .. }) {
                            1
                        } else {
                            2
                        },
                        Some(
                            catalog
                                .resource_index(country, crate::catalog::ResourceKind::Country)?
                                .to_string(),
                        ),
                        None,
                    )
                }
                Action::PlaySound { sound } => action_row(
                    19,
                    Some(
                        catalog
                            .resource_id(sound, crate::catalog::ResourceKind::Sound)?
                            .into(),
                    ),
                    None,
                ),
                Action::PlayMusic { theme } => action_row(
                    20,
                    Some(
                        catalog
                            .resource_id(theme, crate::catalog::ResourceKind::Theme)?
                            .into(),
                    ),
                    None,
                ),
                Action::PlayEva { eva } => action_row(
                    21,
                    Some(
                        catalog
                            .resource_id(eva, crate::catalog::ResourceKind::Eva)?
                            .into(),
                    ),
                    None,
                ),
                Action::DeleteTeam { team } => {
                    action_row(5, Some(named(&teams, team, "team")?), None)
                }
                Action::ReinforceDefault { team } => {
                    action_row(7, Some(named(&teams, team, "team")?), None)
                }
                Action::ChronoReinforce { team, at } => action_row(
                    107,
                    Some(named(&teams, team, "team")?),
                    Some(waypoint(at, catalog)?),
                ),
                Action::FlashTeam { team, frames } => action_row(
                    104,
                    Some(named(&teams, team, "team")?),
                    Some(frames.to_string()),
                ),
                Action::RevealMap => action_row(16, None, None),
                Action::SetAlternateBase { at } => {
                    action_row(137, None, Some(waypoint(at, catalog)?))
                }
                Action::ClearAlternateBase => action_row(138, None, None),
                Action::DropZone { at } => action_row(8, None, Some(waypoint(at, catalog)?)),
                Action::CreateCrate { reward, at } => {
                    ensure!(
                        *reward <= 18 || *reward == 20,
                        "木箱种类须为 0～18 或 Random(20)"
                    );
                    action_row(108, Some(reward.to_string()), Some(waypoint(at, catalog)?))
                }
                Action::EnableRule { rule }
                | Action::DisableRule { rule }
                | Action::ForceRule { rule } => {
                    let opcode = match action {
                        Action::EnableRule { .. } => 53,
                        Action::DisableRule { .. } => 54,
                        _ => 22,
                    };
                    action_row(opcode, Some(named(&graph.ids, rule, "rule")?), None)
                }
                Action::Reinforce { team, at } => action_row(
                    80,
                    Some(named(&teams, team, "team")?),
                    Some(waypoint(at, catalog)?),
                ),
                Action::Recruit { team } => action_row(4, Some(named(&teams, team, "team")?), None),
                Action::Message { text } | Action::SetCountdownTitle { text } => {
                    let label = text.check("text")?;
                    ensure!(
                        label.len() <= 31
                            && label.is_ascii()
                            && !label.contains([',', ';', '\r', '\n'])
                            && catalog.strings.iter().any(|s| s == label),
                        "CSF 标签 {label} 不存在或超出原生字段范围"
                    );
                    let opcode = if matches!(action, Action::Message { .. }) {
                        11
                    } else {
                        103
                    };
                    action_row(opcode, Some(label.into()), None)
                }
                Action::SetCountdown { seconds: s } => action_row(27, Some(seconds(*s)?), None),
                Action::StartCountdown => action_row(23, None, None),
                Action::StopCountdown => action_row(24, None, None),
                Action::ExtendCountdown { seconds: s } => action_row(25, Some(seconds(*s)?), None),
                Action::ShortenCountdown { seconds: s } => action_row(26, Some(seconds(*s)?), None),
                Action::SetFlag { flag } => {
                    action_row(56, Some(named(&flags, flag, "flag")?), None)
                }
                Action::ClearFlag { flag } => {
                    action_row(57, Some(named(&flags, flag, "flag")?), None)
                }
                Action::SetLocal { flag } | Action::ClearLocal { flag } => action_row(
                    if matches!(action, Action::SetLocal { .. }) {
                        56
                    } else {
                        57
                    },
                    Some(rules::variable(flag, false, &scratch)?),
                    None,
                ),
                Action::SetGlobal { flag } | Action::ClearGlobal { flag } => action_row(
                    if matches!(action, Action::SetGlobal { .. }) {
                        28
                    } else {
                        29
                    },
                    Some(rules::variable(flag, true, &scratch)?),
                    None,
                ),
                Action::WinPlayer | Action::LosePlayer => {
                    ensure!(
                        plan.mode == MissionMode::SinglePlayer,
                        "winPlayer/losePlayer 仅支持单人任务"
                    );
                    action_row(
                        if matches!(action, Action::WinPlayer) {
                            67
                        } else {
                            68
                        },
                        None,
                        None,
                    )
                }
                action => extra_actions::encode(action, &scratch, catalog, &teams, plan.mode)?,
            });
        }
        let owner = rule
            .owner
            .as_ref()
            .map(|owner| catalog.resource_id(owner, crate::catalog::ResourceKind::Country))
            .transpose()?
            .unwrap_or(trigger_owner);
        let linked = rule
            .linked
            .as_ref()
            .map(|reference| named(&graph.ids, reference, "rule"))
            .transpose()?
            .unwrap_or_else(|| "<none>".into());
        let difficulty = [Difficulty::Easy, Difficulty::Normal, Difficulty::Hard]
            .map(|level| u8::from(rule.difficulties.contains(&level)));
        put(
            "Triggers",
            &tr,
            format!(
                "{owner},{linked},{},{},{},{},{},{}",
                rule.id,
                u8::from(!rule.enabled),
                difficulty[0],
                difficulty[1],
                difficulty[2],
                u8::from(rule.transfer_on_capture)
            ),
        )?;
        put("Events", &tr, mission::encode_list(&events))?;
        put("Actions", &tr, mission::encode_list(&actions))?;
    }
    tags.bind(
        plan,
        &mut rules::Events {
            doc: &scratch,
            catalog,
            flags: &flags,
            teams: &teams,
            mode: plan.mode,
            house_ids: &house_ids,
            house_indices: &house_indices,
            attachments: &mut attachments,
            bound: &mut bound_objects,
            tag: "",
        },
        &mut put,
    )?;
    let mut managed_start_owners = Vec::new();
    for assignment in &plan.start_owners {
        let object_id = assignment.object.check("object")?;
        let (record_id, before, column) = object(&scratch, object_id)?;
        ensure!(
            startup_owner::available(&scratch, &before)
                && startup_owner::get(&scratch, &before).is_none()
                && !bound_objects.contains_key(object_id),
            "对象 {object_id} 已有手工 Tag，不能改为开局归属"
        );
        let before_owner = before
            .fields()
            .first()
            .cloned()
            .context("对象缺少 House 字段")?;
        let before_tag = before.fields()[column].clone();
        startup_owner::set(&mut scratch, record_id, Some(assignment.player as u8))?;
        let after = scratch
            .records()
            .remove(&record_id)
            .context("对象设置开局归属后已不存在")?;
        let after_tag = after.fields()[column].clone();
        managed_start_owners.push(ManagedStartOwner {
            object: object_id.to_owned(),
            type_id: before.type_id().to_owned(),
            before_owner,
            after_owner: after.fields()[0].clone(),
            before_tag,
            after_tag,
            player: assignment.player as u8,
        });
    }
    let mut fields = Vec::new();
    for (section, _) in desired
        .keys()
        .filter(|(s, k)| s.starts_with("TS") && !crate_lottery::is_managed_field(s, k))
    {
        ensure!(
            scratch.ini_entries(section).is_empty(),
            "生成 section {section} 与手工记录冲突"
        );
    }
    for ((section, key), value) in desired {
        let previous = scratch.ini_value(&section, &key);
        if section != "Basic" {
            let existing_house_setting = mission::ids(&scratch, Kind::House)
                .iter()
                .any(|house| house == &section)
                && matches!(
                    key.as_str(),
                    "Country"
                        | "Color"
                        | "PlayerControl"
                        | "IQ"
                        | "Credits"
                        | "TechLevel"
                        | "Allies"
                        | "NodeCount"
                        | "PercentBuilt"
                        | "UIName"
                        | "Edge"
                );
            let existing_house_base_node = mission::ids(&scratch, Kind::House)
                .iter()
                .any(|house| house == &section)
                && key.parse::<usize>().is_ok();
            let house_roster_entry =
                section == "Houses" && key.parse::<u32>().is_ok() && value.starts_with("TSHS");
            let ranking_setting = section == "Ranking" && scenario::is_ranking_key(&key);
            let special_flag = section == "SpecialFlags" && scenario::is_special_flag_key(&key);
            let lighting_setting = section == "Lighting" && scenario::is_lighting_key(&key);
            ensure!(
                previous.is_none()
                    || existing_house_setting
                    || existing_house_base_node
                    || ranking_setting
                    || lighting_setting
                    || crate_lottery::is_managed_field(&section, &key)
                    || special_flag,
                "生成项与手工记录冲突：{section}/{key}"
            );
            ensure!(
                !house_roster_entry || previous.is_none(),
                "新 House 注册位置与手工记录冲突：Houses/{key}"
            );
            ensure!(
                !scratch
                    .ini_entries(&section)
                    .iter()
                    .any(|(_, k, _)| k.eq_ignore_ascii_case(&key) && k != &key),
                "生成项大小写冲突：{section}/{key}"
            );
        }
        scratch.set_ini(&section, &key, Some(value.as_bytes().to_vec()))?;
        fields.push(Field {
            section,
            key,
            value,
            before: previous.map(|v| String::from_utf8_lossy(&v).into_owned()),
        });
    }
    let rebuilds = scenario::write_rebuilds(&mut scratch, plan)?;
    for attachment in &attachments {
        let (id, record, column) = object(&scratch, &attachment.object)?;
        scratch.set(
            Target::Record(id),
            Some(Value::Record(
                record.patched(&[(column, attachment.after.clone())])?,
            )),
        )?;
    }
    for r in mission::references(&scratch) {
        if old
            .fields
            .iter()
            .any(|f| f.section == r.to.0.registry() && (f.key == r.to.1 || f.value == r.to.1))
        {
            ensure!(
                mission::ids(&scratch, r.to.0)
                    .iter()
                    .any(|id| mission::same_id(r.to.0, id, &r.to.1)),
                "删除的生成项仍被 {} {} 引用：{}",
                r.from.0.label(),
                r.from.1,
                r.to.1
            );
        }
    }
    let removed_waypoints = old_waypoint_ids
        .difference(&waypoint_ids)
        .copied()
        .collect();
    scenario::check_removed_waypoints(&scratch, &removed_waypoints)?;
    if let Some(context) = &mut context {
        context.registries = registry_fingerprints(&scratch.snapshot().bytes()?)?;
    }
    let manifest = Manifest {
        version: 1,
        mission: plan.id.clone(),
        flags: flags
            .into_iter()
            .map(|(id, n)| (id, n.parse().unwrap()))
            .collect(),
        fields,
        attachments,
        waypoints: generated_waypoints,
        start_owners: managed_start_owners,
        rebuilds,
        texts: declared_texts,
        assets: plan.assets.clone(),
        context,
        aliases,
        sources: references::mappings(plan)?,
    };
    for (_, key, _) in scratch.ini_entries(META) {
        scratch.set_ini(META, &key, None)?;
    }
    let hex: String = serde_json::to_vec(&manifest)?
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let chunks: Vec<_> = hex.as_bytes().chunks(400).collect();
    ensure!(
        chunks.len() <= 32768,
        "任务托管清单超过容量，请减少文本或定义"
    );
    scratch.set_ini(META, "Version", Some(b"1".to_vec()))?;
    scratch.set_ini(META, "Chunks", Some(chunks.len().to_string().into_bytes()))?;
    for (index, bytes) in chunks.iter().enumerate() {
        scratch.set_ini(META, &format!("Data{index}"), Some(bytes.to_vec()))?;
    }
    // Restoring/reinserting a managed field can move it past unmanaged keys.
    // Once validation confirms the same product, preserve the current layout.
    let bytes = if manifest == old {
        doc.snapshot().bytes()?
    } else {
        scratch.snapshot().bytes()?
    };
    // Reparse before publishing so invalid maps never reach the live document.
    let mut validation = doc.fork();
    validation.replace_bytes(bytes.clone(), "验证任务产物")?;
    Ok(Build {
        document: doc.id,
        state: doc.state_id,
        bytes,
        field_count: manifest.fields.len(),
        rule_count: authored_rule_count,
        team_count: plan.teams.len(),
    })
}
