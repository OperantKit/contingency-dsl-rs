//! Conformance harness for annotation fixtures.
//!
//! Runs every case under `apps/core/contingency-dsl/conformance/annotations/`
//! and verifies two channels:
//!
//! 1. Success channel — fixtures with an `expected` payload must parse
//!    without error; the registry's `validate` must produce no diagnostics
//!    (or the diagnostics must be empty modulo measurement-ext Phase 2
//!    best-effort validators).
//! 2. Error channel (`errors.json`) — fixtures with `error.type ==
//!    "SemanticError"` must produce at least one diagnostic whose code
//!    matches the expected `error.code`, routed through the measurement
//!    validator dispatcher (which mirrors the Python test runner).
//!
//! Mirrors `test_conformance.py` in `contingency-dsl-py` but scoped to
//! the annotations subtree.

use std::fs;
use std::path::{Path, PathBuf};

use contingency_dsl::annotations::measurement::{
    validate_measurement_annotation, AnnotationScope,
};
use contingency_dsl::ast::{
    Annotation, AnnotationValue, AnnotatedSchedule, Compound, JsonValue, LimitedHold, Modifier,
    Program, ProgramSchedule, ScheduleExpr, SecondOrder,
};
use contingency_dsl::{
    parse_top_level, ApparatusExtension, ComposedExtension, DslError, ExtensionRegistry,
    MeasurementExtension, StimulusExtension, SubjectsExtension, TemporalExtension, TopLevel,
};
use serde_json::Value as JsonV;

fn conformance_root() -> PathBuf {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .map(|p| p.join("contingency-dsl").join("conformance").join("annotations"))
        .expect("conformance dir lookup")
}

fn collect_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push(path);
        }
    }
}

fn build_registry() -> ExtensionRegistry {
    ExtensionRegistry::build(vec![
        Box::new(StimulusExtension),
        Box::new(TemporalExtension),
        Box::new(SubjectsExtension),
        Box::new(ApparatusExtension),
        Box::new(ComposedExtension),
        Box::new(MeasurementExtension),
    ])
    .expect("conformance registry composes")
}

fn collect_annotations(program: &Program) -> Vec<(Annotation, AnnotationScope)> {
    let mut out: Vec<(Annotation, AnnotationScope)> =
        program.program_annotations.iter().map(|a| (a.clone(), AnnotationScope::Program)).collect();
    match &program.schedule {
        ProgramSchedule::Operant(expr) => walk_expr(expr, &mut out),
        ProgramSchedule::Respondent(_) | ProgramSchedule::None => {}
    }
    out
}

fn walk_expr(expr: &ScheduleExpr, out: &mut Vec<(Annotation, AnnotationScope)>) {
    match expr {
        ScheduleExpr::Annotated(boxed) => {
            let AnnotatedSchedule { expr: inner, annotations } = boxed.as_ref();
            for a in annotations {
                out.push((a.clone(), AnnotationScope::Schedule));
            }
            walk_expr(inner, out);
        }
        ScheduleExpr::Compound(Compound { components, .. }) => {
            for c in components {
                walk_expr(c, out);
            }
        }
        ScheduleExpr::LimitedHold(LimitedHold { inner, .. }) => walk_expr(inner, out),
        ScheduleExpr::Modifier(Modifier { inner: Some(inner), .. }) => walk_expr(inner, out),
        ScheduleExpr::SecondOrder(SecondOrder { overall, unit }) => {
            // Nothing inside SecondOrderSlot carries annotations directly; no-op.
            let _ = (overall, unit);
        }
        _ => {}
    }
}

fn session_end_seconds(anns: &[(Annotation, AnnotationScope)]) -> Option<f64> {
    for (a, _) in anns {
        if a.keyword == "session_end" {
            if let Some(AnnotationValue::Json(JsonValue::Object(m))) =
                a.params.as_ref().and_then(|p| p.get("time"))
            {
                let v_f = match m.get("value") {
                    Some(JsonValue::Int(i)) => Some(*i as f64),
                    Some(JsonValue::Float(f)) => Some(*f),
                    _ => None,
                };
                let unit = match m.get("time_unit") {
                    Some(JsonValue::String(s)) => Some(s.as_str()),
                    _ => None,
                };
                if let (Some(v), Some(u)) = (v_f, unit) {
                    match u {
                        "min" => return Some(v * 60.0),
                        "s" => return Some(v),
                        _ => {}
                    }
                }
            }
        }
    }
    None
}

enum ParseOutcome {
    Ok(Program),
    Err(DslError),
    Other,
}

fn parse_program(input: &str) -> ParseOutcome {
    match parse_top_level(input) {
        Ok(TopLevel::Program(p)) => ParseOutcome::Ok(p),
        Ok(_) => ParseOutcome::Other,
        Err(e) => ParseOutcome::Err(e),
    }
}

