//! Recursive descent productions for the Tier A respondent primitives.
//!
//! Mirrors a subset of `contingency_dsl/respondent_parser.py`. Entry
//! point: [`try_parse_respondent`] is invoked from
//! [`crate::parser::Parser::parse_base_schedule`] when the head token is
//! an identifier whose value names a Tier A primitive. Returns the
//! constructed respondent AST node wrapped in a `ScheduleExpr`, or `None`
//! if the token does not begin a respondent primitive.
//!
//! Coverage: Pair.{ForwardDelay, ForwardTrace, Simultaneous, Backward},
//! Extinction, CSOnly, USOnly, Contingency, TrulyRandom,
//! ExplicitlyUnpaired, Compound, Serial, ITI, Differential.
//!
//! Compound-stimulus `cs=` arguments that accept an inline `Compound(...)`
//! or `Serial(...)` call as the CS slot are supported; standalone
//! Compound / Serial top-level primitives emit their respective node types
//! without an enclosing Pair.

use crate::ast::{
    Contingency as CContingency, CsArg, CsArgKind, CsOnly, Duration, ExplicitlyUnpaired,
    Extinction, PairBackward, PairForwardDelay, PairForwardTrace, PairSimultaneous,
    RespondentCompound, RespondentDifferential, RespondentExpr, RespondentIti, RespondentSerial,
    ScheduleExpr, TrulyRandom, UsOnly,
};
use crate::errors::{DslError, Result, SourceSpan};
use crate::tokens::TokenType;

pub(crate) const RESPONDENT_PRIMITIVE_NAMES: &[&str] = &[
    "Pair",
    "Extinction",
    "CSOnly",
    "USOnly",
    "Contingency",
    "TrulyRandom",
    "ExplicitlyUnpaired",
    "Compound",
    "Serial",
    "ITI",
    "Differential",
];

pub(crate) fn is_respondent_primitive(name: &str) -> bool {
    RESPONDENT_PRIMITIVE_NAMES.iter().any(|n| *n == name)
}

pub(crate) fn try_parse_respondent(
    p: &mut crate::parser::Parser,
) -> Result<Option<ScheduleExpr>> {
    let head = p.peek().clone();
    if !is_respondent_primitive(&head.value) {
        return Ok(None);
    }
    Ok(Some(match head.value.as_str() {
        "Pair" => parse_pair(p)?,
        "Extinction" => parse_extinction(p)?,
        "CSOnly" => parse_cs_only(p)?,
        "USOnly" => parse_us_only(p)?,
        "Contingency" => parse_contingency(p)?,
        "TrulyRandom" => parse_truly_random(p)?,
        "ExplicitlyUnpaired" => parse_explicitly_unpaired(p)?,
        "Compound" => parse_compound_primitive(p)?,
        "Serial" => parse_serial_primitive(p)?,
        "ITI" => parse_iti_primitive(p)?,
        "Differential" => parse_differential(p)?,
        _ => return Ok(None),
    }))
}

fn expect_ident_value(p: &mut crate::parser::Parser, expected: &str) -> Result<()> {
    let t = p.peek().clone();
    if t.ty != TokenType::Identifier || t.value != expected {
        return Err(DslError::parse(
            format!("expected '{}', got {:?} '{}'", expected, t.ty, t.value),
            SourceSpan::new(t.line, t.col),
        ));
    }
    p.advance();
    Ok(())
}

fn parse_duration(p: &mut crate::parser::Parser) -> Result<Duration> {
    let (v, u) = p.parse_value()?;
    let unit = u.ok_or_else(|| {
        DslError::parse(
            "RESPONDENT_MISSING_TIME_UNIT",
            SourceSpan::new(p.peek().line, p.peek().col),
        )
    })?;
    Ok(Duration { value: v, unit })
}

fn parse_stimulus_ref(p: &mut crate::parser::Parser) -> Result<String> {
    let t = p.peek().clone();
    match t.ty {
        TokenType::Identifier => {
            p.advance();
            Ok(t.value)
        }
        TokenType::StringLiteral => {
            p.advance();
            Ok(t.value)
        }
        _ => Err(DslError::parse(
            format!(
                "expected stimulus reference (identifier or string), got {:?} '{}'",
                t.ty, t.value
            ),
            SourceSpan::new(t.line, t.col),
        )),
    }
}

