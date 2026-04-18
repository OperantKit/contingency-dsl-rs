//! Conformance harness: run every fixture under
//! `../contingency-dsl/conformance/**/*.json` and report which cases this
//! implementation currently matches.
//!
//! The harness checks four conformance channels:
//!
//! 1. shape match — fixtures with `expected` or `pre_expansion` must parse
//!    successfully and produce the matching JSON shape under subset
//!    comparison;
//! 2. error channel — fixtures with an `error` object (or `expected_error`)
//!    must produce a DslError whose `kind` and `code` match;
//! 3. warning channel — fixtures with a `warnings` list must accumulate a
//!    diagnostic containing each listed code during analyzer expansion;
//! 4. algebraic fixtures (`lhs` / `rhs` / `relation`) are skipped.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use contingency_dsl::ast::JsonValue as AstValue;
use contingency_dsl::{
    expand_bindings, paper_to_json, parse_top_level, phase_sequence_to_json, program_to_json,
    TopLevel,
};
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
            // Skip representations/ — it has its own test module in py
            // (t-tau conversion) and is not in scope for this harness.
            if path.file_name().and_then(|s| s.to_str()) == Some("representations") {
                continue;
            }
            walk(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
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
    // Shape channel
    shape_total: usize,
    shape_match: usize,
    // Error channel
    error_total: usize,
    error_match: usize,
    // Warning channel
    warning_total: usize,
    warning_match: usize,
    // Per-file breakdown: (shape_pass, shape_total, error_pass, error_total,
    // warning_pass, warning_total)
    per_file: BTreeMap<String, (usize, usize, usize, usize, usize, usize)>,
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
        let mut row = (0usize, 0usize, 0usize, 0usize, 0usize, 0usize);

        for case in cases {
            let JsonV::Object(obj) = case else { continue };
            let Some(JsonV::String(input)) = obj.get("input") else { continue };
            // Algebraic equivalence fixtures are skipped (py runs them
            // through a separate comparator).
            if obj.contains_key("lhs") && obj.contains_key("rhs") {
                continue;
            }

            // Error channel: fixture has an `error` object or `expected_error`.
            let error_info = normalize_error(&obj);
            let expected_warnings = normalize_warnings(&obj);
            let want_shape = obj
                .get("pre_expansion")
                .or_else(|| obj.get("expected"))
                .cloned();

            if let Some((etype, ecode)) = error_info {
                stats.error_total += 1;
                row.3 += 1;
                let parse_result = parse_top_level(input);
                let matched = match &parse_result {
                    Err(e) => {
                        let kind_ok = etype.is_empty() || e.kind_str() == etype;
                        let code_ok = ecode
                            .as_deref()
                            .map(|want| e.code().map_or(false, |c| c == want))
                            .unwrap_or(true);
                        kind_ok && code_ok
                    }
                    Ok(top) => {
                        // Parse succeeded; some codes surface only at the
                        // analyzer pass — check collected diagnostics.
                        let diags = collect_diagnostics(top);
                        ecode
                            .as_deref()
                            .map(|want| diags.iter().any(|d| d == want))
                            .unwrap_or(false)
                    }
                };
                if matched {
                    stats.error_match += 1;
                    row.2 += 1;
                }
                continue;
            }

            if !expected_warnings.is_empty() {
                stats.warning_total += 1;
                row.5 += 1;
                let diags = match parse_top_level(input) {
                    Ok(top) => collect_diagnostics(&top),
                    Err(_) => Vec::new(),
                };
                let matched = expected_warnings
                    .iter()
                    .all(|code| diags.iter().any(|d| d == code));
                if matched {
                    stats.warning_match += 1;
                    row.4 += 1;
                }
                // Fall through — some warning fixtures also have an
                // `expected` shape that should be verified.
            }

            let Some(want) = want_shape else {
                continue;
            };
            stats.shape_total += 1;
            row.1 += 1;

            let top = match parse_top_level(input) {
                Ok(t) => t,
                Err(_) => continue,
            };
            let got_ast = match &top {
                TopLevel::Program(p) => program_to_json(p),
                TopLevel::PhaseSequence(s) => phase_sequence_to_json(s),
                TopLevel::Paper(p) => paper_to_json(p),
            };
            let got = ast_value_to_serde(&got_ast);
            let mut matched = json_subset(&got, &want);
            if !matched {
                if let (TopLevel::Program(p), Some(expected_val)) =
                    (&top, obj.get("expected"))
                {
                    let expanded = expand_bindings(p.clone());
                    let got2 = ast_value_to_serde(&program_to_json(&expanded));
                    if json_subset(&got2, expected_val) {
                        matched = true;
                    }
                }
            }
            if matched {
                stats.shape_match += 1;
                row.0 += 1;
            }
        }

        let covered = row.1 + row.3 + row.5;
        if covered > 0 {
            stats.per_file.insert(rel, row);
        }
    }

    eprintln!();
    eprintln!("== contingency-dsl-rs conformance report ==");
    eprintln!("shape channel   : {} / {}", stats.shape_match, stats.shape_total);
    eprintln!("error channel   : {} / {}", stats.error_match, stats.error_total);
    eprintln!("warning channel : {} / {}", stats.warning_match, stats.warning_total);
    eprintln!(
        "grand total     : {} / {}",
        stats.shape_match + stats.error_match + stats.warning_match,
        stats.shape_total + stats.error_total + stats.warning_total
    );
    eprintln!();
    eprintln!("per-file pass rate (shape | error | warning):");
    for (file, (sp, st, ep, et, wp, wt)) in &stats.per_file {
        let seg = |p: usize, t: usize| {
            if t == 0 {
                "     -".to_string()
            } else {
                format!("{:>3}/{:>3}", p, t)
            }
        };
        eprintln!(
            "  {} | {} | {}   {}",
            seg(*sp, *st),
            seg(*ep, *et),
            seg(*wp, *wt),
            file
        );
    }
}

