//! Abstract Syntax Tree nodes for contingency-dsl.
//!
//! Mirrors `contingency_dsl/ast.py` in the Python reference implementation.
//! The AST is language-independent and matches `schema/operant/ast.schema.json`.
//!
//! All nodes are immutable value types built from owned data. `ScheduleExpr` is the
//! operant expression union; `AnnotatedScheduleExpr` wraps one with trailing
//! annotations. Annotations themselves are represented as sequences of
//! `AnnotationArg` values under a canonical keyword.

use std::collections::BTreeMap;

use crate::enums::{Combinator, Distribution, Domain};

/// 2D schedule type: Distribution × Domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScheduleType {
    pub distribution: Distribution,
    pub domain: Domain,
}

impl ScheduleType {
    pub fn new(distribution: Distribution, domain: Domain) -> Self {
        Self { distribution, domain }
    }
}

// --- Leaf schedule forms ---

/// Parametric atomic schedule (e.g. FR5, VI60s, RT20ms).
#[derive(Debug, Clone, PartialEq)]
pub struct Atomic {
    pub schedule_type: ScheduleType,
    pub value: f64,
    pub time_unit: Option<String>,
}

/// Boundary schedule: EXT (zero) or CRF (FR1 identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Special {
    pub kind: SpecialKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecialKind {
    Ext,
    Crf,
}

impl SpecialKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ext => "EXT",
            Self::Crf => "CRF",
        }
    }
}

/// Scalar keyword parameter on a compound (COD / FRCO / BO).
#[derive(Debug, Clone, PartialEq)]
pub struct CompoundParam {
    pub value: f64,
    pub time_unit: Option<String>,
}

/// Direction-specific override on a directional COD.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectionalEntry {
    pub from_ref: DirRef,
    pub to_ref: DirRef,
    pub value: f64,
    pub time_unit: Option<String>,
}

/// Direction reference: numeric index (1-based) or identifier awaiting resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirRef {
    Index(i64),
    Name(String),
}

/// Directional COD: optional base plus direction-specific overrides.
#[derive(Debug, Clone, PartialEq)]
pub struct DirectionalCOD {
    pub base: Option<CompoundParam>,
    pub directional: Vec<DirectionalEntry>,
}

/// PUNISH(...) keyword value on Conc.
///
/// `payload` carries the pre-built JSON-shaped value preserved verbatim by the
/// serializer. Three forms share this wrapper (directional / changeover /
/// component) per `conformance/operant/compound.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct PunishParam {
    pub payload: JsonValue,
}

/// Compound-schedule parameter value (four shapes share one slot).
#[derive(Debug, Clone, PartialEq)]
pub enum CombinatorParamValue {
    Scalar(CompoundParam),
    Directional(DirectionalCOD),
    Punish(PunishParam),
    /// Raw JSON value preserved verbatim by the serializer. Used for
    /// Overlay `target=...` (string) and other compound kwargs whose shape
    /// does not fit the scalar form.
    Raw(JsonValue),
}

/// Compound schedule: combinator applied to 2+ sub-schedules.
#[derive(Debug, Clone, PartialEq)]
pub struct Compound {
    pub combinator: Combinator,
    pub components: Vec<ScheduleExpr>,
    /// Preserved insertion order is not semantically meaningful; the serializer
    /// emits keys in fixture-compatible canonical order.
    pub params: Option<BTreeMap<String, CombinatorParamValue>>,
}

/// Modifier schedule: DR, PR, Repeat, Pctl, Lag.
#[derive(Debug, Clone, PartialEq)]
pub struct Modifier {
    pub kind: ModifierKind,
    pub value: Option<f64>,
    pub time_unit: Option<String>,
    pub inner: Option<Box<ScheduleExpr>>,
    pub pr_step: Option<String>,
    pub pr_start: Option<f64>,
    pub pr_increment: Option<f64>,
    pub pr_ratio: Option<f64>,
    pub pctl_target: Option<String>,
    pub pctl_rank: Option<i64>,
    pub pctl_window: Option<i64>,
    pub pctl_dir: Option<String>,
    pub length: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModifierKind {
    Drl,
    Drh,
    Dro,
    Pr,
    Repeat,
    Pctl,
    Lag,
}

impl ModifierKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Drl => "DRL",
            Self::Drh => "DRH",
            Self::Dro => "DRO",
            Self::Pr => "PR",
            Self::Repeat => "Repeat",
            Self::Pctl => "Pctl",
            Self::Lag => "Lag",
        }
    }
}

/// Temporal availability window (FI30 LH10 form).
#[derive(Debug, Clone, PartialEq)]
pub struct LimitedHold {
    pub hold_duration: f64,
    pub inner: Box<ScheduleExpr>,
    pub time_unit: Option<String>,
}