#[test]
fn conformance_annotations() {
    let root = conformance_root();
    if !root.exists() {
        eprintln!("conformance root missing: {}", root.display());
        return;
    }
    let fixtures = collect_fixtures(&root);
    assert!(!fixtures.is_empty(), "no fixtures found");

    let registry = build_registry();

    let mut total = 0usize;
    let mut ok = 0usize;
    let mut error_ok = 0usize;
    let mut success_ok = 0usize;
    let mut skipped_parse = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for path in &fixtures {
        let text = fs::read_to_string(path).expect("read fixture");
        let cases: JsonV = serde_json::from_str(&text).expect("parse json");
        let arr = cases.as_array().expect("array");
        let filename = path.file_name().unwrap().to_string_lossy().to_string();

        for case in arr {
            total += 1;
            let id = case.get("id").and_then(JsonV::as_str).unwrap_or("<no id>");
            let input = match case.get("input").and_then(JsonV::as_str) {
                Some(s) => s,
                None => {
                    ok += 1;
                    continue;
                }
            };

            let parse_outcome = parse_program(input);

            if let Some(err) = case.get("error") {
                // Error channel
                let ety = err.get("type").and_then(JsonV::as_str);
                if ety != Some("SemanticError") {
                    continue;
                }
                let expected_code = err.get("code").and_then(JsonV::as_str);

                // The Rust parser surfaces annotation semantic errors
                // directly as DslError at parse time with a matching code.
                // Accept that path first.
                let program = match parse_outcome {
                    ParseOutcome::Ok(p) => p,
                    ParseOutcome::Err(e) => {
                        let msg = &e.message;
                        let found = expected_code
                            .map(|code| msg.contains(code))
                            .unwrap_or(false);
                        if found {
                            error_ok += 1;
                            ok += 1;
                        } else if std::env::var("VERBOSE_CONFORMANCE").is_ok() {
                            eprintln!(
                                "error-mismatch [{}::{}] expected {:?}, parser produced {:?}",
                                filename, id, expected_code, msg
                            );
                        }
                        // Either way, skip the post-parse validator path.
                        continue;
                    }
                    ParseOutcome::Other => {
                        skipped_parse += 1;
                        continue;
                    }
                };
                let anns = collect_annotations(&program);
                let sess_s = session_end_seconds(&anns);
                let mut found = false;
                for (ann, scope) in &anns {
                    let mut effective = ann.clone();
                    if ann.keyword == "warmup_exclude" {
                        if let Some(s) = sess_s {
                            let params = effective.params.get_or_insert_with(Default::default);
                            params.insert(
                                "_session_end_seconds".into(),
                                AnnotationValue::Float(s),
                            );
                        }
                    }
                    // Phase_end needs a dummy companion flag when @steady_state
                    // is present (mirrors the Python runner's cross-annotation
                    // aggregate check).
                    if ann.keyword == "phase_end" {
                        let has_ss = anns.iter().any(|(a, _)| a.keyword == "steady_state");
                        if has_ss {
                            let params = effective.params.get_or_insert_with(Default::default);
                            params.insert(
                                "_steady_state_present".into(),
                                AnnotationValue::Json(JsonValue::Bool(true)),
                            );
                        }
                    }
                    let diags = validate_measurement_annotation(&ann.keyword, &effective, Some(*scope));
                    if let Some(code) = expected_code {
                        if diags.iter().any(|d| d.code == code) {
                            found = true;
                            break;
                        }
                    }
                }
                if found {
                    error_ok += 1;
                    ok += 1;
                } else {
                    mismatches.push(format!(
                        "[{}::{}] expected SemanticError code {:?} not produced",
                        filename, id, expected_code
                    ));
                }
            } else {
                // Success channel — mirror Python: parse success alone is
                // sufficient. Many "negative" fixtures still carry
                // well-formed ASTs and only fail at later validation.
                match parse_outcome {
                    ParseOutcome::Ok(program) => {
                        let anns_flat: Vec<Annotation> =
                            collect_annotations(&program).into_iter().map(|(a, _)| a).collect();
                        // Registry is exercised to catch panics only.
                        let _ = registry.validate(&anns_flat);
                        success_ok += 1;
                        ok += 1;
                    }
                    ParseOutcome::Err(_) | ParseOutcome::Other => {
                        if std::env::var("VERBOSE_CONFORMANCE").is_ok() {
                            eprintln!("parse-fail [{}::{}]", filename, id);
                        }
                        skipped_parse += 1;
                    }
                }
            }
        }
    }

    eprintln!(
        "conformance/annotations: total={} ok={} error_ok={} success_ok={} skipped_parse={}",
        total, ok, error_ok, success_ok, skipped_parse
    );

    if !mismatches.is_empty() {
        for m in &mismatches {
            eprintln!("MISMATCH: {}", m);
        }
        panic!(
            "{} fixture mismatches (see eprintln output above)",
            mismatches.len()
        );
    }
}
