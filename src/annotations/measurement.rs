//! `measurement-ext` annotation extension.
//!
//! Ported from `contingency_dsl/annotations/measurement/`. Phase 1 of the
//! Python module covers `@session_end`, `@baseline`, `@steady_state`;
//! Phase 2 registers additional keywords whose validators live alongside
//! here (and are dispatched by [`validate_measurement_annotation`]).

use crate::annotations::util::{
    coerce_duration_unit, coerce_duration_value, param, param_int, param_number, param_str,
};
use crate::ast::{Annotation, AnnotationValue, JsonValue};
use crate::extensions::AnnotationExtension;

// ---------------------------------------------------------------------------
// Typed annotation payloads (Phase 1).
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEndRule {
    First,
    TimeOnly,
    ReinforcersOnly,
}

impl SessionEndRule {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "first" => Some(Self::First),
            "time_only" => Some(Self::TimeOnly),
            "reinforcers_only" => Some(Self::ReinforcersOnly),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionEndAnnotation {
    pub rule: SessionEndRule,
    pub time: Option<f64>,
    pub time_unit: Option<String>,
    pub reinforcers: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineMeasure {
    OperantLevel,
    NoTraining,
    Custom,
}

impl BaselineMeasure {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "operant_level" => Some(Self::OperantLevel),
            "no_training" => Some(Self::NoTraining),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineAnnotation {
    pub pre_training_sessions: i64,
    pub measure: BaselineMeasure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteadyStateMeasure {
    Rate,
    Reinforcers,
    Iri,
    Latency,
    Custom,
}

impl SteadyStateMeasure {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rate" => Some(Self::Rate),
            "reinforcers" => Some(Self::Reinforcers),
            "iri" => Some(Self::Iri),
            "latency" => Some(Self::Latency),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SteadyStateAnnotation {
    pub window_sessions: i64,
    pub max_change_pct: f64,
    pub measure: SteadyStateMeasure,
    pub min_sessions: Option<i64>,
}

// ---------------------------------------------------------------------------
// Extension.
// ---------------------------------------------------------------------------

const KEYWORDS: &[&str] = &[
    // Phase 1
    "session_end",
    "baseline",
    "steady_state",
    // Phase 2 (registered, validated via dispatcher)
    "dependent_measure",
    "training_volume",
    "microstructure",
    "phase_end",
    "logging",
    "iri_window",
    "warmup_exclude",
];

#[derive(Debug, Default, Clone, Copy)]
pub struct MeasurementExtension;

impl MeasurementExtension {
    pub const NAME: &'static str = "measurement-ext";
    pub const VERSION: &'static str = "0.1.0";
}

impl AnnotationExtension for MeasurementExtension {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn version(&self) -> &'static str {
        Self::VERSION
    }
    fn annotation_keywords(&self) -> &'static [&'static str] {
        KEYWORDS
    }
    fn validate(&self, annotations: &[Annotation]) -> Vec<String> {
        let mut diag: Vec<String> = Vec::new();
        for ann in annotations {
            if !KEYWORDS.contains(&ann.keyword.as_str()) {
                continue;
            }
            for d in validate_measurement_annotation(&ann.keyword, ann, None) {
                diag.push(format!("{}: {}", d.code, d.message));
            }
        }
        diag
    }
}

// ---------------------------------------------------------------------------
// Diagnostic record and dispatch.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationDiagnostic {
    pub code: String,
    pub message: String,
}

impl ValidationDiagnostic {
    fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

/// Scope hint for scope-sensitive keywords (`@operandum`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnnotationScope {
    Program,
    Schedule,
}

impl AnnotationScope {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Schedule => "schedule",
        }
    }
}

