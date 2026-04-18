//! AST → JSON-shaped value serializer.
//!
//! Mirrors `contingency_dsl/serializer.py`. Produces [`JsonValue`] trees that
//! match the conformance test format under
//! `contingency-dsl/conformance/**/*.json` (`expected` field).

use std::collections::BTreeMap;

use crate::ast::{
    AdjustingSchedule, AnnotatedRespondent, AnnotatedSchedule, Annotation, AnnotationValue,
    Atomic, AversiveSchedule, Binding, CombinatorParamValue, Compound, CompoundParam,
    Contingency, CsArg, CsArgKind, CsOnly, DirRef, DirectionalCOD, DirectionalEntry, Duration,
    ExplicitlyUnpaired, Extinction, IdentifierRef, InterlockingSchedule, JsonValue, LimitedHold,
    Modifier, NumericPayload, PairBackward, PairForwardDelay, PairForwardTrace,
    PairSimultaneous, ParamDecl, Program, ProgramSchedule, PunishParam, RespondentCompound,
    RespondentDifferential, RespondentExpr, RespondentExtensionCall, RespondentIti,
    RespondentSerial, ResponseCost, ResponseCostWrapped, ScheduleExpr, ScheduleType, SecondOrder,
    Special, Timeout, TimeoutWrapped, TrialBased, TrialParam, TrulyRandom, UsOnly,
};
use crate::enums::Combinator;
use crate::experiment::{
    Criterion, CumulativeReinforcements, Experiment, ExperimenterJudgment, FixedSessions, Paper,
    PerformanceCriterion, Phase, PhaseRef, PhaseSchedule, PhaseSequence, Stability,
};

/// Serialize a Program to a JSON value matching the conformance fixture shape.
pub fn program_to_json(program: &Program) -> JsonValue {
    let schedule = &program.schedule;
    let mut hoisted: Option<Vec<JsonValue>> = None;

    let schedule_json = match schedule {
        ProgramSchedule::Operant(expr) => {
            // Hoist trailing annotations from stateful / trial-based / modifier
            // leaves onto the program per serializer.py.
            if let ScheduleExpr::Annotated(boxed) = expr {
                let AnnotatedSchedule { expr: inner, annotations } = boxed.as_ref();
                let hoistable = matches!(
                    inner,
                    ScheduleExpr::AdjustingSchedule(_)
                        | ScheduleExpr::InterlockingSchedule(_)
                        | ScheduleExpr::Modifier(_)
                        | ScheduleExpr::TrialBased(_)
                );
                // Declaration-mode MTS keeps annotations inline.
                let mts_decl = matches!(inner, ScheduleExpr::TrialBased(tb)
                    if tb.trial_type == "MTS" && tb.params.contains_key("samples"));
                if hoistable && !mts_decl {
                    hoisted = Some(
                        annotations
                            .iter()
                            .map(annotation_to_json)
                            .collect(),
                    );
                    expr_to_json(inner)
                } else {
                    expr_to_json(expr)
                }
            } else {
                expr_to_json(expr)
            }
        }
        ProgramSchedule::Respondent(r) => respondent_to_json(r),
        ProgramSchedule::None => JsonValue::Null,
    };

    let mut map = BTreeMap::new();
    map.insert("type".into(), JsonValue::String("Program".into()));
    map.insert(
        "program_annotations".into(),
        JsonValue::Array(
            program.program_annotations.iter().map(annotation_to_json).collect(),
        ),
    );
    map.insert(
        "param_decls".into(),
        JsonValue::Array(program.param_decls.iter().map(param_decl_to_json).collect()),
    );
    map.insert(
        "bindings".into(),
        JsonValue::Array(program.bindings.iter().map(binding_to_json).collect()),
    );
    map.insert("schedule".into(), schedule_json);
    if let Some(anns) = hoisted {
        map.insert("annotations".into(), JsonValue::Array(anns));
    }
    JsonValue::Object(map)
}

pub fn phase_sequence_to_json(seq: &PhaseSequence) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("PhaseSequence".into()));
    m.insert(
        "shared_annotations".into(),
        JsonValue::Array(seq.program_annotations.iter().map(annotation_to_json).collect()),
    );
    m.insert("shared_param_decls".into(), JsonValue::Array(vec![]));
    m.insert(
        "phases".into(),
        JsonValue::Array(seq.phases.iter().map(phase_to_json).collect()),
    );
    JsonValue::Object(m)
}

