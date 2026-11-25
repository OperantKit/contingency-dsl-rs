//! Boundaries of `simplify_algebra` that the equivalence pairs in
//! `conformance/operant/algebra.json` do not reach: what must be left alone,
//! and rules that combine.

use contingency_dsl::{json_to_string, parse, program_to_json, simplify_algebra};

fn simplified(source: &str) -> String {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    json_to_string(&program_to_json(&simplify_algebra(program)))
}

fn parsed(source: &str) -> String {
    let program = parse(source).unwrap_or_else(|e| panic!("{source}: {e:?}"));
    json_to_string(&program_to_json(&program))
}

#[track_caller]
fn assert_simplifies(source: &str, expected: &str) {
    assert_eq!(
        simplified(source),
        parsed(expected),
        "{source} -> {expected}"
    );
    // The result is already in its simplest form.
    assert_eq!(
        simplified(expected),
        parsed(expected),
        "{expected} is not stable"
    );
}

#[track_caller]
fn assert_unchanged(source: &str) {
    assert_simplifies(source, source);
}

#[test]
fn only_a_leading_crf_leaves_a_chain() {
    assert_unchanged("Chain(FR5, CRF)");
    assert_unchanged("Chain(FR5, CRF, FI30s)");
    assert_simplifies("Chain(CRF, CRF, FI30s)", "FI30s");
    assert_simplifies("Chain(CRF, FR5, CRF)", "Chain(FR5, CRF)");
    assert_simplifies("Chain(CRF, CRF)", "CRF");
}

#[test]
fn combinators_without_identity_rules_keep_ext_and_crf() {
    for combinator in ["Conc", "Tand", "Mult", "Mix"] {
        assert_unchanged(&format!("{combinator}(CRF, FR5)"));
        assert_unchanged(&format!("{combinator}(FR5, EXT)"));
    }
    assert_unchanged("Chain(EXT, FR5)");
}

#[test]
fn compounds_with_keyword_arguments_keep_their_shape() {
    // Flattening would move the meaning of COD(1->2) and of BO.
    assert_unchanged("Conc(FR5, Conc(VI30s, VI60s), COD=2s)");
    assert_unchanged("Conc(FR5, Conc(VI30s, VI60s, COD=2s))");
    assert_unchanged("Mult(FR5, Mult(FR6, FR7, BO=5s))");
    // Components inside are still simplified.
    assert_simplifies(
        "Conc(Alt(FR5, EXT), VI60s, COD=2s)",
        "Conc(FR5, VI60s, COD=2s)",
    );
}

#[test]
fn every_associative_combinator_flattens_on_both_sides() {
    for combinator in ["Conc", "Alt", "Conj", "Chain", "Tand", "Mult", "Mix"] {
        let flat = format!("{combinator}(FR5, FI30s, VI60s)");
        assert_simplifies(
            &format!("{combinator}(FR5, {combinator}(FI30s, VI60s))"),
            &flat,
        );
        assert_simplifies(
            &format!("{combinator}({combinator}(FR5, FI30s), VI60s)"),
            &flat,
        );
    }
    assert_unchanged("Chain(FR5, Tand(FI30s, VI60s))");
}

#[test]
fn overlay_is_not_flattened() {
    assert_unchanged("Overlay(FR5, Overlay(FR1, FR2))");
}

#[test]
fn identity_elements_alone_stay_themselves() {
    assert_simplifies("Alt(EXT, EXT)", "EXT");
    assert_simplifies("Conj(CRF, CRF)", "CRF");
}

#[test]
fn a_rewrite_can_expose_another_one() {
    assert_simplifies("Alt(FR5, Conj(Alt(FR6, FR7), CRF))", "Alt(FR5, FR6, FR7)");
    assert_simplifies("Conj(FR5, Alt(EXT, EXT))", "EXT");
    assert_simplifies(
        "Tand(FR5, Chain(CRF, Tand(FR6, FR7)))",
        "Tand(FR5, FR6, FR7)",
    );
}

#[test]
fn repeat_expands_to_tand() {
    assert_simplifies("Repeat(3, FR5)", "Tand(FR5, FR5, FR5)");
    // Theorem 7: Repeat(m, Repeat(n, S)) is Repeat(m * n, S).
    assert_eq!(
        simplified("Repeat(2, Repeat(3, FR5))"),
        simplified("Repeat(6, FR5)")
    );
    assert_simplifies("Repeat(1, Repeat(1, VI30s))", "VI30s");
}

#[test]
fn bindings_are_expanded_first() {
    assert_simplifies("let x = EXT\nAlt(FR5, x)", "FR5");
}

#[test]
fn simplifies_under_limited_hold_and_annotations() {
    assert_simplifies("Alt(FI30s LH 5s, EXT)", "FI30s LH 5s");
    assert_simplifies("let x = Alt(FI30s, EXT)\nx LH 5s", "FI30s LH 5s");
    assert_simplifies(
        "Alt(FR5, EXT) @reinforcer(\"food\")",
        "FR5 @reinforcer(\"food\")",
    );
}