/// Dispatch by keyword. Mirrors the Python `KEYWORD_VALIDATORS` table
/// combined with the positional/scope-aware wrappers.
pub fn validate_measurement_annotation(
    keyword: &str,
    ann: &Annotation,
    scope: Option<AnnotationScope>,
) -> Vec<ValidationDiagnostic> {
    match keyword {
        "session_end" => validate_session_end(ann),
        "baseline" => validate_baseline(ann),
        "steady_state" => validate_steady_state(ann),
        "dependent_measure" => validate_dependent_measure(ann),
        "training_volume" => validate_training_volume(ann),
        "microstructure" => validate_microstructure(ann),
        "phase_end" => validate_phase_end(ann),
        "logging" => validate_logging(ann),
        "iri_window" => validate_iri_window(ann),
        "warmup_exclude" => validate_warmup_exclude(ann),
        "training" => validate_training(ann),
        "testing" => validate_testing(ann),
        "clock" => validate_clock(ann),
        "warmup" => validate_warmup(ann),
        "algorithm" => validate_algorithm(ann),
        "deprivation" => validate_deprivation(ann),
        "reinforcer" => validate_reinforcer(ann),
        "brief" => validate_brief(ann),
        "session" => validate_session(ann),
        "probe_policy" => validate_probe_policy(ann),
        "stimulus_classes" => validate_stimulus_classes(ann),
        "n" => validate_n(ann),
        "operandum" => validate_operandum(ann, scope),
        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Validator implementations (one per keyword, ported closely).
// ---------------------------------------------------------------------------

fn validate_session_end(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let rule = param_str(ann, "rule");
    if rule.is_none() {
        d.push(ValidationDiagnostic::new(
            "SESSION_END_MISSING_PARAM",
            "@session_end requires 'rule' parameter",
        ));
        return d;
    }
    let rule = rule.unwrap();
    let valid_rules = ["first", "reinforcers_only", "time_only"]; // sorted
    if !["first", "time_only", "reinforcers_only"].contains(&rule) {
        d.push(ValidationDiagnostic::new(
            "SESSION_END_UNKNOWN_RULE",
            format!(
                "@session_end rule must be one of: {}",
                valid_rules.join(", ")
            ),
        ));
        return d;
    }
    let has_time = param(ann, "time").is_some();
    let has_reinforcers = param(ann, "reinforcers").is_some();
    match rule {
        "first" => {
            if !has_time || !has_reinforcers {
                d.push(ValidationDiagnostic::new(
                    "SESSION_END_MISSING_PARAM",
                    "@session_end rule='first' requires both 'time' and 'reinforcers'",
                ));
            }
        }
        "time_only" => {
            if !has_time {
                d.push(ValidationDiagnostic::new(
                    "SESSION_END_MISSING_PARAM",
                    "@session_end rule='time_only' requires 'time'",
                ));
            }
            if has_reinforcers {
                d.push(ValidationDiagnostic::new(
                    "SESSION_END_INVALID_COMBO",
                    "@session_end rule='time_only' forbids 'reinforcers'",
                ));
            }
        }
        "reinforcers_only" => {
            if !has_reinforcers {
                d.push(ValidationDiagnostic::new(
                    "SESSION_END_MISSING_PARAM",
                    "@session_end rule='reinforcers_only' requires 'reinforcers'",
                ));
            }
            if has_time {
                d.push(ValidationDiagnostic::new(
                    "SESSION_END_INVALID_COMBO",
                    "@session_end rule='reinforcers_only' forbids 'time'",
                ));
            }
        }
        _ => {}
    }
    d
}

fn validate_baseline(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let sessions = param_number(ann, "pre_training_sessions");
    match sessions {
        None => d.push(ValidationDiagnostic::new(
            "BASELINE_INVALID_SESSIONS",
            "@baseline requires 'pre_training_sessions'",
        )),
        Some(s) if s < 1.0 => d.push(ValidationDiagnostic::new(
            "BASELINE_INVALID_SESSIONS",
            "@baseline pre_training_sessions must be a positive integer",
        )),
        _ => {}
    }
    if let Some(m) = param_str(ann, "measure") {
        if !["operant_level", "no_training", "custom"].contains(&m) {
            d.push(ValidationDiagnostic::new(
                "BASELINE_UNKNOWN_MEASURE",
                "@baseline measure must be one of: custom, no_training, operant_level",
            ));
        }
    }
    d
}

fn validate_steady_state(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let window = param_number(ann, "window_sessions");
    match window {
        None => d.push(ValidationDiagnostic::new(
            "STEADY_STATE_INVALID_WINDOW",
            "@steady_state requires 'window_sessions'",
        )),
        Some(w) if w < 2.0 => d.push(ValidationDiagnostic::new(
            "STEADY_STATE_INVALID_WINDOW",
            "@steady_state window_sessions must be >= 2",
        )),
        _ => {}
    }
    let max_change = param_number(ann, "max_change_pct");
    match max_change {
        None => d.push(ValidationDiagnostic::new(
            "STEADY_STATE_NEGATIVE_CHANGE",
            "@steady_state requires 'max_change_pct'",
        )),
        Some(m) if m < 0.0 => d.push(ValidationDiagnostic::new(
            "STEADY_STATE_NEGATIVE_CHANGE",
            "@steady_state max_change_pct must be non-negative",
        )),
        _ => {}
    }
    if let (Some(min_s), Some(w)) = (param_number(ann, "min_sessions"), window) {
        if min_s < w {
            d.push(ValidationDiagnostic::new(
                "STEADY_STATE_INVALID_MIN_SESSIONS",
                "@steady_state min_sessions must be >= window_sessions",
            ));
        }
    }
    if let Some(m) = param_str(ann, "measure") {
        if !["rate", "reinforcers", "iri", "latency", "custom"].contains(&m) {
            d.push(ValidationDiagnostic::new(
                "STEADY_STATE_UNKNOWN_MEASURE",
                "@steady_state measure must be one of: custom, iri, latency, rate, reinforcers",
            ));
        }
    }
    d
}

fn validate_dependent_measure(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let valid: &[&str] = &[
        "rate",
        "reinforcers",
        "response_count",
        "iri",
        "latency",
        "irt",
        "prf_pause",
        "duration",
        "preference",
        "choice_ratio",
        "cumulative_responses",
    ];
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "variables") {
        if xs.is_empty() {
            d.push(ValidationDiagnostic::new(
                "DEPENDENT_MEASURE_EMPTY_VARIABLES",
                "@dependent_measure requires a non-empty 'variables' list",
            ));
        } else {
            for v in xs {
                if let JsonValue::String(s) = v {
                    if !valid.contains(&s.as_str()) {
                        d.push(ValidationDiagnostic::new(
                            "DEPENDENT_MEASURE_UNKNOWN_VARIABLE",
                            format!(
                                "@dependent_measure variable '{}' is not recognised",
                                s
                            ),
                        ));
                    }
                }
            }
        }
    }
    if let (Some(primary), Some(AnnotationValue::Json(JsonValue::Array(xs)))) =
        (param_str(ann, "primary"), param(ann, "variables"))
    {
        let present = xs.iter().any(|v| matches!(v, JsonValue::String(s) if s == primary));
        if !present {
            d.push(ValidationDiagnostic::new(
                "DEPENDENT_MEASURE_PRIMARY_NOT_IN_VARIABLES",
                format!(
                    "@dependent_measure primary='{}' must appear in variables",
                    primary
                ),
            ));
        }
    }
    d
}

fn validate_training_volume(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let valid: &[&str] = &["sessions", "reinforcers", "time", "responses"];
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "track") {
        if xs.is_empty() {
            d.push(ValidationDiagnostic::new(
                "TRAINING_VOLUME_EMPTY_TRACK",
                "@training_volume requires a non-empty 'track' list",
            ));
        } else {
            for v in xs {
                if let JsonValue::String(s) = v {
                    if !valid.contains(&s.as_str()) {
                        d.push(ValidationDiagnostic::new(
                            "TRAINING_VOLUME_UNKNOWN_METRIC",
                            format!("@training_volume metric '{}' is not recognised", s),
                        ));
                    }
                }
            }
        }
    }
    for key in [
        "criterion_sessions",
        "criterion_reinforcers",
        "criterion_time",
        "criterion_responses",
    ] {
        if let Some(v) = param(ann, key) {
            let n = if key.contains("_time") {
                coerce_duration_value(v)
            } else {
                match v {
                    AnnotationValue::Int(i) => Some(*i as f64),
                    AnnotationValue::Float(f) => Some(*f),
                    _ => None,
                }
            };
            match n {
                Some(num) if num > 0.0 => {}
                _ => d.push(ValidationDiagnostic::new(
                    "TRAINING_VOLUME_NONPOSITIVE_CRITERION",
                    format!("@training_volume {} must be positive", key),
                )),
            }
        }
    }
    d
}

