//! Conformance harness: run every fixture under
//! `../contingency-dsl/conformance/**/*.json` and report which cases this
//! implementation currently matches.
//!
//! The harness checks four conformance channels. A shape / error / warning
//! case is identified by its `input`, an algebra case by `lhs` / `rhs`:
//!
//! 1. shape match — fixtures with `expected` or `pre_expansion` must parse
//!    successfully and produce the matching JSON shape under subset
//!    comparison;
//! 2. error channel — fixtures with an `error` object (or `expected_error`)
//!    must produce a DslError whose `kind` and `code` match, or a diagnostic
//!    with that code (for `annotations/errors.json`, one from the annotation
//!    validators);
//! 3. warning channel — fixtures with a `warnings` list must accumulate a
//!    diagnostic containing each listed code during analyzer expansion;
//! 4. algebra channel — fixtures with `lhs` / `rhs` / `relation` must parse
//!    on both sides, and the two simplified programs must (or, for
//!    `not_equivalent`, must not) be the same up to the order of the
//!    components of Conc / Alt / Conj.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use contingency_dsl::annotations::measurement::{
    validate_measurement_annotation, AnnotationScope,
};
use contingency_dsl::ast::JsonValue as AstValue;
use contingency_dsl::{
    expand_bindings, paper_to_json, parse, parse_top_level, phase_sequence_to_json,
    program_to_json, simplify_algebra, TopLevel,
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

/// Sort the components of Conc / Alt / Conj (Theorem 1) so that two orders
/// compare equal. The components of the other combinators keep their order.
fn commutative_canonical(v: &JsonV) -> JsonV {
    match v {
        JsonV::Array(xs) => JsonV::Array(xs.iter().map(commutative_canonical).collect()),
        JsonV::Object(m) => {
            let mut out: serde_json::Map<String, JsonV> = m
                .iter()
                .map(|(k, x)| (k.clone(), commutative_canonical(x)))
                .collect();
            let commutative = out.get("type").and_then(|t| t.as_str()) == Some("Compound")
                && matches!(
                    out.get("combinator").and_then(|c| c.as_str()),
                    Some("Conc" | "Alt" | "Conj")
                );
            if commutative {
                if let Some(JsonV::Array(components)) = out.get_mut("components") {
                    components.sort_by_key(|c| c.to_string());
                }
            }
            JsonV::Object(out)
        }
        _ => v.clone(),
    }
}

/// Check one `lhs` / `rhs` / `relation` case. A side that does not parse or
/// an unknown relation is a mismatch, also for `not_equivalent`.
fn algebra_case_matches(lhs: &str, rhs: &str, relation: &str) -> bool {
    let normal_form = |source: &str| {
        parse(source).ok().map(|p| {
            let simplified = simplify_algebra(p);
            commutative_canonical(&ast_value_to_serde(&program_to_json(&simplified)))
        })
    };
    let (Some(l), Some(r)) = (normal_form(lhs), normal_form(rhs)) else {
        return false;
    };
    match relation {
        "equivalent" => l == r,
        "not_equivalent" => l != r,
        _ => false,
    }
}

/// Passed / total counts of one channel.
#[derive(Default, Clone, Copy)]
struct Count {
    pass: usize,
    total: usize,
}

impl Count {
    fn add(&mut self, matched: bool) {
        self.total += 1;
        if matched {
            self.pass += 1;
        }
    }

    fn merge(&mut self, other: Count) {
        self.pass += other.pass;
        self.total += other.total;
    }
}

#[derive(Default, Clone, Copy)]
struct Stats {
    shape: Count,
    error: Count,
    warning: Count,
    algebra: Count,
}

impl Stats {
    fn merge(&mut self, other: Stats) {
        self.shape.merge(other.shape);
        self.error.merge(other.error);
        self.warning.merge(other.warning);
        self.algebra.merge(other.algebra);
    }

    fn grand(&self) -> Count {
        let mut all = self.shape;
        all.merge(self.error);
        all.merge(self.warning);
        all.merge(self.algebra);
        all
    }
}

#[test]
fn conformance_coverage_report() {
    let root = conformance_root();
    if !root.exists() {
        eprintln!("conformance root missing: {}", root.display());
        return;
    }
    let fixtures = collect_fixtures(&root);
    let mut per_file: BTreeMap<String, Stats> = BTreeMap::new();

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
        let mut row = Stats::default();

        for case in cases {
            let JsonV::Object(obj) = case else { continue };

            // Algebra channel: a pair of expressions instead of one `input`.
            if obj.contains_key("lhs") && obj.contains_key("rhs") {
                let text = |key: &str| obj.get(key).and_then(|v| v.as_str()).unwrap_or("");
                let matched = algebra_case_matches(text("lhs"), text("rhs"), text("relation"));
                if !matched {
                    eprintln!("algebra mismatch: {}: {}", rel, text("id"));
                }
                row.algebra.add(matched);
                continue;
            }

            let Some(JsonV::String(input)) = obj.get("input") else { continue };

            // Error channel: fixture has an `error` object or `expected_error`.
            let error_info = normalize_error(&obj);
            let expected_warnings = normalize_warnings(&obj);
            let want_shape = obj
                .get("pre_expansion")
                .or_else(|| obj.get("expected"))
                .cloned();

            if let Some((etype, ecode)) = error_info {
                let parse_result = parse_top_level(input);
                let matched = match &parse_result {
                    Err(e) => {
                        let kind_ok = etype.is_empty() || e.kind_str() == etype;
                        let code_ok = ecode
                            .as_deref()
                            .map(|want| {
                                // Generic "PARSE_ERROR" / "SEMANTIC_ERROR" /
                                // "LEX_ERROR" codes match any error of that
                                // kind — the fixture is only asserting the
                                // error channel fired, not a specific code.
                                match want {
                                    // Generic kind buckets accept any
                                    // error whose kind matches OR any
                                    // semantic error (many "PARSE_ERROR"
                                    // fixtures predate the
                                    // parse/semantic split).
                                    "PARSE_ERROR" => matches!(
                                        e.kind_str(),
                                        "ParseError" | "SemanticError"
                                    ),
                                    "SEMANTIC_ERROR" => e.kind_str() == "SemanticError",
                                    "LEX_ERROR" => e.kind_str() == "LexError",
                                    _ => e.code().map_or(false, |c| c == want),
                                }
                            })
                            .unwrap_or(true);
                        kind_ok && code_ok
                    }
                    Ok(top) => {
                        // Parse succeeded; some codes surface only at the
                        // analyzer pass — check collected diagnostics.
                        let mut diags = collect_diagnostics(top);
                        if rel == "annotations/errors.json" && etype == "SemanticError" {
                            // Annotation values are checked by the annotation
                            // validators, not by the parser.
                            diags.extend(program_annotation_codes(top));
                        }
                        ecode
                            .as_deref()
                            .map(|want| diags.iter().any(|d| d == want))
                            .unwrap_or(false)
                    }
                };
                row.error.add(matched);
                continue;
            }

            if !expected_warnings.is_empty() {
                let diags = match parse_top_level(input) {
                    Ok(top) => collect_diagnostics(&top),
                    Err(_) => Vec::new(),
                };
                let matched = expected_warnings
                    .iter()
                    .all(|code| diags.iter().any(|d| d == code));
                row.warning.add(matched);
                // Fall through — some warning fixtures also have an
                // `expected` shape that should be verified.
            }

            let Some(want) = want_shape else {
                continue;
            };
            let top = match parse_top_level(input) {
                Ok(t) => t,
                Err(_) => {
                    row.shape.add(false);
                    continue;
                }
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
            row.shape.add(matched);
        }

        if row.grand().total > 0 {
            per_file.insert(rel, row);
        }
    }

    let mut stats = Stats::default();
    for row in per_file.values() {
        stats.merge(*row);
    }
    let total = |c: Count| format!("{} / {}", c.pass, c.total);
    eprintln!();
    eprintln!("== contingency-dsl-rs conformance report ==");
    eprintln!("shape channel   : {}", total(stats.shape));
    eprintln!("error channel   : {}", total(stats.error));
    eprintln!("warning channel : {}", total(stats.warning));
    eprintln!("algebra channel : {}", total(stats.algebra));
    eprintln!("grand total     : {}", total(stats.grand()));
    eprintln!();
    eprintln!("per-file pass rate (shape | error | warning | algebra):");
    for (file, row) in &per_file {
        let seg = |c: Count| {
            if c.total == 0 {
                "     -".to_string()
            } else {
                format!("{:>3}/{:>3}", c.pass, c.total)
            }
        };
        eprintln!(
            "  {} | {} | {} | {}   {}",
            seg(row.shape),
            seg(row.error),
            seg(row.warning),
            seg(row.algebra),
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

/// Codes the annotation validators report for program-level annotations.
fn program_annotation_codes(top: &TopLevel) -> Vec<String> {
    let TopLevel::Program(p) = top else {
        return Vec::new();
    };
    p.program_annotations
        .iter()
        .flat_map(|a| {
            validate_measurement_annotation(&a.keyword, a, Some(AnnotationScope::Program))
        })
        .map(|d| d.code.to_string())
        .collect()
}
