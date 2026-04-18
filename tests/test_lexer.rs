//! Lexer unit tests. Mirrors `tests/test_lexer.py` in contingency-dsl-py.

use contingency_dsl::{tokenize, DslErrorKind, TokenType};

fn token_types(source: &str) -> Vec<TokenType> {
    tokenize(source)
        .unwrap()
        .into_iter()
        .filter(|t| t.ty != TokenType::Eof)
        .map(|t| t.ty)
        .collect()
}

fn token_pairs(source: &str) -> Vec<(TokenType, String)> {
    tokenize(source)
        .unwrap()
        .into_iter()
        .filter(|t| t.ty != TokenType::Eof)
        .map(|t| (t.ty, t.value))
        .collect()
}

// ---- LimitedHold alias ----

#[test]
fn limitedhold_bare_is_lh_token() {
    assert_eq!(
        token_pairs("LimitedHold"),
        vec![(TokenType::Lh, "LimitedHold".to_string())]
    );
}

#[test]
fn limitedhold_with_number_splits() {
    assert_eq!(
        token_pairs("LimitedHold10"),
        vec![
            (TokenType::Lh, "LimitedHold".to_string()),
            (TokenType::Number, "10".to_string()),
        ]
    );
}

#[test]
fn limitedhold_with_number_and_unit() {
    assert_eq!(
        token_pairs("LimitedHold10s"),
        vec![
            (TokenType::Lh, "LimitedHold".to_string()),
            (TokenType::Number, "10".to_string()),
            (TokenType::TimeUnit, "s".to_string()),
        ]
    );
}

#[test]
fn limitedhold_param_decl() {
    assert_eq!(
        token_types("LimitedHold = 10s"),
        vec![TokenType::Lh, TokenType::Equals, TokenType::Number, TokenType::TimeUnit]
    );
}

#[test]
fn limitedhold_expression_postfix() {
    assert_eq!(
        token_types("FI30 LimitedHold10"),
        vec![
            TokenType::ScheduleType,
            TokenType::Number,
            TokenType::Lh,
            TokenType::Number,
        ]
    );
}

#[test]
fn lh_short_still_works() {
    assert_eq!(
        token_pairs("LH"),
        vec![(TokenType::Lh, "LH".to_string())]
    );
    assert_eq!(
        token_pairs("LH10"),
        vec![
            (TokenType::Lh, "LH".to_string()),
            (TokenType::Number, "10".to_string()),
        ]
    );
    assert_eq!(
        token_pairs("LH10s"),
        vec![
            (TokenType::Lh, "LH".to_string()),
            (TokenType::Number, "10".to_string()),
            (TokenType::TimeUnit, "s".to_string()),
        ]
    );
}

// ---- Unrecognized time unit suffix ----

#[test]
fn fi30m_raises_lex_error() {
    let err = tokenize("FI30m").unwrap_err();
    assert_eq!(err.kind, DslErrorKind::Lex);
    assert!(err.message.contains("unrecognized time unit suffix"));
    assert!(err.message.contains('m'));
}

#[test]
fn sec_alias_is_normalized_to_s() {
    for source in ["FR5sec", "VI60sec", "DRL5sec"] {
        let pairs = token_pairs(source);
        let last = pairs.last().expect("at least one token");
        assert_eq!(
            (last.0, last.1.as_str()),
            (TokenType::TimeUnit, "s"),
            "{}: {:?}",
            source,
            pairs
        );
    }
}

#[test]
fn valid_units_still_work() {
    assert_eq!(
        token_pairs("FR5s"),
        vec![
            (TokenType::ScheduleType, "FR".to_string()),
            (TokenType::Number, "5".to_string()),
            (TokenType::TimeUnit, "s".to_string()),
        ]
    );
    assert_eq!(
        token_pairs("VI60ms"),
        vec![
            (TokenType::ScheduleType, "VI".to_string()),
            (TokenType::Number, "60".to_string()),
            (TokenType::TimeUnit, "ms".to_string()),
        ]
    );
    assert_eq!(
        token_pairs("FI5min"),
        vec![
            (TokenType::ScheduleType, "FI".to_string()),
            (TokenType::Number, "5".to_string()),
            (TokenType::TimeUnit, "min".to_string()),
        ]
    );
}

// ---- Basic smoke tests for other productions ----

#[test]
fn ext_and_crf_are_keyword_tokens() {
    assert_eq!(token_types("EXT"), vec![TokenType::Ext]);
    assert_eq!(token_types("CRF"), vec![TokenType::Crf]);
}

#[test]
fn combinator_with_args() {
    assert_eq!(
        token_types("Conc(VI30s, VI60s)"),
        vec![
            TokenType::Combinator,
            TokenType::LParen,
            TokenType::ScheduleType,
            TokenType::Number,
            TokenType::TimeUnit,
            TokenType::Comma,
            TokenType::ScheduleType,
            TokenType::Number,
            TokenType::TimeUnit,
            TokenType::RParen,
        ]
    );
}

#[test]
fn arrow_kwarg() {
    assert_eq!(
        token_types("COD(1->2)=2s"),
        vec![
            TokenType::KwArgName,
            TokenType::LParen,
            TokenType::Number,
            TokenType::Arrow,
            TokenType::Number,
            TokenType::RParen,
            TokenType::Equals,
            TokenType::Number,
            TokenType::TimeUnit,
        ]
    );
}

#[test]
fn line_comment_is_skipped() {
    assert_eq!(
        token_types("FR 10 -- comment\nFR 5"),
        vec![
            TokenType::ScheduleType,
            TokenType::Number,
            TokenType::ScheduleType,
            TokenType::Number,
        ]
    );
}

#[test]
fn block_mode_emits_newline() {
    let src = "phase training:\n  use FR 10\nphase probe:\n  use EXT\n";
    let tokens = tokenize(src).unwrap();
    let newlines = tokens.iter().filter(|t| t.ty == TokenType::Newline).count();
    assert!(newlines >= 3, "tokens = {:?}", tokens);
}

#[test]
fn unterminated_string_literal_fails() {
    let err = tokenize("\"abc").unwrap_err();
    assert_eq!(err.kind, DslErrorKind::Lex);
    assert!(err.message.contains("unterminated"));
}