/// Second-order schedule: FR5(FI30) form.
///
/// Either slot may be an Atomic or an IdentifierRef; the analyzer substitutes
/// let-bound identifiers with their Atomic value during binding expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct SecondOrder {
    pub overall: SecondOrderSlot,
    pub unit: SecondOrderSlot,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SecondOrderSlot {
    Atomic(Atomic),
    Ref(String),
}

/// Per-schedule timeout parameters (Leitenberg, 1965).
#[derive(Debug, Clone, PartialEq)]
pub struct Timeout {
    pub duration: f64,
    pub duration_unit: Option<String>,
    pub reset_on_response: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TimeoutWrapped {
    pub inner: Box<ScheduleExpr>,
    pub timeout: Timeout,
}

/// Response-cost parameters (Weiner, 1962; Hackenberg, 2009).
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseCost {
    pub amount: f64,
    pub unit: String,
}

impl ResponseCost {
    pub const DEFAULT_UNIT: &'static str = "token";
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResponseCostWrapped {
    pub inner: Box<ScheduleExpr>,
    pub response_cost: ResponseCost,
}

/// Adjusting schedule (Mazur, 1987 style).
///
/// `adj_start`, `adj_step`, `adj_min`, `adj_max` are `{value, time_unit}`
/// payloads preserved as `NumericPayload`.
#[derive(Debug, Clone, PartialEq)]
pub struct AdjustingSchedule {
    pub adj_target: String,
    pub adj_start: NumericPayload,
    pub adj_step: NumericPayload,
    pub adj_min: Option<NumericPayload>,
    pub adj_max: Option<NumericPayload>,
}

/// Interlocking schedule (Ferster & Skinner, 1957): R0 decays linearly over T.
#[derive(Debug, Clone, PartialEq)]
pub struct InterlockingSchedule {
    pub interlock_r0: i64,
    pub interlock_t: NumericPayload,
}

/// Trial-based schedule primitive (MTS, GoNoGo, ...).
#[derive(Debug, Clone, PartialEq)]
pub struct TrialBased {
    pub trial_type: String,
    pub params: BTreeMap<String, TrialParam>,
}

/// Parameter slot in a trial-based schedule. A JSON value is kept for payload-
/// shaped inputs (e.g. `{value, time_unit}` or nested stimulus arrays).
#[derive(Debug, Clone, PartialEq)]
pub enum TrialParam {
    Int(i64),
    Float(f64),
    Str(String),
    Numeric(NumericPayload),
    Schedule(Box<ScheduleExpr>),
    Json(JsonValue),
}

/// Aversive schedule: Sidman / DiscrimAv / Escape.
#[derive(Debug, Clone, PartialEq)]
pub struct AversiveSchedule {
    pub kind: AversiveKind,
    pub params: BTreeMap<String, AversiveParam>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AversiveKind {
    Sidman,
    DiscrimAv,
    Escape,
}

impl AversiveKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sidman => "Sidman",
            Self::DiscrimAv => "DiscrimAv",
            Self::Escape => "Escape",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AversiveParam {
    Numeric(NumericPayload),
    Str(String),
}

/// `{value, time_unit}` payload shared by multiple stateful forms.
#[derive(Debug, Clone, PartialEq)]
pub struct NumericPayload {
    pub value: f64,
    pub time_unit: Option<String>,
}

/// Reference to a let-bound name (pre-expansion; resolved by analyzer).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentifierRef {
    pub name: String,
}

/// Operant-layer expression union.
#[derive(Debug, Clone, PartialEq)]
pub enum ScheduleExpr {
    Atomic(Atomic),
    Special(Special),
    Compound(Compound),
    Modifier(Modifier),
    LimitedHold(LimitedHold),
    SecondOrder(SecondOrder),
    IdentifierRef(IdentifierRef),
    AversiveSchedule(AversiveSchedule),
    TimeoutWrapped(TimeoutWrapped),
    ResponseCostWrapped(ResponseCostWrapped),
    TrialBased(TrialBased),
    AdjustingSchedule(AdjustingSchedule),
    InterlockingSchedule(InterlockingSchedule),
    /// Respondent primitive resolved by the Tier A respondent parser.
    Respondent(Box<RespondentExpr>),
    /// Placeholder for extension-resolved respondent primitives.
    RespondentExtension(RespondentExtensionCall),
    /// Annotated schedule expression (leaf form that carries trailing
    /// annotations).
    Annotated(Box<AnnotatedSchedule>),
}

// --- Annotation nodes (§4.7) ---

/// Scalar argument attached to an annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationArg {
    pub value: AnnotationValue,
    pub key: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AnnotationValue {
    Str(String),
    Int(i64),
    Float(f64),
    /// Composite value (list / object) preserved as a raw JSON tree for
    /// fixtures that use structured annotation payloads.
    Json(JsonValue),
}

/// A single @-prefixed annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct Annotation {
    pub keyword: String,
    pub positional: Option<AnnotationValue>,
    pub params: Option<BTreeMap<String, AnnotationValue>>,
    /// Original source spelling when an alias was used (e.g. `punisher` for the
    /// canonical `reinforcer`).
    pub label: Option<String>,
}

/// Schedule expression + trailing annotations.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedSchedule {
    pub expr: ScheduleExpr,
    pub annotations: Vec<Annotation>,
}