pub fn paper_to_json(paper: &Paper) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Paper".into()));
    m.insert(
        "shared_annotations".into(),
        JsonValue::Array(paper.shared_annotations.iter().map(annotation_to_json).collect()),
    );
    m.insert(
        "experiments".into(),
        JsonValue::Array(paper.experiments.iter().map(experiment_to_json).collect()),
    );
    JsonValue::Object(m)
}

fn experiment_to_json(exp: &Experiment) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Experiment".into()));
    m.insert("label".into(), JsonValue::String(exp.label.clone()));
    let phases: Vec<JsonValue> = exp.phases.iter().map(phase_to_json).collect();
    let body = JsonValue::Object({
        let mut b = BTreeMap::new();
        b.insert("type".into(), JsonValue::String("PhaseSequence".into()));
        b.insert(
            "shared_annotations".into(),
            JsonValue::Array(exp.annotations.iter().map(annotation_to_json).collect()),
        );
        b.insert("shared_param_decls".into(), JsonValue::Array(vec![]));
        b.insert("phases".into(), JsonValue::Array(phases));
        b
    });
    m.insert("body".into(), body);
    JsonValue::Object(m)
}

fn phase_to_json(phase: &Phase) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Phase".into()));
    m.insert("label".into(), JsonValue::String(phase.label.clone()));
    m.insert("criterion".into(), criterion_to_json(&phase.criterion));
    m.insert("schedule".into(), phase_schedule_to_json(&phase.schedule));
    if !phase.annotations.is_empty() {
        m.insert(
            "phase_annotations".into(),
            JsonValue::Array(phase.annotations.iter().map(annotation_to_json).collect()),
        );
    }
    JsonValue::Object(m)
}

fn phase_schedule_to_json(s: &PhaseSchedule) -> JsonValue {
    match s {
        PhaseSchedule::Operant(e) => expr_to_json(e),
        PhaseSchedule::Respondent(r) => respondent_to_json(r),
        PhaseSchedule::PhaseRef(PhaseRef { target_label }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("PhaseRef".into()));
            m.insert("ref".into(), JsonValue::String(target_label.clone()));
            JsonValue::Object(m)
        }
        PhaseSchedule::None => JsonValue::Null,
    }
}

fn criterion_to_json(c: &Criterion) -> JsonValue {
    let mut m = BTreeMap::new();
    match c {
        Criterion::FixedSessions(FixedSessions { count }) => {
            m.insert("type".into(), JsonValue::String("FixedSessions".into()));
            m.insert("count".into(), JsonValue::Int(*count));
        }
        Criterion::Stability(Stability {
            method,
            window_sessions,
            max_change_pct,
            min_sessions,
            measure,
        }) => {
            m.insert("type".into(), JsonValue::String("Stability".into()));
            m.insert("method".into(), JsonValue::String(method.clone()));
            if let Some(v) = min_sessions {
                m.insert("min_sessions".into(), JsonValue::Int(*v));
            }
            if let Some(v) = window_sessions {
                m.insert("window_sessions".into(), JsonValue::Int(*v));
            }
            if let Some(v) = max_change_pct {
                m.insert("max_change_pct".into(), JsonValue::Float(*v));
            }
            if let Some(v) = measure {
                m.insert("measure".into(), JsonValue::String(v.clone()));
            }
        }
        Criterion::ExperimenterJudgment(ExperimenterJudgment) => {
            m.insert("type".into(), JsonValue::String("ExperimenterJudgment".into()));
        }
        Criterion::CumulativeReinforcements(CumulativeReinforcements { count }) => {
            m.insert(
                "type".into(),
                JsonValue::String("CumulativeReinforcements".into()),
            );
            m.insert("count".into(), JsonValue::Int(*count));
        }
        Criterion::PerformanceCriterion(PerformanceCriterion {
            measure,
            threshold,
            window_sessions,
            threshold_unit: _,
            comparator,
        }) => {
            m.insert("type".into(), JsonValue::String("PerformanceCriterion".into()));
            m.insert("measure".into(), JsonValue::String(measure.clone()));
            m.insert("threshold".into(), JsonValue::Float(*threshold));
            if let Some(op) = comparator {
                m.insert("op".into(), JsonValue::String(op.clone()));
            }
            if let Some(v) = window_sessions {
                m.insert("window_sessions".into(), JsonValue::Int(*v));
            }
        }
    }
    JsonValue::Object(m)
}

