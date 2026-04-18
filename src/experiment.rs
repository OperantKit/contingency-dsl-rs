//! AST nodes for the contingency-dsl experiment layer.
//!
//! Mirrors `contingency_dsl/experiment.py`. A `PhaseSequence` is an ordered
//! list of `Phase` objects, each pairing a schedule with a stopping criterion
//! and phase-local annotations. AST shapes mirror
//! `contingency-dsl/schema/experiment/` JSON schemas.

use crate::ast::{Annotation, JsonValue, RespondentExpr, ScheduleExpr};

// --- Stopping criteria ---

#[derive(Debug, Clone, PartialEq)]
pub struct FixedSessions {
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stability {
    pub method: String,
    pub window_sessions: Option<i64>,
    pub max_change_pct: Option<f64>,
    pub min_sessions: Option<i64>,
    pub measure: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExperimenterJudgment;

#[derive(Debug, Clone, PartialEq)]
pub struct CumulativeReinforcements {
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PerformanceCriterion {
    pub measure: String,
    pub threshold: f64,
    pub window_sessions: Option<i64>,
    pub threshold_unit: Option<String>,
    pub comparator: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Criterion {
    FixedSessions(FixedSessions),
    Stability(Stability),
    ExperimenterJudgment(ExperimenterJudgment),
    CumulativeReinforcements(CumulativeReinforcements),
    PerformanceCriterion(PerformanceCriterion),
}

// --- Phase schedule slots ---

#[derive(Debug, Clone, PartialEq)]
pub enum PhaseSchedule {
    Operant(ScheduleExpr),
    Respondent(RespondentExpr),
    PhaseRef(PhaseRef),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseRef {
    pub target_label: String,
}

// --- Phase & sequence ---

#[derive(Debug, Clone, PartialEq)]
pub struct Phase {
    pub label: String,
    pub schedule: PhaseSchedule,
    pub criterion: Criterion,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhaseSequence {
    pub phases: Vec<Phase>,
    pub program_annotations: Vec<Annotation>,
}

// --- Paper & Experiment ---

#[derive(Debug, Clone, PartialEq)]
pub struct Experiment {
    pub label: String,
    pub phases: Vec<Phase>,
    pub annotations: Vec<Annotation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    pub experiments: Vec<Experiment>,
    pub shared_annotations: Vec<Annotation>,
    pub program_annotations: Vec<Annotation>,
    /// Raw payload preserved for fields not yet explicitly modeled (e.g.
    /// cross-experiment references).
    pub extra: Option<JsonValue>,
}
