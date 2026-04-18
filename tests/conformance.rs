//! Conformance harness: run every fixture under
//! `../contingency-dsl/conformance/**/*.json` and report which cases this
//! implementation currently matches.
//!
//! The harness never fails a case with `expected_error` or `expected_warning`
//! (those channels are out of scope until the error-code layer lands). For
//! cases whose `expected` payload this implementation does not yet produce,
//! the failure is counted and attributed to the originating file so future
//! sessions can target the largest gaps first.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use contingency_dsl::ast::JsonValue as AstValue;
use contingency_dsl::{parse, program_to_json};
use serde_json::Value as JsonV;

fn conformance_root() -> PathBuf {
    // crate dir is `.../apps/core/contingency-dsl-rs`; the DSL spec lives in
    // `.../apps/core/contingency-dsl/conformance`.
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    here.parent()
        .map(|p| p.join("contingency-dsl").join("conformance"))
        .expect("conformance dir lookup")
}

fn collect_fixtures(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
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
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem == "errors" || stem == "warnings" {
                continue;
            }
            out.push(path);
        }
    }
}

fn ast_value_to_serde(v: &AstValue) -> JsonV {
    match v {
        AstValue::Null => JsonV::Null,
        AstValue::Bool(b) => JsonV::Bool(*b),
        AstValue::Int(i) => JsonV::Number((*i).into()),
        AstValue::Float(f) => serde_json::Number::from_f64(*f)
            .map(JsonV::Number)
            .unwrap_or(JsonV::Null),
        AstValue::String(s) => JsonV::String(s.clone()),
        AstValue::Array(xs) => JsonV::Array(xs.iter().map(ast_value_to_serde).collect()),
        AstValue::Object(m) => {
            let mut out = serde_json::Map::new();
            for (k, v) in m {
                out.insert(k.clone(), ast_value_to_serde(v));
            }
            JsonV::Object(out)
        }
    }
}

/// Subset-structural match: every key in `expected` must exist in `actual`
/// with an equal value, but `actual` may carry additional keys. This mirrors
/// `_compare` in the Python conformance runner.
fn json_subset(actual: &JsonV, expected: &JsonV) -> bool {
    match (actual, expected) {
        (JsonV::Number(x), JsonV::Number(y)) => {
            let xf = x.as_f64();
            let yf = y.as_f64();
            match (xf, yf) {
                (Some(xv), Some(yv)) => (xv - yv).abs() < 1e-9,
                _ => x == y,
            }
        }
        (JsonV::Array(xs), JsonV::Array(ys)) => {
            xs.len() == ys.len() && xs.iter().zip(ys.iter()).all(|(a, e)| json_subset(a, e))
        }
        (JsonV::Object(actual_map), JsonV::Object(expected_map)) => expected_map
            .iter()
            .all(|(k, v)| actual_map.get(k).map_or(false, |w| json_subset(w, v))),
        _ => actual == expected,
    }
}

#[derive(Default)]
struct Stats {
    total: usize,
    parse_ok: usize,
    shape_match: usize,
    parse_err: usize,
    shape_mismatch: usize,
    skipped_error: usize,
    skipped_warning: usize,
    per_file: BTreeMap<String, (usize, usize)>, // (passed, total-covered)
}

#[test]
fn conformance_coverage_report() {
    let root = conformance_root();
    if !root.exists() {
        eprintln!("conformance root missing: {}", root.display());
        return;
    }
    let fixtures = collect_fixtures(&root);
    let mut stats = Stats::default();

    for path in &fixtures {
        let data = fs::read_to_string(path).expect("read fixture");
        let json: JsonV = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("skip non-JSON: {}: {}", path.display(), e);
                continue;
            }
        };
        let cases = match &json {
            JsonV::Array(xs) => xs.clone(),
            _ => continue,
        };
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();
        let mut covered = 0usize;
        let mut passed = 0usize;

        for case in cases {
            let JsonV::Object(obj) = case else { continue };
            let Some(JsonV::String(input)) = obj.get("input") else { continue };
            // rs parser has no analyzer yet, so prefer `pre_expansion` (raw
            // parse output) over `expected` (post-analysis) when both exist.
            let want_opt = obj
                .get("pre_expansion")
                .or_else(|| obj.get("expected"))
                .cloned();
            if obj.contains_key("expected_error") {
                stats.skipped_error += 1;
                continue;
            }
            let Some(want) = want_opt else {
                if obj.contains_key("expected_warning") {
                    stats.skipped_warning += 1;
                }
                continue;
            };
            covered += 1;
            stats.total += 1;

            let program = match parse(input) {
                Ok(p) => p,
                Err(_) => {
                    stats.parse_err += 1;
                    continue;
                }
            };
            stats.parse_ok += 1;
            let got = ast_value_to_serde(&program_to_json(&program));
            if json_subset(&got, &want) {
                stats.shape_match += 1;
                passed += 1;
            } else {
                stats.shape_mismatch += 1;
            }
        }

        if covered > 0 {
            stats.per_file.insert(rel, (passed, covered));
        }
    }

    eprintln!();
    eprintln!("== contingency-dsl-rs conformance report ==");
    eprintln!("total covered cases: {}", stats.total);
    eprintln!("  parse ok        : {}", stats.parse_ok);
    eprintln!("  shape match     : {}", stats.shape_match);
    eprintln!("  parse errors    : {}", stats.parse_err);
    eprintln!("  shape mismatches: {}", stats.shape_mismatch);
    eprintln!("  skipped (errors)  : {}", stats.skipped_error);
    eprintln!("  skipped (warnings): {}", stats.skipped_warning);
    eprintln!();
    eprintln!("per-file pass rate:");
    for (file, (pass, total)) in &stats.per_file {
        eprintln!("  {:>4}/{:>4}  {}", pass, total, file);
    }
}