fn param_decl_to_json(p: &ParamDecl) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("ParamDecl".into()));
    m.insert("name".into(), JsonValue::String(p.name.clone()));
    m.insert("value".into(), JsonValue::Float(p.value));
    if let Some(u) = &p.time_unit {
        m.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    JsonValue::Object(m)
}

fn binding_to_json(b: &Binding) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Binding".into()));
    m.insert("name".into(), JsonValue::String(b.name.clone()));
    m.insert("value".into(), expr_to_json(&b.value));
    JsonValue::Object(m)
}

/// Serialize a ScheduleExpr to its JSON shape.
pub fn expr_to_json(expr: &ScheduleExpr) -> JsonValue {
    match expr {
        ScheduleExpr::Atomic(a) => atomic_to_json(a),
        ScheduleExpr::Special(s) => special_to_json(s),
        ScheduleExpr::Compound(c) => compound_to_json(c),
        ScheduleExpr::Modifier(m) => modifier_to_json(m),
        ScheduleExpr::LimitedHold(lh) => limited_hold_to_json(lh),
        ScheduleExpr::SecondOrder(so) => second_order_to_json(so),
        ScheduleExpr::IdentifierRef(r) => identifier_ref_to_json(r),
        ScheduleExpr::AversiveSchedule(av) => aversive_to_json(av),
        ScheduleExpr::TimeoutWrapped(tw) => timeout_wrapped_to_json(tw),
        ScheduleExpr::ResponseCostWrapped(rw) => response_cost_wrapped_to_json(rw),
        ScheduleExpr::TrialBased(tb) => trial_based_to_json(tb),
        ScheduleExpr::AdjustingSchedule(adj) => adjusting_to_json(adj),
        ScheduleExpr::InterlockingSchedule(il) => interlocking_to_json(il),
        ScheduleExpr::Respondent(r) => respondent_to_json(r),
        ScheduleExpr::RespondentExtension(rx) => respondent_extension_to_json(rx),
        ScheduleExpr::Annotated(boxed) => annotated_schedule_to_json(boxed),
    }
}

fn atomic_to_json(a: &Atomic) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Atomic".into()));
    m.insert(
        "dist".into(),
        JsonValue::String(a.schedule_type.distribution.code().to_string()),
    );
    m.insert(
        "domain".into(),
        JsonValue::String(a.schedule_type.domain.code().to_string()),
    );
    m.insert("value".into(), JsonValue::Float(a.value));
    if let Some(u) = &a.time_unit {
        m.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    JsonValue::Object(m)
}

fn special_to_json(s: &Special) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Special".into()));
    m.insert("kind".into(), JsonValue::String(s.kind.as_str().into()));
    JsonValue::Object(m)
}

fn compound_to_json(c: &Compound) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("Compound".into()));
    m.insert(
        "combinator".into(),
        JsonValue::String(c.combinator.as_str().into()),
    );
    m.insert(
        "components".into(),
        JsonValue::Array(c.components.iter().map(expr_to_json).collect()),
    );
    if let Some(params) = &c.params {
        let mut pm = BTreeMap::new();
        for (k, v) in params {
            pm.insert(k.clone(), combinator_param_to_json(v));
        }
        m.insert("params".into(), JsonValue::Object(pm));
    }
    JsonValue::Object(m)
}

fn combinator_param_to_json(v: &CombinatorParamValue) -> JsonValue {
    match v {
        CombinatorParamValue::Scalar(cp) => compound_param_to_json(cp),
        CombinatorParamValue::Directional(DirectionalCOD { base, directional }) => {
            let mut m = BTreeMap::new();
            if let Some(b) = base {
                m.insert("base".into(), compound_param_to_json(b));
            }
            m.insert(
                "directional".into(),
                JsonValue::Array(directional.iter().map(directional_entry_to_json).collect()),
            );
            JsonValue::Object(m)
        }
        CombinatorParamValue::Punish(PunishParam { payload }) => payload.clone(),
        CombinatorParamValue::Raw(v) => v.clone(),
    }
}