fn parse_cs_arg(p: &mut crate::parser::Parser) -> Result<CsArg> {
    // Accept inline Compound([...]) or Serial([...], isi=...) as the CS
    // argument for compound-stimulus pair procedures.
    let head = p.peek().clone();
    if head.ty == TokenType::Identifier
        && head.value == "Compound"
        && p.peek_at(1).map_or(false, |n| n.ty == TokenType::LParen)
    {
        p.advance(); // Compound
        p.expect(TokenType::LParen, "(")?;
        p.expect(TokenType::LBracket, "[")?;
        let mut stimuli = Vec::new();
        if !p.at(TokenType::RBracket) {
            stimuli.push(parse_stimulus_ref(p)?);
            while p.at(TokenType::Comma) {
                p.advance();
                stimuli.push(parse_stimulus_ref(p)?);
            }
        }
        p.expect(TokenType::RBracket, "]")?;
        p.expect(TokenType::RParen, ")")?;
        return Ok(CsArg { kind: CsArgKind::Compound(stimuli) });
    }
    if head.ty == TokenType::Identifier
        && head.value == "Serial"
        && p.peek_at(1).map_or(false, |n| n.ty == TokenType::LParen)
    {
        p.advance(); // Serial
        p.expect(TokenType::LParen, "(")?;
        p.expect(TokenType::LBracket, "[")?;
        let mut stimuli = Vec::new();
        if !p.at(TokenType::RBracket) {
            stimuli.push(parse_stimulus_ref(p)?);
            while p.at(TokenType::Comma) {
                p.advance();
                stimuli.push(parse_stimulus_ref(p)?);
            }
        }
        p.expect(TokenType::RBracket, "]")?;
        p.expect(TokenType::Comma, ",")?;
        expect_ident_value(p, "isi")?;
        p.expect(TokenType::Equals, "=")?;
        let mut isis = Vec::new();
        if p.at(TokenType::LBracket) {
            p.advance();
            if !p.at(TokenType::RBracket) {
                isis.push(parse_duration(p)?);
                while p.at(TokenType::Comma) {
                    p.advance();
                    isis.push(parse_duration(p)?);
                }
            }
            p.expect(TokenType::RBracket, "]")?;
        } else {
            isis.push(parse_duration(p)?);
        }
        p.expect(TokenType::RParen, ")")?;
        return Ok(CsArg {
            kind: CsArgKind::Serial { stimuli, isis },
        });
    }
    let name = parse_stimulus_ref(p)?;
    Ok(CsArg { kind: CsArgKind::Ref(name) })
}