fn validate_microstructure(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let method = param_str(ann, "prf_pause_method");
    let valid_methods: &[&str] = &["latency_to_first", "latency_to_nth", "quarter_life", "breakpoint"];
    if let Some(m) = method {
        if !valid_methods.contains(&m) {
            d.push(ValidationDiagnostic::new(
                "MICROSTRUCTURE_UNKNOWN_METHOD",
                format!(
                    "@microstructure prf_pause_method '{}' is not recognised",
                    m
                ),
            ));
        }
    }
    let n_present = param(ann, "prf_pause_n").is_some();
    if method == Some("latency_to_nth") && !n_present {
        d.push(ValidationDiagnostic::new(
            "MICROSTRUCTURE_NTH_MISSING",
            "@microstructure prf_pause_method='latency_to_nth' requires prf_pause_n",
        ));
    }
    if method == Some("latency_to_first") && n_present {
        d.push(ValidationDiagnostic::new(
            "MICROSTRUCTURE_NTH_FORBIDDEN",
            "@microstructure prf_pause_method='latency_to_first' forbids prf_pause_n",
        ));
    }
    let valid_reports: &[&str] = &["irt_distribution", "response_bout", "burst_pause", "rate_histogram"];
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "report") {
        for v in xs {
            if let JsonValue::String(r) = v {
                if !valid_reports.contains(&r.as_str()) {
                    d.push(ValidationDiagnostic::new(
                        "MICROSTRUCTURE_UNKNOWN_REPORT",
                        format!("@microstructure report '{}' is not recognised", r),
                    ));
                }
            }
        }
    }
    if let Some(v) = param(ann, "irt_bin_width") {
        if let Some(n) = coerce_duration_value(v) {
            if n <= 0.0 {
                d.push(ValidationDiagnostic::new(
                    "MICROSTRUCTURE_NONPOSITIVE_BIN",
                    "@microstructure irt_bin_width must be positive",
                ));
            }
        }
    }
    if let Some(v) = param(ann, "burst_threshold") {
        if let Some(n) = coerce_duration_value(v) {
            if n <= 0.0 {
                d.push(ValidationDiagnostic::new(
                    "MICROSTRUCTURE_NONPOSITIVE_THRESHOLD",
                    "@microstructure burst_threshold must be positive",
                ));
            }
        }
    }
    d
}