fn compound_param_to_json(p: &CompoundParam) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("value".into(), JsonValue::Float(p.value));
    if let Some(u) = &p.time_unit {
        m.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    JsonValue::Object(m)
}

fn directional_entry_to_json(e: &DirectionalEntry) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("from".into(), dir_ref_to_json(&e.from_ref));
    m.insert("to".into(), dir_ref_to_json(&e.to_ref));
    m.insert("value".into(), JsonValue::Float(e.value));
    if let Some(u) = &e.time_unit {
        m.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    JsonValue::Object(m)
}

fn dir_ref_to_json(d: &DirRef) -> JsonValue {
    match d {
        DirRef::Index(i) => JsonValue::Int(*i),
        DirRef::Name(s) => JsonValue::String(s.clone()),
    }
}

fn modifier_to_json(m: &Modifier) -> JsonValue {
    let mut map = BTreeMap::new();
    map.insert("type".into(), JsonValue::String("Modifier".into()));
    map.insert(
        "modifier".into(),
        JsonValue::String(m.kind.as_str().into()),
    );
    if let Some(v) = m.value {
        map.insert("value".into(), JsonValue::Float(v));
    }
    if let Some(u) = &m.time_unit {
        map.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    if let Some(inner) = &m.inner {
        map.insert("inner".into(), expr_to_json(inner));
    }
    if let Some(s) = &m.pr_step {
        map.insert("pr_step".into(), JsonValue::String(s.clone()));
    }
    if let Some(v) = m.pr_start {
        map.insert("pr_start".into(), JsonValue::Float(v));
    }
    if let Some(v) = m.pr_increment {
        map.insert("pr_increment".into(), JsonValue::Float(v));
    }
    if let Some(v) = m.pr_ratio {
        map.insert("pr_ratio".into(), JsonValue::Float(v));
    }
    if let Some(s) = &m.pctl_target {
        map.insert("pctl_target".into(), JsonValue::String(s.clone()));
    }
    if let Some(v) = m.pctl_rank {
        map.insert("pctl_rank".into(), JsonValue::Int(v));
    }
    if let Some(v) = m.pctl_window {
        map.insert("pctl_window".into(), JsonValue::Int(v));
    }
    if let Some(s) = &m.pctl_dir {
        map.insert("pctl_dir".into(), JsonValue::String(s.clone()));
    }
    if let Some(v) = m.length {
        map.insert("length".into(), JsonValue::Int(v));
    }
    JsonValue::Object(map)
}

fn limited_hold_to_json(lh: &LimitedHold) -> JsonValue {
    // Flatten onto leaf when the inner is a leaf per schema/operant/ast.schema.json.
    let inner_ref = lh.inner.as_ref();
    let is_leaf = matches!(
        inner_ref,
        ScheduleExpr::Atomic(_)
            | ScheduleExpr::Special(_)
            | ScheduleExpr::Modifier(_)
            | ScheduleExpr::SecondOrder(_)
            | ScheduleExpr::AdjustingSchedule(_)
            | ScheduleExpr::InterlockingSchedule(_)
            | ScheduleExpr::AversiveSchedule(_)
            | ScheduleExpr::TrialBased(_)
    );
    let mut inner_json = expr_to_json(inner_ref);
    if is_leaf {
        if let JsonValue::Object(map) = &mut inner_json {
            map.insert("limitedHold".into(), JsonValue::Float(lh.hold_duration));
            if let Some(u) = &lh.time_unit {
                map.insert("limitedHoldUnit".into(), JsonValue::String(u.clone()));
            }
        }
        return inner_json;
    }
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("LimitedHold".into()));
    m.insert("hold_duration".into(), JsonValue::Float(lh.hold_duration));
    m.insert("inner".into(), inner_json);
    if let Some(u) = &lh.time_unit {
        m.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    JsonValue::Object(m)
}

fn second_order_to_json(so: &SecondOrder) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("SecondOrder".into()));
    m.insert("overall".into(), atomic_to_json(&so.overall));
    m.insert("unit".into(), atomic_to_json(&so.unit));
    JsonValue::Object(m)
}