// --- Respondent layer (CS-US primitives) ---

/// Respondent expression union (Tier A only; Tier B is handled via the
/// extension point in `RespondentExtensionCall`).
#[derive(Debug, Clone, PartialEq)]
pub enum RespondentExpr {
    PairForwardDelay(PairForwardDelay),
    PairForwardTrace(PairForwardTrace),
    PairSimultaneous(PairSimultaneous),
    PairBackward(PairBackward),
    Extinction(Extinction),
    CsOnly(CsOnly),
    UsOnly(UsOnly),
    Contingency(Contingency),
    TrulyRandom(TrulyRandom),
    ExplicitlyUnpaired(ExplicitlyUnpaired),
    Compound(RespondentCompound),
    Serial(RespondentSerial),
    Iti(RespondentIti),
    Differential(RespondentDifferential),
    Extension(RespondentExtensionCall),
    Annotated(Box<AnnotatedRespondent>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Duration {
    pub value: f64,
    pub unit: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsArg {
    pub kind: CsArgKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CsArgKind {
    Ref(String),
    Compound(Vec<String>),
    Serial { stimuli: Vec<String>, isis: Vec<Duration> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairForwardDelay {
    pub cs: CsArg,
    pub us: String,
    pub isi: Duration,
    pub cs_duration: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairForwardTrace {
    pub cs: CsArg,
    pub us: String,
    pub cs_duration: Duration,
    pub trace_interval: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairSimultaneous {
    pub cs: CsArg,
    pub us: String,
    pub duration: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PairBackward {
    pub cs: CsArg,
    pub us: String,
    pub cs_duration: Duration,
    pub backward_interval: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Extinction {
    pub cs: String,
    pub trials: Option<i64>,
    pub iti: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CsOnly {
    pub cs: String,
    pub cs_duration: Duration,
    pub trials: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsOnly {
    pub us: String,
    pub trials: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Contingency {
    pub cs: String,
    pub us: String,
    pub p_us_given_cs: f64,
    pub p_us_no_cs: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrulyRandom {
    pub cs: String,
    pub us: String,
    pub rate_cs: f64,
    pub rate_us: f64,
    pub p: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExplicitlyUnpaired {
    pub cs: String,
    pub us: String,
    pub min_separation: Duration,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RespondentCompound {
    pub stimuli: Vec<String>,
    pub mode: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RespondentSerial {
    pub stimuli: Vec<String>,
    pub isis: Vec<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RespondentIti {
    pub mode: String,
    pub mean: Option<Duration>,
    pub fixed: Option<Duration>,
    pub uniform: Option<(Duration, Duration)>,
    pub exponential: Option<Duration>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RespondentDifferential {
    pub cs_plus: String,
    pub cs_minus: String,
    pub us: String,
    pub isi: Duration,
    pub cs_duration: Duration,
}

/// Call into a respondent extension registry (Tier B primitive).
#[derive(Debug, Clone, PartialEq)]
pub struct RespondentExtensionCall {
    pub identifier: String,
    pub positional: Vec<JsonValue>,
    pub keyword: BTreeMap<String, JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotatedRespondent {
    pub expr: RespondentExpr,
    pub annotations: Vec<Annotation>,
}

// --- Program-level nodes ---

#[derive(Debug, Clone, PartialEq)]
pub struct ParamDecl {
    pub name: String,
    pub value: f64,
    pub time_unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub name: String,
    pub value: ScheduleExpr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub program_annotations: Vec<Annotation>,
    pub param_decls: Vec<ParamDecl>,
    pub bindings: Vec<Binding>,
    pub schedule: ProgramSchedule,
}

/// Top-level schedule: operant expression, respondent expression, or empty.
#[derive(Debug, Clone, PartialEq)]
pub enum ProgramSchedule {
    Operant(ScheduleExpr),
    Respondent(RespondentExpr),
    None,
}

impl Default for ProgramSchedule {
    fn default() -> Self {
        Self::Operant(ScheduleExpr::Special(Special { kind: SpecialKind::Ext }))
    }
}

// --- Minimal owned JSON value used by payload carriers ---

/// Minimal owned JSON value used by payload carriers (PunishParam, TrialParam,
/// annotation structured values, and the respondent extension registry).
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}