fn validate_phase_end(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let params_ref = ann.params.as_ref();
    let has = |k: &str| params_ref.map(|m| m.contains_key(k)).unwrap_or(false);

    let rule = param_str(ann, "rule");
    if let Some(r) = rule {
        if !["all", "any", "first"].contains(&r) {
            d.push(ValidationDiagnostic::new(
                "PHASE_END_UNKNOWN_RULE",
                format!("@phase_end rule='{}' is not recognised", r),
            ));
        }
    }
    let stability = param_str(ann, "stability");
    if let Some(s) = stability {
        if !["steady_state", "moving_average", "none"].contains(&s) {
            d.push(ValidationDiagnostic::new(
                "PHASE_END_UNKNOWN_STABILITY",
                format!("@phase_end stability='{}' is not recognised", s),
            ));
        }
    }
    let any_criterion = ["min_sessions", "max_sessions", "reinforcers", "stability", "time"]
        .iter()
        .any(|k| has(k));
    if !any_criterion {
        d.push(ValidationDiagnostic::new(
            "PHASE_END_NO_CRITERIA",
            "@phase_end requires at least one stop criterion",
        ));
    }
    if let (Some(minv), Some(maxv)) = (
        param_number(ann, "min_sessions"),
        param_number(ann, "max_sessions"),
    ) {
        if maxv < minv {
            d.push(ValidationDiagnostic::new(
                "PHASE_END_MAX_LESS_THAN_MIN",
                "@phase_end max_sessions must be >= min_sessions",
            ));
        }
    }
    if stability == Some("steady_state") && !has("_steady_state_present") {
        d.push(ValidationDiagnostic::new(
            "PHASE_END_MISSING_STEADY_STATE",
            "@phase_end stability='steady_state' requires a companion @steady_state annotation in the same scope",
        ));
    }
    d
}

fn validate_logging(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let valid_events: &[&str] = &[
        "response", "reinforcer", "stimulus", "session_start", "session_end",
        "trial_start", "trial_end", "phase_start", "phase_end", "changeover",
        "punisher", "brief",
    ];
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "events") {
        if xs.is_empty() {
            d.push(ValidationDiagnostic::new(
                "LOGGING_EMPTY_EVENTS",
                "@logging requires a non-empty 'events' list",
            ));
        } else {
            for v in xs {
                if let JsonValue::String(e) = v {
                    if !valid_events.contains(&e.as_str()) {
                        d.push(ValidationDiagnostic::new(
                            "LOGGING_UNKNOWN_EVENT",
                            format!("@logging event '{}' is not recognised", e),
                        ));
                    }
                }
            }
        }
    }
    if let Some(fmt) = param_str(ann, "format") {
        if !["json", "jsonl", "csv", "tsv"].contains(&fmt) {
            d.push(ValidationDiagnostic::new(
                "LOGGING_UNKNOWN_FORMAT",
                format!("@logging format='{}' is not recognised", fmt),
            ));
        }
    }
    if let Some(p) = param_str(ann, "precision") {
        if !["s", "ms", "us", "ns"].contains(&p) {
            d.push(ValidationDiagnostic::new(
                "LOGGING_UNKNOWN_PRECISION",
                format!("@logging precision='{}' is not recognised", p),
            ));
        }
    }
    d
}