fn identifier_ref_to_json(r: &IdentifierRef) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("IdentifierRef".into()));
    m.insert("name".into(), JsonValue::String(r.name.clone()));
    JsonValue::Object(m)
}

fn aversive_to_json(av: &AversiveSchedule) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("AversiveSchedule".into()));
    m.insert("kind".into(), JsonValue::String(av.kind.as_str().into()));
    let mut pm = BTreeMap::new();
    for (k, v) in &av.params {
        let entry = match v {
            crate::ast::AversiveParam::Numeric(NumericPayload { value, time_unit }) => {
                let mut nm = BTreeMap::new();
                nm.insert("value".into(), JsonValue::Float(*value));
                if let Some(u) = time_unit {
                    nm.insert("time_unit".into(), JsonValue::String(u.clone()));
                }
                JsonValue::Object(nm)
            }
            crate::ast::AversiveParam::Str(s) => JsonValue::String(s.clone()),
        };
        pm.insert(k.clone(), entry);
    }
    m.insert("params".into(), JsonValue::Object(pm));
    JsonValue::Object(m)
}

fn timeout_wrapped_to_json(tw: &TimeoutWrapped) -> JsonValue {
    let mut inner = expr_to_json(tw.inner.as_ref());
    if let JsonValue::Object(map) = &mut inner {
        let Timeout { duration, duration_unit, reset_on_response } = &tw.timeout;
        let mut tm = BTreeMap::new();
        tm.insert("duration".into(), JsonValue::Float(*duration));
        if let Some(u) = duration_unit {
            tm.insert("durationUnit".into(), JsonValue::String(u.clone()));
        }
        tm.insert("resetOnResponse".into(), JsonValue::Bool(*reset_on_response));
        map.insert("timeout".into(), JsonValue::Object(tm));
    }
    inner
}

fn response_cost_wrapped_to_json(rw: &ResponseCostWrapped) -> JsonValue {
    let mut inner = expr_to_json(rw.inner.as_ref());
    if let JsonValue::Object(map) = &mut inner {
        let ResponseCost { amount, unit } = &rw.response_cost;
        let mut rcm = BTreeMap::new();
        rcm.insert("amount".into(), JsonValue::Float(*amount));
        rcm.insert("unit".into(), JsonValue::String(unit.clone()));
        map.insert("responseCost".into(), JsonValue::Object(rcm));
    }
    inner
}

fn trial_based_to_json(tb: &TrialBased) -> JsonValue {
    let mut m = BTreeMap::new();
    if tb.trial_type == "MTS" && tb.params.contains_key("samples") {
        m.insert("type".into(), JsonValue::String("MTS".into()));
        let mut pm = BTreeMap::new();
        for key in ["samples", "comparisons"] {
            if let Some(v) = tb.params.get(key) {
                pm.insert(key.into(), trial_param_to_json(v));
            }
        }
        m.insert("params".into(), JsonValue::Object(pm));
        return JsonValue::Object(m);
    }
    m.insert("type".into(), JsonValue::String("TrialBased".into()));
    m.insert("trial_type".into(), JsonValue::String(tb.trial_type.clone()));
    for (k, v) in &tb.params {
        m.insert(k.clone(), trial_param_to_json(v));
    }
    JsonValue::Object(m)
}

fn trial_param_to_json(p: &TrialParam) -> JsonValue {
    match p {
        TrialParam::Int(i) => JsonValue::Int(*i),
        TrialParam::Float(f) => JsonValue::Float(*f),
        TrialParam::Str(s) => JsonValue::String(s.clone()),
        TrialParam::Numeric(NumericPayload { value, time_unit }) => {
            let mut m = BTreeMap::new();
            m.insert("value".into(), JsonValue::Float(*value));
            if let Some(u) = time_unit {
                m.insert("time_unit".into(), JsonValue::String(u.clone()));
            }
            JsonValue::Object(m)
        }
        TrialParam::Schedule(s) => expr_to_json(s),
        TrialParam::Json(j) => j.clone(),
    }
}