fn normalize_error(obj: &serde_json::Map<String, JsonV>) -> Option<(String, Option<String>)> {
    // Shorthand:
    //   "error": "ParseError"             → kind shorthand
    //   "error": "MISSING_SIDMAN_PARAM"   → code shorthand
    //   "error": { "type": "...", "code": "..." }
    if let Some(v) = obj.get("error") {
        match v {
            JsonV::String(s) => {
                let is_kind = matches!(s.as_str(), "ParseError" | "LexError" | "SemanticError");
                if is_kind {
                    return Some((s.clone(), None));
                } else {
                    return Some((String::new(), Some(s.clone())));
                }
            }
            JsonV::Object(m) => {
                let etype = m
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let ecode = m.get("code").and_then(|x| x.as_str()).map(str::to_string);
                // If `type` doesn't look like a kind name, it is itself a
                // code shorthand (fixtures like `{"type": "PCTL_INVALID_RANK"}`).
                let kind_ok = matches!(etype.as_str(), "" | "ParseError" | "LexError" | "SemanticError");
                if !kind_ok {
                    return Some((String::new(), Some(etype)));
                }
                return Some((etype, ecode));
            }
            _ => {}
        }
    }
    if let Some(JsonV::String(code)) = obj.get("expected_error") {
        return Some((String::new(), Some(code.clone())));
    }
    None
}

fn normalize_warnings(obj: &serde_json::Map<String, JsonV>) -> Vec<String> {
    let pick = obj
        .get("warnings")
        .or_else(|| obj.get("expected_warnings"))
        .or_else(|| obj.get("expected_warning"))
        .or_else(|| obj.get("warning"));
    match pick {
        Some(JsonV::Array(xs)) => xs
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        Some(JsonV::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    }
}

fn collect_diagnostics(top: &TopLevel) -> Vec<String> {
    match top {
        TopLevel::Program(p) => contingency_dsl::collect_warnings(p),
        TopLevel::PhaseSequence(s) => s.warnings.clone(),
        TopLevel::Paper(_) => Vec::new(),
    }
}