fn validate_iri_window(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(n) = param_str(ann, "normalize") {
        if !["none", "mean", "max"].contains(&n) {
            d.push(ValidationDiagnostic::new(
                "IRI_WINDOW_UNKNOWN_NORMALIZE",
                format!("@iri_window normalize='{}' is not recognised", n),
            ));
        }
    }
    let valid_reports: &[&str] = &["mean", "median", "cv", "sd", "min", "max", "quantiles"];
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "report") {
        for v in xs {
            if let JsonValue::String(r) = v {
                if !valid_reports.contains(&r.as_str()) {
                    d.push(ValidationDiagnostic::new(
                        "IRI_WINDOW_UNKNOWN_REPORT",
                        format!("@iri_window report '{}' is not recognised", r),
                    ));
                }
            }
        }
    }
    if let Some(v) = param(ann, "bin_width") {
        if let Some(n) = coerce_duration_value(v) {
            if n <= 0.0 {
                d.push(ValidationDiagnostic::new(
                    "IRI_WINDOW_NONPOSITIVE_BIN",
                    "@iri_window bin_width must be positive",
                ));
            }
        }
    }
    d
}

fn validate_warmup_exclude(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let dur_sec = param(ann, "duration").and_then(duration_seconds);
    if let Some(n) = dur_sec {
        if n <= 0.0 {
            d.push(ValidationDiagnostic::new(
                "WARMUP_EXCLUDE_NONPOSITIVE",
                "@warmup_exclude duration must be positive",
            ));
        }
    }
    if let Some(s) = param_str(ann, "scope") {
        if !["first_session", "every_session", "trial"].contains(&s) {
            d.push(ValidationDiagnostic::new(
                "WARMUP_EXCLUDE_UNKNOWN_SCOPE",
                format!("@warmup_exclude scope='{}' is not recognised", s),
            ));
        }
    }
    if let (Some(dur), Some(session_s)) = (dur_sec, param_number(ann, "_session_end_seconds")) {
        if dur >= session_s {
            d.push(ValidationDiagnostic::new(
                "WARMUP_EXCLUDE_EXCEEDS_SESSION",
                "@warmup_exclude duration must be less than @session_end time",
            ));
        }
    }
    d
}

fn validate_training(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "relations") {
        if xs.is_empty() {
            d.push(ValidationDiagnostic::new(
                "TRAINING_EMPTY_RELATIONS",
                "@training requires a non-empty 'relations' list",
            ));
        }
    }
    if let Some(c) = param_number(ann, "criterion") {
        if c <= 0.0 || c > 100.0 {
            d.push(ValidationDiagnostic::new(
                "TRAINING_INVALID_CRITERION",
                "@training criterion must be in (0, 100]",
            ));
        }
    }
    if let Some(b) = param_number(ann, "block_size") {
        if b <= 0.0 {
            d.push(ValidationDiagnostic::new(
                "TRAINING_NONPOSITIVE_BLOCK_SIZE",
                "@training block_size must be positive",
            ));
        }
    }
    d
}

fn validate_testing(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(AnnotationValue::Json(JsonValue::Array(xs))) = param(ann, "relations") {
        if xs.is_empty() {
            d.push(ValidationDiagnostic::new(
                "TESTING_EMPTY_RELATIONS",
                "@testing requires a non-empty 'relations' list",
            ));
        }
    }
    if let Some(p) = param_number(ann, "probe_ratio") {
        if !(0.0..=1.0).contains(&p) {
            d.push(ValidationDiagnostic::new(
                "TESTING_INVALID_PROBE_RATIO",
                "@testing probe_ratio must be in [0, 1]",
            ));
        }
    }
    if let Some(c) = param_number(ann, "criterion") {
        if c <= 0.0 || c > 100.0 {
            d.push(ValidationDiagnostic::new(
                "TESTING_INVALID_CRITERION",
                "@testing criterion must be in (0, 100]",
            ));
        }
    }
    if let Some(b) = param_number(ann, "block_size") {
        if b <= 0.0 {
            d.push(ValidationDiagnostic::new(
                "TESTING_NONPOSITIVE_BLOCK_SIZE",
                "@testing block_size must be positive",
            ));
        }
    }
    d
}

fn validate_clock(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(u) = param_str(ann, "unit") {
        if !["s", "ms", "min"].contains(&u) {
            d.push(ValidationDiagnostic::new(
                "CLOCK_UNKNOWN_UNIT",
                format!("@clock unit='{}' is not recognised (valid: s, ms, min)", u),
            ));
        }
    }
    d
}