fn adjusting_to_json(a: &AdjustingSchedule) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("AdjustingSchedule".into()));
    m.insert("adj_target".into(), JsonValue::String(a.adj_target.clone()));
    m.insert("adj_start".into(), numeric_payload_to_json(&a.adj_start));
    m.insert("adj_step".into(), numeric_payload_to_json(&a.adj_step));
    if let Some(v) = &a.adj_min {
        m.insert("adj_min".into(), numeric_payload_to_json(v));
    }
    if let Some(v) = &a.adj_max {
        m.insert("adj_max".into(), numeric_payload_to_json(v));
    }
    JsonValue::Object(m)
}

fn interlocking_to_json(il: &InterlockingSchedule) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("InterlockingSchedule".into()));
    m.insert("interlock_R0".into(), JsonValue::Int(il.interlock_r0));
    m.insert("interlock_T".into(), numeric_payload_to_json(&il.interlock_t));
    JsonValue::Object(m)
}

fn numeric_payload_to_json(n: &NumericPayload) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("value".into(), JsonValue::Float(n.value));
    if let Some(u) = &n.time_unit {
        m.insert("time_unit".into(), JsonValue::String(u.clone()));
    }
    JsonValue::Object(m)
}

fn annotated_schedule_to_json(a: &AnnotatedSchedule) -> JsonValue {
    let mut inner = expr_to_json(&a.expr);
    let ann_list: Vec<JsonValue> = a.annotations.iter().map(annotation_to_json).collect();
    let is_leaf = matches!(
        &a.expr,
        ScheduleExpr::Atomic(_)
            | ScheduleExpr::Special(_)
            | ScheduleExpr::Modifier(_)
            | ScheduleExpr::SecondOrder(_)
            | ScheduleExpr::LimitedHold(_)
            | ScheduleExpr::Compound(_)
            | ScheduleExpr::AdjustingSchedule(_)
            | ScheduleExpr::InterlockingSchedule(_)
            | ScheduleExpr::AversiveSchedule(_)
            | ScheduleExpr::TrialBased(_)
            | ScheduleExpr::TimeoutWrapped(_)
            | ScheduleExpr::ResponseCostWrapped(_)
    );
    if is_leaf {
        if let JsonValue::Object(map) = &mut inner {
            let combined = match map.remove("annotations") {
                Some(JsonValue::Array(existing)) => {
                    let mut v = ann_list;
                    v.extend(existing);
                    v
                }
                _ => ann_list,
            };
            map.insert("annotations".into(), JsonValue::Array(combined));
        }
        return inner;
    }
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String("AnnotatedSchedule".into()));
    m.insert("expr".into(), inner);
    m.insert("annotations".into(), JsonValue::Array(ann_list));
    JsonValue::Object(m)
}

fn annotation_to_json(a: &Annotation) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("keyword".into(), JsonValue::String(a.keyword.clone()));
    if let Some(pos) = &a.positional {
        m.insert("positional".into(), annotation_value_to_json(pos));
    }
    if let Some(params) = &a.params {
        let mut pm = BTreeMap::new();
        for (k, v) in params {
            pm.insert(k.clone(), annotation_value_to_json(v));
        }
        m.insert("params".into(), JsonValue::Object(pm));
    }
    if let Some(label) = &a.label {
        m.insert("label".into(), JsonValue::String(label.clone()));
    }
    JsonValue::Object(m)
}

fn annotation_value_to_json(v: &AnnotationValue) -> JsonValue {
    match v {
        AnnotationValue::Str(s) => JsonValue::String(s.clone()),
        AnnotationValue::Int(i) => JsonValue::Int(*i),
        AnnotationValue::Float(f) => JsonValue::Float(*f),
        AnnotationValue::Json(j) => j.clone(),
    }
}