fn parse_pair(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    let _pair_tok = p.advance();
    p.expect(TokenType::Dot, ". after Pair")?;
    let variant_tok = p.expect(TokenType::Identifier, "Pair variant")?;
    p.expect(TokenType::LParen, "( after Pair variant")?;
    // Pair.Backward source text takes (us, cs, ...) positional order (Spetch
    // et al., 1981). All other Pair variants take (cs, us, ...).
    let swap = variant_tok.value == "Backward";
    let first = parse_cs_arg(p)?;
    p.expect(TokenType::Comma, ",")?;
    let second = if p.at(TokenType::Identifier)
        && (p.peek().value == "us" || p.peek().value == "cs")
        && p.peek_at(1).map_or(false, |n| n.ty == TokenType::Equals)
    {
        p.advance();
        p.expect(TokenType::Equals, "=")?;
        parse_stimulus_ref(p)?
    } else {
        parse_stimulus_ref(p)?
    };
    let (cs, us) = if swap {
        // `first` was us (as CsArg); extract its ref form.
        let us_name = match first.kind {
            CsArgKind::Ref(s) => s,
            _ => {
                return Err(DslError::parse(
                    "Pair.Backward US must be a single identifier",
                    SourceSpan::new(variant_tok.line, variant_tok.col),
                ))
            }
        };
        (CsArg { kind: CsArgKind::Ref(second) }, us_name)
    } else {
        (first, second)
    };
    let mut isi: Option<Duration> = None;
    let mut cs_duration: Option<Duration> = None;
    let mut trace_interval: Option<Duration> = None;
    let mut backward_interval: Option<Duration> = None;
    let mut duration: Option<Duration> = None;
    while p.at(TokenType::Comma) {
        p.advance();
        let key = p.expect(TokenType::Identifier, "Pair kwarg name")?;
        p.expect(TokenType::Equals, "=")?;
        match key.value.as_str() {
            "isi" => isi = Some(parse_duration(p)?),
            "cs_duration" => cs_duration = Some(parse_duration(p)?),
            "trace_interval" => trace_interval = Some(parse_duration(p)?),
            "backward_interval" => backward_interval = Some(parse_duration(p)?),
            "duration" => duration = Some(parse_duration(p)?),
            _ => {
                return Err(DslError::parse(
                    format!("unknown Pair kwarg '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    let expr = match variant_tok.value.as_str() {
        "ForwardDelay" => RespondentExpr::PairForwardDelay(PairForwardDelay {
            cs,
            us,
            isi: isi.ok_or_else(|| {
                DslError::parse(
                    "PAIR_FORWARDDELAY_MISSING_ISI",
                    SourceSpan::new(variant_tok.line, variant_tok.col),
                )
            })?,
            cs_duration: cs_duration.ok_or_else(|| {
                DslError::parse(
                    "PAIR_FORWARDDELAY_MISSING_CS_DURATION",
                    SourceSpan::new(variant_tok.line, variant_tok.col),
                )
            })?,
        }),
        "ForwardTrace" => {
            let ti = trace_interval.ok_or_else(|| {
                DslError::parse(
                    "PAIR_FORWARDTRACE_MISSING_TRACE_INTERVAL",
                    SourceSpan::new(variant_tok.line, variant_tok.col),
                )
            })?;
            RespondentExpr::PairForwardTrace(PairForwardTrace {
                cs,
                us,
                cs_duration: cs_duration.unwrap_or_else(|| Duration {
                    value: 0.0,
                    unit: "s".into(),
                }),
                trace_interval: ti,
            })
        }
        "Simultaneous" => RespondentExpr::PairSimultaneous(PairSimultaneous {
            cs,
            us,
            duration: duration.unwrap_or_else(|| Duration {
                value: 0.0,
                unit: "s".into(),
            }),
        }),
        "Backward" => RespondentExpr::PairBackward(PairBackward {
            cs,
            us,
            cs_duration: cs_duration.unwrap_or_else(|| Duration {
                value: 0.0,
                unit: "s".into(),
            }),
            backward_interval: backward_interval.or(isi).ok_or_else(|| {
                DslError::parse(
                    "PAIR_BACKWARD_MISSING_INTERVAL",
                    SourceSpan::new(variant_tok.line, variant_tok.col),
                )
            })?,
        }),
        _ => {
            return Err(DslError::parse(
                format!("unknown Pair variant '{}'", variant_tok.value),
                SourceSpan::new(variant_tok.line, variant_tok.col),
            ))
        }
    };
    Ok(ScheduleExpr::Respondent(Box::new(expr)))
}

fn parse_single_stim_arg(
    p: &mut crate::parser::Parser,
    kw_name: &str,
) -> Result<String> {
    // Accept either positional or `<kw_name>=<stim>`.
    if p.at(TokenType::Identifier) && p.peek().value == kw_name {
        let next_eq = p.peek_at(1).map_or(false, |n| n.ty == TokenType::Equals);
        if next_eq {
            p.advance();
            p.expect(TokenType::Equals, "=")?;
        }
    }
    parse_stimulus_ref(p)
}

fn parse_extinction(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after Extinction")?;
    let cs = parse_single_stim_arg(p, "cs")?;
    let mut trials: Option<i64> = None;
    let mut iti: Option<Duration> = None;
    while p.at(TokenType::Comma) {
        p.advance();
        let key = p.expect(TokenType::Identifier, "Extinction kwarg name")?;
        p.expect(TokenType::Equals, "=")?;
        match key.value.as_str() {
            "trials" => {
                let nt = p.expect(TokenType::Number, "trials")?;
                trials = Some(nt.value.parse().map_err(|_| {
                    DslError::parse(
                        format!("invalid trials: {}", nt.value),
                        SourceSpan::new(nt.line, nt.col),
                    )
                })?);
            }
            "iti" => iti = Some(parse_duration(p)?),
            _ => {
                return Err(DslError::parse(
                    format!("unknown Extinction kwarg '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(RespondentExpr::Extinction(
        Extinction { cs, trials, iti },
    ))))
}

fn parse_cs_only(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after CSOnly")?;
    let cs = parse_single_stim_arg(p, "cs")?;
    let mut cs_duration: Option<Duration> = None;
    let mut trials: Option<i64> = None;
    while p.at(TokenType::Comma) {
        p.advance();
        // Positional trials integer after cs.
        if p.at(TokenType::Number) && trials.is_none() {
            let nt = p.advance();
            if !nt.value.contains('.') {
                trials = Some(nt.value.parse().unwrap_or(0));
                continue;
            }
        }
        let key = p.expect(TokenType::Identifier, "CSOnly kwarg name")?;
        p.expect(TokenType::Equals, "=")?;
        match key.value.as_str() {
            "cs_duration" => cs_duration = Some(parse_duration(p)?),
            "trials" => {
                let nt = p.expect(TokenType::Number, "trials")?;
                trials = Some(nt.value.parse().map_err(|_| {
                    DslError::parse(
                        format!("invalid trials: {}", nt.value),
                        SourceSpan::new(nt.line, nt.col),
                    )
                })?);
            }
            _ => {
                return Err(DslError::parse(
                    format!("unknown CSOnly kwarg '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(RespondentExpr::CsOnly(
        CsOnly {
            cs,
            cs_duration: cs_duration.unwrap_or_else(|| Duration {
                value: 0.0,
                unit: "s".into(),
            }),
            trials,
        },
    ))))
}

fn parse_us_only(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after USOnly")?;
    let us = parse_single_stim_arg(p, "us")?;
    let mut trials: Option<i64> = None;
    while p.at(TokenType::Comma) {
        p.advance();
        if p.at(TokenType::Number) && trials.is_none() {
            let nt = p.advance();
            if !nt.value.contains('.') {
                trials = Some(nt.value.parse().unwrap_or(0));
                continue;
            }
        }
        let key = p.expect(TokenType::Identifier, "USOnly kwarg name")?;
        p.expect(TokenType::Equals, "=")?;
        match key.value.as_str() {
            "trials" => {
                let nt = p.expect(TokenType::Number, "trials")?;
                trials = Some(nt.value.parse().map_err(|_| {
                    DslError::parse(
                        format!("invalid trials: {}", nt.value),
                        SourceSpan::new(nt.line, nt.col),
                    )
                })?);
            }
            _ => {
                return Err(DslError::parse(
                    format!("unknown USOnly kwarg '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(RespondentExpr::UsOnly(
        UsOnly { us, trials },
    ))))
}

fn parse_contingency(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after Contingency")?;
    // Two forms: Contingency(p_cs, p_no_cs) with bare numbers, or
    // Contingency(cs, us, p_us_given_cs=..., p_us_given_no_cs=...).
    let mut cs = String::new();
    let mut us = String::new();
    let mut p_us_given_cs: Option<f64> = None;
    let mut p_us_no_cs: Option<f64> = None;
    if p.at(TokenType::Number) {
        let nt = p.advance();
        p_us_given_cs = Some(nt.value.parse().unwrap_or(0.0));
        p.expect(TokenType::Comma, ",")?;
        let nt2 = p.expect(TokenType::Number, "p_us_no_cs")?;
        p_us_no_cs = Some(nt2.value.parse().unwrap_or(0.0));
    } else {
        cs = parse_single_stim_arg(p, "cs")?;
        p.expect(TokenType::Comma, ",")?;
        us = parse_single_stim_arg(p, "us")?;
        while p.at(TokenType::Comma) {
            p.advance();
            let key = p.expect(TokenType::Identifier, "Contingency kwarg")?;
            p.expect(TokenType::Equals, "=")?;
            let nt = p.expect(TokenType::Number, "probability")?;
            let v: f64 = nt.value.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid probability: {}", nt.value),
                    SourceSpan::new(nt.line, nt.col),
                )
            })?;
            match key.value.as_str() {
                "p_us_given_cs" => p_us_given_cs = Some(v),
                "p_us_given_no_cs" | "p_us_no_cs" => p_us_no_cs = Some(v),
                _ => {
                    return Err(DslError::parse(
                        format!("unknown Contingency kwarg '{}'", key.value),
                        SourceSpan::new(key.line, key.col),
                    ))
                }
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(
        RespondentExpr::Contingency(CContingency {
            cs,
            us,
            p_us_given_cs: p_us_given_cs.unwrap_or(0.0),
            p_us_no_cs: p_us_no_cs.unwrap_or(0.0),
        }),
    )))
}

fn parse_truly_random(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after TrulyRandom")?;
    let cs = parse_single_stim_arg(p, "cs")?;
    p.expect(TokenType::Comma, ",")?;
    let us = parse_single_stim_arg(p, "us")?;
    let mut rate_cs = 0.0;
    let mut rate_us = 0.0;
    while p.at(TokenType::Comma) {
        p.advance();
        let key = p.expect(TokenType::Identifier, "TrulyRandom kwarg")?;
        p.expect(TokenType::Equals, "=")?;
        let nt = p.expect(TokenType::Number, "rate")?;
        let v: f64 = nt.value.parse().map_err(|_| {
            DslError::parse(
                format!("invalid rate: {}", nt.value),
                SourceSpan::new(nt.line, nt.col),
            )
        })?;
        // Skip optional time-unit suffix on rate values.
        if p.at(TokenType::TimeUnit) {
            p.advance();
        }
        match key.value.as_str() {
            "rate_cs" | "p" => rate_cs = v,
            "rate_us" => rate_us = v,
            _ => {
                return Err(DslError::parse(
                    format!("unknown TrulyRandom kwarg '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(
        RespondentExpr::TrulyRandom(TrulyRandom {
            cs,
            us,
            rate_cs,
            rate_us,
        }),
    )))
}

fn parse_explicitly_unpaired(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after ExplicitlyUnpaired")?;
    let cs = parse_single_stim_arg(p, "cs")?;
    p.expect(TokenType::Comma, ",")?;
    let us = parse_single_stim_arg(p, "us")?;
    let mut min_separation: Option<Duration> = None;
    while p.at(TokenType::Comma) {
        p.advance();
        let key = p.expect(TokenType::Identifier, "ExplicitlyUnpaired kwarg")?;
        p.expect(TokenType::Equals, "=")?;
        if key.value == "min_separation" {
            min_separation = Some(parse_duration(p)?);
        } else {
            return Err(DslError::parse(
                format!("unknown ExplicitlyUnpaired kwarg '{}'", key.value),
                SourceSpan::new(key.line, key.col),
            ));
        }
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(
        RespondentExpr::ExplicitlyUnpaired(ExplicitlyUnpaired {
            cs,
            us,
            min_separation: min_separation.unwrap_or_else(|| Duration {
                value: 0.0,
                unit: "s".into(),
            }),
        }),
    )))
}

fn parse_compound_primitive(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after Compound")?;
    p.expect(TokenType::LBracket, "[")?;
    let mut stimuli = Vec::new();
    if !p.at(TokenType::RBracket) {
        stimuli.push(parse_stimulus_ref(p)?);
        while p.at(TokenType::Comma) {
            p.advance();
            stimuli.push(parse_stimulus_ref(p)?);
        }
    }
    p.expect(TokenType::RBracket, "]")?;
    // Optional mode=overlap|sequential kwarg (ignored here; default "overlap").
    while p.at(TokenType::Comma) {
        p.advance();
        let key = p.expect(TokenType::Identifier, "Compound kwarg name")?;
        p.expect(TokenType::Equals, "=")?;
        // Consume the value to advance the stream.
        let _v = p.advance();
        let _ = key;
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(RespondentExpr::Compound(
        RespondentCompound { stimuli },
    ))))
}

fn parse_serial_primitive(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after Serial")?;
    p.expect(TokenType::LBracket, "[")?;
    let mut stimuli = Vec::new();
    if !p.at(TokenType::RBracket) {
        stimuli.push(parse_stimulus_ref(p)?);
        while p.at(TokenType::Comma) {
            p.advance();
            stimuli.push(parse_stimulus_ref(p)?);
        }
    }
    p.expect(TokenType::RBracket, "]")?;
    p.expect(TokenType::Comma, ",")?;
    expect_ident_value(p, "isi")?;
    p.expect(TokenType::Equals, "=")?;
    let mut isis = Vec::new();
    if p.at(TokenType::LBracket) {
        p.advance();
        if !p.at(TokenType::RBracket) {
            isis.push(parse_duration(p)?);
            while p.at(TokenType::Comma) {
                p.advance();
                isis.push(parse_duration(p)?);
            }
        }
        p.expect(TokenType::RBracket, "]")?;
    } else {
        isis.push(parse_duration(p)?);
    }
    p.expect(TokenType::RParen, ")")?;
    Ok(ScheduleExpr::Respondent(Box::new(RespondentExpr::Serial(
        RespondentSerial { stimuli, isis },
    ))))
}

fn parse_iti_primitive(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after ITI")?;
    let mut mode: String = "fixed".into();
    let mut mean: Option<Duration> = None;
    let mut fixed: Option<Duration> = None;
    let mut uniform_min: Option<Duration> = None;
    let mut uniform_max: Option<Duration> = None;
    let mut exponential: Option<Duration> = None;
    let mut seen_first = false;
    while !p.at(TokenType::RParen) {
        if seen_first {
            p.expect(TokenType::Comma, ", after ITI kwarg")?;
        }
        seen_first = true;
        let head = p.peek().clone();
        let key_name = match head.ty {
            TokenType::Identifier => head.value.clone(),
            TokenType::DistributionName => head.value.clone(),
            _ => {
                return Err(DslError::parse(
                    format!("unexpected token in ITI: {:?} '{}'", head.ty, head.value),
                    SourceSpan::new(head.line, head.col),
                ))
            }
        };
        p.advance();
        p.expect(TokenType::Equals, "= after ITI kwarg")?;
        match key_name.as_str() {
            "mode" => {
                let vt = p.advance();
                mode = vt.value;
            }
            "mean" => mean = Some(parse_duration(p)?),
            "fixed" => fixed = Some(parse_duration(p)?),
            "min" => uniform_min = Some(parse_duration(p)?),
            "max" => uniform_max = Some(parse_duration(p)?),
            "exponential" => exponential = Some(parse_duration(p)?),
            _ => {
                return Err(DslError::parse(
                    format!("unknown ITI kwarg '{}'", key_name),
                    SourceSpan::new(head.line, head.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    let uniform = match (uniform_min, uniform_max) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    Ok(ScheduleExpr::Respondent(Box::new(RespondentExpr::Iti(
        RespondentIti {
            mode,
            mean,
            fixed,
            uniform,
            exponential,
        },
    ))))
}

fn parse_differential(p: &mut crate::parser::Parser) -> Result<ScheduleExpr> {
    p.advance();
    p.expect(TokenType::LParen, "( after Differential")?;
    let mut cs_plus: Option<String> = None;
    let mut cs_minus: Option<String> = None;
    let mut us: Option<String> = None;
    let mut isi: Option<Duration> = None;
    let mut cs_duration: Option<Duration> = None;
    let mut seen_first = false;
    while !p.at(TokenType::RParen) {
        if seen_first {
            p.expect(TokenType::Comma, ",")?;
        }
        seen_first = true;
        let key = p.expect(TokenType::Identifier, "Differential kwarg")?;
        p.expect(TokenType::Equals, "=")?;
        match key.value.as_str() {
            "cs_plus" | "cs_positive" => cs_plus = Some(parse_stimulus_ref(p)?),
            "cs_minus" | "cs_negative" => cs_minus = Some(parse_stimulus_ref(p)?),
            "us" => us = Some(parse_stimulus_ref(p)?),
            "isi" => isi = Some(parse_duration(p)?),
            "cs_duration" => cs_duration = Some(parse_duration(p)?),
            _ => {
                return Err(DslError::parse(
                    format!("unknown Differential kwarg '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ))
            }
        }
    }
    p.expect(TokenType::RParen, ")")?;
    let cs_plus = cs_plus.ok_or_else(|| {
        DslError::parse(
            "DIFFERENTIAL_MISSING_CS_PLUS",
            SourceSpan::new(p.peek().line, p.peek().col),
        )
    })?;
    let cs_minus = cs_minus.ok_or_else(|| {
        DslError::parse(
            "DIFFERENTIAL_MISSING_CS_MINUS",
            SourceSpan::new(p.peek().line, p.peek().col),
        )
    })?;
    let us = us.unwrap_or_default();
    Ok(ScheduleExpr::Respondent(Box::new(
        RespondentExpr::Differential(RespondentDifferential {
            cs_plus,
            cs_minus,
            us,
            isi: isi.unwrap_or_else(|| Duration { value: 0.0, unit: "s".into() }),
            cs_duration: cs_duration.unwrap_or_else(|| Duration {
                value: 0.0,
                unit: "s".into(),
            }),
        }),
    )))
}