fn validate_warmup(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(v) = param(ann, "duration") {
        if let Some(n) = coerce_duration_value(v) {
            if n <= 0.0 {
                d.push(ValidationDiagnostic::new(
                    "WARMUP_NONPOSITIVE_DURATION",
                    "@warmup duration must be positive",
                ));
            }
        }
    }
    d
}

fn validate_algorithm(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(n) = param_number(ann, "n") {
        if n <= 0.0 {
            d.push(ValidationDiagnostic::new(
                "ALGORITHM_NONPOSITIVE_N",
                "@algorithm n must be positive",
            ));
        }
    }
    d
}

fn validate_deprivation(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(h) = param_number(ann, "hours") {
        if h < 0.0 {
            d.push(ValidationDiagnostic::new(
                "DEPRIVATION_NEGATIVE_HOURS",
                "@deprivation hours must be non-negative",
            ));
        }
    }
    d
}

fn validate_n(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let pos = match &ann.positional {
        Some(AnnotationValue::Int(i)) => Some(*i as f64),
        Some(AnnotationValue::Float(f)) => Some(*f),
        _ => None,
    };
    let v = pos.or_else(|| param_number(ann, "n"));
    if let Some(val) = v {
        if val <= 0.0 {
            d.push(ValidationDiagnostic::new("N_NONPOSITIVE", "@n must be positive"));
        }
    }
    d
}

fn validate_reinforcer(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(t) = param_str(ann, "type") {
        let valid: &[&str] = &["food", "water", "drug", "social", "token", "activity"];
        if !valid.contains(&t) {
            d.push(ValidationDiagnostic::new(
                "REINFORCER_UNKNOWN_TYPE",
                format!("@reinforcer type='{}' is not recognised", t),
            ));
        }
    }
    if let Some(v) = param(ann, "duration") {
        if let Some(n) = coerce_duration_value(v) {
            if n < 0.0 {
                d.push(ValidationDiagnostic::new(
                    "REINFORCER_NEGATIVE_DURATION",
                    "@reinforcer duration must be non-negative",
                ));
            }
        }
    }
    d
}

fn validate_brief(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(v) = param(ann, "duration") {
        if let Some(n) = coerce_duration_value(v) {
            if n < 0.0 {
                d.push(ValidationDiagnostic::new(
                    "BRIEF_NEGATIVE_DURATION",
                    "@brief duration must be non-negative",
                ));
            }
        }
    }
    d
}