fn respondent_to_json(r: &RespondentExpr) -> JsonValue {
    match r {
        RespondentExpr::PairForwardDelay(PairForwardDelay { cs, us, isi, cs_duration }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("PairForwardDelay".into()));
            m.insert("cs".into(), cs_arg_to_json(cs));
            m.insert("us".into(), JsonValue::String(us.clone()));
            m.insert("isi".into(), duration_to_json(isi));
            m.insert("cs_duration".into(), duration_to_json(cs_duration));
            JsonValue::Object(m)
        }
        RespondentExpr::PairForwardTrace(PairForwardTrace {
            cs,
            us,
            cs_duration,
            trace_interval,
        }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("PairForwardTrace".into()));
            m.insert("cs".into(), cs_arg_to_json(cs));
            m.insert("us".into(), JsonValue::String(us.clone()));
            m.insert("trace_interval".into(), duration_to_json(trace_interval));
            m.insert("cs_duration".into(), duration_to_json(cs_duration));
            JsonValue::Object(m)
        }
        RespondentExpr::PairSimultaneous(PairSimultaneous { cs, us, duration: _ }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("PairSimultaneous".into()));
            m.insert("cs".into(), cs_arg_to_json(cs));
            m.insert("us".into(), JsonValue::String(us.clone()));
            JsonValue::Object(m)
        }
        RespondentExpr::PairBackward(PairBackward { cs, us, cs_duration: _, backward_interval }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("PairBackward".into()));
            m.insert("us".into(), JsonValue::String(us.clone()));
            m.insert("cs".into(), cs_arg_to_json(cs));
            m.insert("isi".into(), duration_to_json(backward_interval));
            JsonValue::Object(m)
        }
        RespondentExpr::Extinction(Extinction { cs, .. }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Extinction".into()));
            m.insert("cs".into(), JsonValue::String(cs.clone()));
            JsonValue::Object(m)
        }
        RespondentExpr::CsOnly(CsOnly { cs, trials, .. }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("CSOnly".into()));
            m.insert("cs".into(), JsonValue::String(cs.clone()));
            m.insert(
                "trials".into(),
                trials.map_or(JsonValue::Null, JsonValue::Int),
            );
            JsonValue::Object(m)
        }
        RespondentExpr::UsOnly(UsOnly { us, trials }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("USOnly".into()));
            m.insert("us".into(), JsonValue::String(us.clone()));
            m.insert(
                "trials".into(),
                trials.map_or(JsonValue::Null, JsonValue::Int),
            );
            JsonValue::Object(m)
        }
        RespondentExpr::Contingency(Contingency { p_us_given_cs, p_us_no_cs, .. }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Contingency".into()));
            m.insert("p_us_given_cs".into(), JsonValue::Float(*p_us_given_cs));
            m.insert("p_us_given_no_cs".into(), JsonValue::Float(*p_us_no_cs));
            JsonValue::Object(m)
        }
        RespondentExpr::TrulyRandom(TrulyRandom { cs, us, .. }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("TrulyRandom".into()));
            m.insert("cs".into(), JsonValue::String(cs.clone()));
            m.insert("us".into(), JsonValue::String(us.clone()));
            JsonValue::Object(m)
        }
        RespondentExpr::ExplicitlyUnpaired(ExplicitlyUnpaired { cs, us, min_separation }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("ExplicitlyUnpaired".into()));
            m.insert("cs".into(), JsonValue::String(cs.clone()));
            m.insert("us".into(), JsonValue::String(us.clone()));
            m.insert("min_separation".into(), duration_to_json(min_separation));
            JsonValue::Object(m)
        }
        RespondentExpr::Compound(RespondentCompound { stimuli }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Compound".into()));
            m.insert(
                "cs_list".into(),
                JsonValue::Array(stimuli.iter().cloned().map(JsonValue::String).collect()),
            );
            m.insert("mode".into(), JsonValue::String("overlap".into()));
            JsonValue::Object(m)
        }
        RespondentExpr::Serial(RespondentSerial { stimuli, isis }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Serial".into()));
            m.insert(
                "cs_list".into(),
                JsonValue::Array(stimuli.iter().cloned().map(JsonValue::String).collect()),
            );
            if let Some(first) = isis.first() {
                m.insert("isi".into(), duration_to_json(first));
            }
            JsonValue::Object(m)
        }
        RespondentExpr::Iti(RespondentIti { mode, mean, .. }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("ITI".into()));
            m.insert("distribution".into(), JsonValue::String(mode.clone()));
            if let Some(d) = mean {
                m.insert("mean".into(), duration_to_json(d));
            }
            JsonValue::Object(m)
        }
        RespondentExpr::Differential(RespondentDifferential {
            cs_plus,
            cs_minus,
            us,
            isi: _,
            cs_duration: _,
        }) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Differential".into()));
            m.insert("cs_positive".into(), JsonValue::String(cs_plus.clone()));
            m.insert("cs_negative".into(), JsonValue::String(cs_minus.clone()));
            m.insert("us".into(), JsonValue::String(us.clone()));
            JsonValue::Object(m)
        }
        RespondentExpr::Extension(ext) => respondent_extension_to_json(ext),
        RespondentExpr::Annotated(boxed) => {
            let AnnotatedRespondent { expr, annotations } = boxed.as_ref();
            let mut inner = respondent_to_json(expr);
            if let JsonValue::Object(map) = &mut inner {
                let ann_list: Vec<JsonValue> =
                    annotations.iter().map(annotation_to_json).collect();
                map.insert("annotations".into(), JsonValue::Array(ann_list));
            }
            inner
        }
    }
}