fn validate_session(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let params_ref = ann.params.as_ref();
    if params_ref.map(|m| m.is_empty()).unwrap_or(true) {
        d.push(ValidationDiagnostic::new(
            "SESSION_MISSING_PARAM",
            "@session requires at least one of trials / blocks / block_size",
        ));
        return d;
    }
    let trials = param_number(ann, "trials");
    let blocks = param_number(ann, "blocks");
    let block_size = param_number(ann, "block_size");
    if let Some(t) = trials {
        if t <= 0.0 {
            d.push(ValidationDiagnostic::new(
                "SESSION_NONPOSITIVE_TRIALS",
                "@session trials must be positive",
            ));
        }
    }
    if let Some(b) = blocks {
        if b <= 0.0 {
            d.push(ValidationDiagnostic::new(
                "SESSION_NONPOSITIVE_BLOCKS",
                "@session blocks must be positive",
            ));
        }
    }
    if blocks.is_some() && block_size.is_none() && trials.is_none() {
        d.push(ValidationDiagnostic::new(
            "SESSION_BLOCKS_WITHOUT_BLOCK_SIZE",
            "@session blocks= requires block_size=",
        ));
    }
    if let (Some(t), Some(b), Some(bs)) = (trials, blocks, block_size) {
        if t != b * bs {
            d.push(ValidationDiagnostic::new(
                "SESSION_BLOCK_TRIALS_MISMATCH",
                format!(
                    "@session trials ({}) != blocks ({}) * block_size ({})",
                    fmt_num(t),
                    fmt_num(b),
                    fmt_num(bs)
                ),
            ));
        }
    }
    d
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

fn validate_probe_policy(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    if let Some(r) = param_number(ann, "probe_ratio") {
        if !(0.0..=1.0).contains(&r) {
            d.push(ValidationDiagnostic::new(
                "PROBE_POLICY_INVALID_RATIO",
                "@probe_policy probe_ratio must be in [0, 1]",
            ));
        }
    }
    let order = param_str(ann, "order");
    if let Some(o) = order {
        if !["random", "blocked", "progressive"].contains(&o) {
            d.push(ValidationDiagnostic::new(
                "PROBE_POLICY_UNKNOWN_ORDER",
                format!("@probe_policy order='{}' is not recognised", o),
            ));
        }
    }
    let interspersed = match param(ann, "interspersed") {
        Some(AnnotationValue::Json(JsonValue::Bool(b))) => Some(*b),
        _ => None,
    };
    if interspersed == Some(true) && order == Some("blocked") {
        d.push(ValidationDiagnostic::new(
            "PROBE_POLICY_INCONSISTENT_INTERSPERSED_ORDER",
            "@probe_policy interspersed=true is inconsistent with order='blocked'",
        ));
    }
    d
}

fn validate_stimulus_classes(ann: &Annotation) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    // Positional integer lower-bound.
    let pos_num = match &ann.positional {
        Some(AnnotationValue::Int(i)) => Some(*i as f64),
        Some(AnnotationValue::Float(f)) => Some(*f),
        _ => None,
    };
    if let Some(n) = pos_num {
        if n < 2.0 {
            d.push(ValidationDiagnostic::new(
                "STIMULUS_CLASSES_TOO_FEW",
                "@stimulus_classes requires at least 2 classes",
            ));
        }
    }
    // Keyword form A=[...] B=[...].
    let mut class_members: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    if let Some(params) = ann.params.as_ref() {
        for (k, v) in params {
            let is_class_label = k.chars().all(|c| c.is_ascii_uppercase()) && !k.is_empty();
            if !is_class_label {
                continue;
            }
            if let AnnotationValue::Json(JsonValue::Array(items)) = v {
                let mut members: Vec<String> = Vec::new();
                for it in items {
                    if let JsonValue::String(s) = it {
                        members.push(s.clone());
                    }
                }
                class_members.insert(k.clone(), members);
            }
        }
    }
    if !class_members.is_empty() {
        if class_members.len() < 2 {
            d.push(ValidationDiagnostic::new(
                "STIMULUS_CLASSES_TOO_FEW",
                "@stimulus_classes requires at least 2 classes",
            ));
        }
        let mut seen: std::collections::BTreeMap<String, String> = Default::default();
        for (cls, members) in &class_members {
            for m in members {
                if let Some(prev) = seen.get(m) {
                    if prev != cls {
                        d.push(ValidationDiagnostic::new(
                            "STIMULUS_CLASSES_DUPLICATE_STIMULUS",
                            format!(
                                "@stimulus_classes stimulus '{}' appears in both class '{}' and class '{}'",
                                m, prev, cls
                            ),
                        ));
                    }
                } else {
                    seen.insert(m.clone(), cls.clone());
                }
            }
        }
    }
    d
}

fn validate_operandum(ann: &Annotation, scope: Option<AnnotationScope>) -> Vec<ValidationDiagnostic> {
    let mut d: Vec<ValidationDiagnostic> = Vec::new();
    let physical_keys = [
        "width",
        "height",
        "depth",
        "distance",
        "force",
        "weight",
        "model",
        "manufacturer",
        "position",
        "orientation",
        "material",
    ];
    let has = |k: &str| ann.params.as_ref().map(|m| m.contains_key(k)).unwrap_or(false);
    let has_physical = physical_keys.iter().any(|k| has(k));
    if scope == Some(AnnotationScope::Schedule) && has_physical {
        d.push(ValidationDiagnostic::new(
            "SCHEDULE_ANNOTATION_PHYSICAL_PARAM",
            "physical parameters on @operandum are only allowed at program scope",
        ));
    }
    if scope == Some(AnnotationScope::Program) && has("component") {
        d.push(ValidationDiagnostic::new(
            "PROGRAM_ANNOTATION_COMPONENT",
            "'component=' on @operandum is only valid at schedule scope",
        ));
    }
    d
}

// ---------------------------------------------------------------------------
// Duration-to-seconds coercion (used by @warmup_exclude).
// ---------------------------------------------------------------------------

fn duration_seconds(v: &AnnotationValue) -> Option<f64> {
    const FACTORS: &[(&str, f64)] = &[
        ("ms", 0.001),
        ("s", 1.0),
        ("sec", 1.0),
        ("min", 60.0),
        ("h", 3600.0),
        ("hr", 3600.0),
    ];
    match v {
        AnnotationValue::Int(i) => Some(*i as f64),
        AnnotationValue::Float(f) => Some(*f),
        AnnotationValue::Json(JsonValue::Object(m)) => {
            let value = match m.get("value")? {
                JsonValue::Int(i) => *i as f64,
                JsonValue::Float(f) => *f,
                _ => return None,
            };
            if let Some(JsonValue::String(u)) = m.get("time_unit") {
                for (name, factor) in FACTORS {
                    if u == name {
                        return Some(value * factor);
                    }
                }
            }
            Some(value)
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Parsers for Phase 1 typed payloads.
// ---------------------------------------------------------------------------

pub fn parse_session_end(ann: &Annotation) -> Option<SessionEndAnnotation> {
    let rule_s = param_str(ann, "rule").unwrap_or("first");
    let rule = SessionEndRule::parse(rule_s)?;
    let (time, time_unit) = match param(ann, "time") {
        Some(AnnotationValue::Int(i)) => (Some(*i as f64), None),
        Some(AnnotationValue::Float(f)) => (Some(*f), None),
        Some(AnnotationValue::Json(JsonValue::Object(_))) => {
            let v = param(ann, "time").unwrap();
            (coerce_duration_value(v), coerce_duration_unit(v).map(String::from))
        }
        _ => (None, None),
    };
    let reinforcers = param_int(ann, "reinforcers");
    Some(SessionEndAnnotation {
        rule,
        time,
        time_unit,
        reinforcers,
    })
}

pub fn parse_baseline(ann: &Annotation) -> BaselineAnnotation {
    let sessions = param_number(ann, "pre_training_sessions").unwrap_or(1.0) as i64;
    let measure = param_str(ann, "measure")
        .and_then(BaselineMeasure::parse)
        .unwrap_or(BaselineMeasure::OperantLevel);
    BaselineAnnotation {
        pre_training_sessions: sessions,
        measure,
    }
}

pub fn parse_steady_state(ann: &Annotation) -> SteadyStateAnnotation {
    let window = param_number(ann, "window_sessions").unwrap_or(5.0) as i64;
    let max_change = param_number(ann, "max_change_pct").unwrap_or(10.0);
    let measure = param_str(ann, "measure")
        .and_then(SteadyStateMeasure::parse)
        .unwrap_or(SteadyStateMeasure::Rate);
    let min_sessions = param_int(ann, "min_sessions");
    SteadyStateAnnotation {
        window_sessions: window,
        max_change_pct: max_change,
        measure,
        min_sessions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Annotation, AnnotationValue};
    use std::collections::BTreeMap;

    fn make(kw: &str, positional: Option<AnnotationValue>, params: &[(&str, AnnotationValue)]) -> Annotation {
        let mut m = BTreeMap::new();
        for (k, v) in params {
            m.insert((*k).to_string(), v.clone());
        }
        Annotation {
            keyword: kw.into(),
            positional,
            params: if m.is_empty() { None } else { Some(m) },
            label: None,
        }
    }

    #[test]
    fn session_end_first_requires_both() {
        let a = make(
            "session_end",
            None,
            &[
                ("rule", AnnotationValue::Str("first".into())),
                ("reinforcers", AnnotationValue::Int(60)),
            ],
        );
        let d = validate_session_end(&a);
        assert_eq!(d[0].code, "SESSION_END_MISSING_PARAM");
    }

    #[test]
    fn session_end_unknown_rule() {
        let a = make(
            "session_end",
            None,
            &[("rule", AnnotationValue::Str("infinite".into()))],
        );
        let d = validate_session_end(&a);
        assert_eq!(d[0].code, "SESSION_END_UNKNOWN_RULE");
    }

    #[test]
    fn steady_state_invalid_window() {
        let a = make(
            "steady_state",
            None,
            &[
                ("window_sessions", AnnotationValue::Int(1)),
                ("max_change_pct", AnnotationValue::Int(10)),
            ],
        );
        let d = validate_steady_state(&a);
        assert_eq!(d[0].code, "STEADY_STATE_INVALID_WINDOW");
    }

    #[test]
    fn phase_end_no_criteria() {
        let a = make("phase_end", None, &[("rule", AnnotationValue::Str("all".into()))]);
        let d = validate_phase_end(&a);
        assert!(d.iter().any(|x| x.code == "PHASE_END_NO_CRITERIA"));
    }

    #[test]
    fn dispatcher_routes_unknown_keyword_to_empty() {
        let a = make("completely_unknown", None, &[]);
        assert!(validate_measurement_annotation("completely_unknown", &a, None).is_empty());
    }
}