fn respondent_extension_to_json(ext: &RespondentExtensionCall) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("type".into(), JsonValue::String(ext.identifier.clone()));
    for (k, v) in &ext.keyword {
        m.insert(k.clone(), v.clone());
    }
    if !ext.positional.is_empty() {
        m.insert("positional".into(), JsonValue::Array(ext.positional.clone()));
    }
    JsonValue::Object(m)
}

fn cs_arg_to_json(cs: &CsArg) -> JsonValue {
    match &cs.kind {
        CsArgKind::Ref(s) => JsonValue::String(s.clone()),
        CsArgKind::Compound(xs) => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Compound".into()));
            m.insert(
                "cs_list".into(),
                JsonValue::Array(xs.iter().cloned().map(JsonValue::String).collect()),
            );
            JsonValue::Object(m)
        }
        CsArgKind::Serial { stimuli, isis } => {
            let mut m = BTreeMap::new();
            m.insert("type".into(), JsonValue::String("Serial".into()));
            m.insert(
                "cs_list".into(),
                JsonValue::Array(stimuli.iter().cloned().map(JsonValue::String).collect()),
            );
            if let Some(first) = isis.first() {
                m.insert("isi".into(), duration_to_json(first));
            }
            JsonValue::Object(m)
        }
    }
}

fn duration_to_json(d: &Duration) -> JsonValue {
    let mut m = BTreeMap::new();
    m.insert("value".into(), JsonValue::Float(d.value));
    m.insert("unit".into(), JsonValue::String(d.unit.clone()));
    JsonValue::Object(m)
}

// --- JSON emission (AST JsonValue → string) ---

pub fn json_to_string(v: &JsonValue) -> String {
    let mut out = String::new();
    emit(v, &mut out);
    out
}

fn emit(v: &JsonValue, out: &mut String) {
    match v {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Int(i) => out.push_str(&i.to_string()),
        JsonValue::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                out.push_str("null");
            } else if f.fract() == 0.0 && f.abs() < 1e16 {
                out.push_str(&format!("{:.1}", f));
            } else {
                out.push_str(&format!("{}", f));
            }
        }
        JsonValue::String(s) => emit_string(s, out),
        JsonValue::Array(xs) => {
            out.push('[');
            for (i, x) in xs.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                emit(x, out);
            }
            out.push(']');
        }
        JsonValue::Object(m) => {
            out.push('{');
            for (i, (k, v)) in m.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                emit_string(k, out);
                out.push(':');
                emit(v, out);
            }
            out.push('}');
        }
    }
}

fn emit_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

impl Combinator {
    pub fn as_str(self) -> &'static str {
        match self {
            Combinator::Conc => "Conc",
            Combinator::Alt => "Alt",
            Combinator::Conj => "Conj",
            Combinator::Chain => "Chain",
            Combinator::Tand => "Tand",
            Combinator::Mult => "Mult",
            Combinator::Mix => "Mix",
            Combinator::Overlay => "Overlay",
            Combinator::Interpolate => "Interpolate",
            Combinator::Interp => "Interp",
        }
    }
}

impl ScheduleType {
    /// 2-char canonical name: `dist.code()` + `domain.code()`.
    pub fn canonical(&self) -> String {
        let mut s = String::with_capacity(2);
        s.push(self.distribution.code());
        s.push(self.domain.code());
        s
    }
}
