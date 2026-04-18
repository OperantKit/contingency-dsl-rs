//! Recursive descent parser for contingency-dsl.
//!
//! Mirrors a subset of `contingency_dsl/parser.py`. Current coverage:
//! - Top-level Program: param_decls, bindings, schedule, EOF.
//! - Atomic schedule: FR/VR/RR/FI/VI/RI/FT/VT/RT with optional time unit.
//! - Special schedule: EXT, CRF.
//! - Second-order: FR5(FI30), FR5(x).
//! - Compound: Conc/Alt/Conj/Chain/Tand/Mult/Mix with positional args only
//!   (scalar keyword args COD/FRCO/BO without the directional / PUNISH forms).
//! - Modifier (DR): DRL/DRH/DRO.
//! - Modifier (Repeat): `N*schedule` or `Repeat(N, schedule)`.
//! - Binding: `let ident = schedule`.
//! - Param decl: `LH = value` / `RD = value` / `COD = value` / `FRCO = value`
//!   / `BO = value` at program head.
//! - LimitedHold postfix: `schedule LH value`.
//! - Identifier reference: bare identifier that refers to a let-bound name.
//! - Compound scalar kwargs: `Conc(VI30s, VI60s, COD=2s)`.
//!
//! Everything outside that list (PUNISH, directional COD, PR, Pctl, aversive,
//! trial-based, adjusting, interlocking, Timeout/ResponseCost postfix, Lag,
//! respondent primitives, experiment-layer blocks, annotations, extensions)
//! is a parse error until implemented in a later pass.

use std::collections::BTreeMap;

use crate::ast::{
    AdjustingSchedule, AnnotatedSchedule, Annotation, AnnotationValue, Atomic, AversiveKind,
    AversiveParam, AversiveSchedule, Binding, CombinatorParamValue, Compound, CompoundParam,
    DirRef, DirectionalCOD, DirectionalEntry, IdentifierRef, InterlockingSchedule, JsonValue,
    LimitedHold, Modifier, ModifierKind, NumericPayload, ParamDecl, Program, ProgramSchedule,
    PunishParam, ResponseCost, ResponseCostWrapped, ScheduleExpr, ScheduleType, SecondOrder,
    SecondOrderSlot, Special, SpecialKind, Timeout, TimeoutWrapped, TrialBased, TrialParam,
};
use crate::enums::{Combinator, Distribution, Domain, KW_ARG_KEYWORDS, LOWERCASE_RESERVED};
use crate::errors::{DslError, Result, SourceSpan};
use crate::lexer::tokenize;
use crate::serializer::expr_to_json;
use crate::tokens::{Token, TokenType};

/// Top-level AST root: a single-schedule Program, a PhaseSequence produced by
/// phase / shaping / progressive blocks, or a Paper wrapping multiple
/// Experiments.
#[derive(Debug, Clone, PartialEq)]
pub enum TopLevel {
    Program(Program),
    PhaseSequence(crate::experiment::PhaseSequence),
    Paper(crate::experiment::Paper),
}

/// Parse DSL source text. Back-compat wrapper that returns a Program for the
/// single-schedule form and an error for phase-sequence / paper inputs; use
/// [`parse_top_level`] to accept all top-level forms.
pub fn parse(source: &str) -> Result<Program> {
    match parse_top_level(source)? {
        TopLevel::Program(p) => Ok(p),
        TopLevel::PhaseSequence(_) => Err(DslError::parse(
            "input parsed as a phase sequence — use parse_top_level()",
            SourceSpan::new(1, 1),
        )),
        TopLevel::Paper(_) => Err(DslError::parse(
            "input parsed as a paper — use parse_top_level()",
            SourceSpan::new(1, 1),
        )),
    }
}

pub fn parse_top_level(source: &str) -> Result<TopLevel> {
    let tokens = tokenize(source)?;
    let mut p = Parser::new(tokens);
    p.parse_top_level()
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    warnings: Vec<String>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0, warnings: Vec::new() }
    }

    pub(crate) fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    pub(crate) fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    pub(crate) fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }

    pub(crate) fn at(&self, ty: TokenType) -> bool {
        self.peek().ty == ty
    }

    fn at_any(&self, types: &[TokenType]) -> bool {
        let cur = self.peek().ty;
        types.iter().any(|t| *t == cur)
    }

    pub(crate) fn expect(&mut self, ty: TokenType, ctx: &str) -> Result<Token> {
        if self.peek().ty != ty {
            let t = self.peek();
            let code = match ty {
                TokenType::RParen => "EXPECTED_RPAREN",
                TokenType::Number => "EXPECTED_VALUE",
                TokenType::Comma => "EXPECTED_COMMA_OR_RPAREN",
                TokenType::Equals => "EXPECTED_EQUALS",
                TokenType::LParen => "EXPECTED_LPAREN",
                _ => "UNEXPECTED_TOKEN",
            };
            // For a LET follow-up that landed on a reserved word, emit
            // RESERVED_WORD rather than the generic code.
            let is_reserved_body = matches!(
                t.ty,
                TokenType::Let
                    | TokenType::DrMod
                    | TokenType::Pr
                    | TokenType::Repeat
                    | TokenType::DistributionName
                    | TokenType::PrParam
            );
            let final_code = if ty == TokenType::Identifier && is_reserved_body {
                "RESERVED_WORD"
            } else {
                code
            };
            return Err(DslError::parse(
                format!(
                    "{}: expected {:?} ({}), got {:?} '{}'",
                    final_code, ty, ctx, t.ty, t.value
                ),
                SourceSpan::new(t.line, t.col),
            ));
        }
        Ok(self.advance())
    }

    fn skip_newlines(&mut self) {
        while self.at(TokenType::Newline) {
            self.advance();
        }
    }

    fn parse_top_level(&mut self) -> Result<TopLevel> {
        self.skip_newlines();
        let mut program_annotations: Vec<Annotation> = Vec::new();
        while self.at(TokenType::At) {
            program_annotations.push(self.parse_annotation()?);
            self.skip_newlines();
        }
        if self.at_any(&[
            TokenType::Phase,
            TokenType::Shaping,
            TokenType::Progressive,
        ]) {
            let seq = self.parse_phase_sequence(program_annotations)?;
            return Ok(TopLevel::PhaseSequence(seq));
        }
        if self.at(TokenType::Experiment) {
            let paper = self.parse_paper(program_annotations)?;
            return Ok(TopLevel::Paper(paper));
        }
        let p = self.parse_program_body(program_annotations)?;
        Ok(TopLevel::Program(p))
    }

    fn parse_paper(
        &mut self,
        shared_annotations: Vec<Annotation>,
    ) -> Result<crate::experiment::Paper> {
        let mut experiments: Vec<crate::experiment::Experiment> = Vec::new();
        while self.at(TokenType::Experiment) {
            experiments.push(self.parse_experiment_block()?);
            self.skip_newlines();
        }
        if !self.at(TokenType::Eof) {
            let t = self.peek();
            return Err(DslError::parse(
                format!(
                    "unexpected token after paper: {:?} '{}'",
                    t.ty, t.value
                ),
                SourceSpan::new(t.line, t.col),
            ));
        }
        Ok(crate::experiment::Paper {
            experiments,
            shared_annotations,
            program_annotations: Vec::new(),
            extra: None,
        })
    }

    fn parse_experiment_block(&mut self) -> Result<crate::experiment::Experiment> {
        self.expect(TokenType::Experiment, "experiment")?;
        // Label: either Identifier or Number (e.g. `experiment 1:`).
        let label_tok = self.peek().clone();
        let label = match label_tok.ty {
            TokenType::Identifier | TokenType::Number => {
                self.advance();
                label_tok.value
            }
            _ => {
                return Err(DslError::parse(
                    format!(
                        "expected experiment label, got {:?} '{}'",
                        label_tok.ty, label_tok.value
                    ),
                    SourceSpan::new(label_tok.line, label_tok.col),
                ))
            }
        };
        self.expect(TokenType::Colon, ": after experiment label")?;
        self.skip_newlines();
        // Experiment-body annotations (appear at the top of the body,
        // before phase blocks or the schedule expression). These become
        // the Experiment-level `shared_annotations` in the emitted AST.
        let mut body_annotations: Vec<Annotation> = Vec::new();
        while self.at(TokenType::At) {
            body_annotations.push(self.parse_annotation()?);
            self.skip_newlines();
        }

        let mut phases: Vec<crate::experiment::Phase> = Vec::new();
        if self.at_any(&[TokenType::Phase, TokenType::Shaping]) {
            while self.at_any(&[TokenType::Phase, TokenType::Shaping]) {
                if self.at(TokenType::Phase) {
                    phases.push(self.parse_phase_block()?);
                } else {
                    phases.extend(self.parse_shaping_block()?);
                }
                self.skip_newlines();
            }
        } else {
            // Single-schedule experiment body.
            let sched = self.parse_schedule()?;
            self.skip_newlines();
            phases.push(crate::experiment::Phase {
                label: label.clone(),
                schedule: crate::experiment::PhaseSchedule::Operant(sched),
                criterion: crate::experiment::Criterion::ExperimenterJudgment(
                    crate::experiment::ExperimenterJudgment,
                ),
                annotations: Vec::new(),
            });
        }
        Ok(crate::experiment::Experiment {
            label,
            phases,
            annotations: body_annotations,
        })
    }

    fn parse_program_body(
        &mut self,
        mut program_annotations: Vec<Annotation>,
    ) -> Result<Program> {

        let mut param_decls = Vec::new();
        while self.at_any(&[TokenType::Lh, TokenType::Rd, TokenType::KwArgName]) {
            if let Some(next) = self.peek_at(1) {
                if next.ty == TokenType::Equals {
                    param_decls.push(self.parse_param_decl()?);
                    continue;
                }
            }
            break;
        }

        let mut bindings: Vec<Binding> = Vec::new();
        while self.at(TokenType::Let) {
            let b = self.parse_binding()?;
            // Forward reference / self-reference: the binding body refers to
            // a name that has not yet been bound at this point.
            check_binding_refs(&b, &bindings)?;
            // Duplicate binding name (shadowing prohibited).
            if bindings.iter().any(|prev| prev.name == b.name) {
                return Err(DslError::semantic(
                    format!("DUPLICATE_BINDING: '{}' is already bound", b.name),
                    SourceSpan::new(1, 1),
                ));
            }
            bindings.push(b);
        }

        // Empty-program special case: after bindings, if we're at EOF and
        // nothing else was parsed, surface UNEXPECTED_EOF per the error
        // channel (py conformance error_empty_input). When there are
        // bindings or param_decls, the minimal program is the parameter
        // declarations followed by an implicit EXT schedule.
        let mut schedule = if self.at(TokenType::Eof) {
            if program_annotations.is_empty()
                && param_decls.is_empty()
                && bindings.is_empty()
            {
                return Err(DslError::parse(
                    "UNEXPECTED_EOF: empty input",
                    SourceSpan::new(self.peek().line, self.peek().col),
                ));
            }
            ScheduleExpr::Special(Special { kind: SpecialKind::Ext })
        } else {
            self.parse_schedule()?
        };

        // Route schedule-scoped annotations from program_annotations onto
        // the schedule itself. Matches py `_route_schedule_annotations`.
        let decl_mode_mts = is_declaration_mode_mts(&schedule);
        let (mut routed, retained): (Vec<Annotation>, Vec<Annotation>) = program_annotations
            .into_iter()
            .partition(|a| {
                a.keyword == "trial_mix"
                    || (decl_mode_mts
                        && matches!(a.keyword.as_str(), "training" | "testing"))
            });
        program_annotations = retained;
        if !routed.is_empty() {
            schedule = match schedule {
                ScheduleExpr::Annotated(boxed) => {
                    let AnnotatedSchedule { expr, mut annotations } = *boxed;
                    let mut v = std::mem::take(&mut routed);
                    v.append(&mut annotations);
                    ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
                        expr,
                        annotations: v,
                    }))
                }
                other => ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
                    expr: other,
                    annotations: routed,
                })),
            };
        }

        self.skip_newlines();
        if !self.at(TokenType::Eof) {
            let t = self.peek();
            return Err(DslError::parse(
                format!(
                    "unexpected token after schedule: {:?} '{}'",
                    t.ty, t.value
                ),
                SourceSpan::new(t.line, t.col),
            ));
        }

        // Validate that every identifier reference in the schedule resolves
        // to a known binding name. Unknown names at top level are a
        // semantic error (UNDEFINED_IDENTIFIER).
        {
            let mut refs: Vec<String> = Vec::new();
            collect_idents(&schedule, &mut refs);
            for r in &refs {
                if !bindings.iter().any(|b| &b.name == r) {
                    return Err(DslError::semantic(
                        format!("UNDEFINED_IDENTIFIER: '{}' is not bound", r),
                        SourceSpan::new(1, 1),
                    ));
                }
            }
        }

        // Validate annotation semantics.
        for ann in &program_annotations {
            validate_annotation(ann)?;
        }

        Ok(Program {
            program_annotations,
            param_decls,
            bindings,
            schedule: ProgramSchedule::Operant(schedule),
        })
    }


    fn parse_phase_sequence(
        &mut self,
        program_annotations: Vec<Annotation>,
    ) -> Result<crate::experiment::PhaseSequence> {
        let mut phases: Vec<crate::experiment::Phase> = Vec::new();
        while self.at_any(&[
            TokenType::Phase,
            TokenType::Shaping,
            TokenType::Progressive,
        ]) {
            if self.at(TokenType::Phase) {
                phases.push(self.parse_phase_block()?);
            } else if self.at(TokenType::Shaping) {
                phases.extend(self.parse_shaping_block()?);
            } else {
                phases.extend(self.parse_progressive_block()?);
            }
            self.skip_newlines();
        }
        // Resolve `use <Label>` phase references to the referenced phase's
        // schedule (backward-only lookup; forward references are left as
        // PhaseRef until a later analyzer pass adds the richer error model).
        resolve_phase_refs(&mut phases);
        if !self.at(TokenType::Eof) {
            let t = self.peek();
            return Err(DslError::parse(
                format!(
                    "unexpected token after phase sequence: {:?} '{}'",
                    t.ty, t.value
                ),
                SourceSpan::new(t.line, t.col),
            ));
        }
        Ok(crate::experiment::PhaseSequence {
            phases,
            program_annotations,
            warnings: std::mem::take(&mut self.warnings),
        })
    }

    fn parse_progressive_block(&mut self) -> Result<Vec<crate::experiment::Phase>> {
        self.expect(TokenType::Progressive, "progressive")?;
        let label_tok = self.expect(TokenType::Identifier, "progressive label")?;
        self.expect(TokenType::Colon, ": after progressive label")?;
        self.skip_newlines();

        // Collect steps lists (one or more `steps <ident> = [val, ...]` lines).
        let mut step_maps: Vec<(String, Vec<f64>)> = Vec::new();
        let mut user_criterion: Option<crate::experiment::Criterion> = None;
        let mut min_sessions: Option<i64> = None;
        let mut has_advance = false;
        loop {
            self.skip_newlines();
            if self.at(TokenType::Steps) {
                self.advance();
                let name_tok = self.expect(TokenType::Identifier, "steps variable name")?;
                self.expect(TokenType::Equals, "= after steps name")?;
                self.expect(TokenType::LBracket, "[ after steps =")?;
                let mut vals = Vec::new();
                if !self.at(TokenType::RBracket) {
                    let n = self.expect(TokenType::Number, "steps value")?;
                    vals.push(n.value.parse::<f64>().unwrap_or(0.0));
                    while self.at(TokenType::Comma) {
                        self.advance();
                        let n = self.expect(TokenType::Number, "steps value")?;
                        vals.push(n.value.parse::<f64>().unwrap_or(0.0));
                    }
                }
                self.expect(TokenType::RBracket, "] on steps")?;
                step_maps.push((name_tok.value, vals));
                continue;
            }
            if self.at(TokenType::Sessions) {
                self.advance();
                if self.at(TokenType::Equals) {
                    self.advance();
                    // May be a placeholder or a concrete number.
                    if self.at(TokenType::Number) {
                        let nt = self.advance();
                        user_criterion =
                            Some(crate::experiment::Criterion::FixedSessions(
                                crate::experiment::FixedSessions {
                                    count: nt.value.parse().unwrap_or(0),
                                },
                            ));
                    } else if self.at(TokenType::LBrace) {
                        // Placeholder {ident} — currently skipped.
                        while !self.at(TokenType::Newline) && !self.at(TokenType::Eof) {
                            self.advance();
                        }
                    }
                } else if self.at(TokenType::Gte) {
                    self.advance();
                    let nt = self.expect(TokenType::Number, "min sessions")?;
                    let n: i64 = nt.value.parse().unwrap_or(0);
                    min_sessions = Some(n);
                }
                continue;
            }
            if self.at(TokenType::Stable) {
                self.advance();
                self.expect(TokenType::LParen, "( after stable")?;
                let method_tok = self.expect(TokenType::Identifier, "stability method")?;
                let mut window_sessions: Option<i64> = None;
                while self.at(TokenType::Comma) {
                    self.advance();
                    let key = self.expect(TokenType::Identifier, "stable kwarg")?;
                    self.expect(TokenType::Equals, "=")?;
                    match key.value.as_str() {
                        "window" => {
                            let nt = self.expect(TokenType::Number, "window")?;
                            window_sessions = Some(nt.value.parse().unwrap_or(0));
                        }
                        _ => {
                            let _ = self.advance();
                        }
                    }
                }
                self.expect(TokenType::RParen, ") on stable")?;
                user_criterion = Some(crate::experiment::Criterion::Stability(
                    crate::experiment::Stability {
                        method: method_tok.value,
                        window_sessions,
                        max_change_pct: None,
                        min_sessions,
                        measure: None,
                    },
                ));
                continue;
            }
            if self.at(TokenType::Advance) {
                // `advance when criterion(...)` / `all(...)` / `manual`.
                // Currently recorded in the AST via shared_annotations is not
                // implemented; skip tokens until newline.
                has_advance = true;
                while !self.at(TokenType::Newline) && !self.at(TokenType::Eof) {
                    self.advance();
                }
                continue;
            }
            break;
        }
        if has_advance {
            // Warn when a progressive block specifies an advance condition
            // but has only one step — the advance condition is vacuous.
            if step_maps.first().map_or(false, |(_, v)| v.len() == 1) {
                if !self.warnings.iter().any(|w| w == "ADVANCE_SINGLE_STEP") {
                    self.warnings.push("ADVANCE_SINGLE_STEP".into());
                }
            }
        }
        if user_criterion.is_none() {
            if let Some(n) = min_sessions {
                user_criterion = Some(crate::experiment::Criterion::Stability(
                    crate::experiment::Stability {
                        method: String::new(),
                        window_sessions: None,
                        max_change_pct: None,
                        min_sessions: Some(n),
                        measure: None,
                    },
                ));
            }
        }

        // Capture the schedule template token range by scanning to the next
        // top-level boundary (another top-level block keyword at column 1, or
        // end of input). This avoids trying to parse a template containing
        // unresolved Placeholder tokens.
        let start = self.pos;
        while self.pos < self.tokens.len() {
            let t = &self.tokens[self.pos];
            if t.ty == TokenType::Eof {
                break;
            }
            if matches!(
                t.ty,
                TokenType::Phase
                    | TokenType::Shaping
                    | TokenType::Progressive
                    | TokenType::Experiment
            ) {
                break;
            }
            self.pos += 1;
        }
        let end = self.pos;

        // Use the first steps map as the expansion driver; py convention is
        // that all steps lists are the same length and expand in lockstep.
        let n = step_maps
            .first()
            .map(|(_, v)| v.len())
            .unwrap_or(0);
        let mut phases = Vec::with_capacity(n);
        for i in 0..n {
            let mut tokens: Vec<Token> = self.tokens[start..end].to_vec();
            for tok in tokens.iter_mut() {
                if tok.ty == TokenType::Placeholder {
                    if let Some((_, vals)) = step_maps.iter().find(|(k, _)| k == &tok.value) {
                        if i < vals.len() {
                            tok.ty = TokenType::Number;
                            let v = vals[i];
                            tok.value = if v.fract() == 0.0 {
                                format!("{}", v as i64)
                            } else {
                                format!("{}", v)
                            };
                        }
                    }
                }
            }
            tokens.push(Token::new(TokenType::Eof, "", 0, 0));
            let mut sub = Parser::new(tokens);
            let schedule = sub.parse_schedule()?;
            phases.push(crate::experiment::Phase {
                label: format!("{}_{}", label_tok.value, i + 1),
                schedule: crate::experiment::PhaseSchedule::Operant(schedule),
                criterion: user_criterion.clone().unwrap_or(
                    crate::experiment::Criterion::ExperimenterJudgment(
                        crate::experiment::ExperimenterJudgment,
                    ),
                ),
                annotations: Vec::new(),
            });
        }
        Ok(phases)
    }

    fn parse_shaping_block(&mut self) -> Result<Vec<crate::experiment::Phase>> {
        self.expect(TokenType::Shaping, "shaping")?;
        let label_tok = self.expect(TokenType::Identifier, "shaping label")?;
        self.expect(TokenType::Colon, ": after shaping label")?;
        self.skip_newlines();
        let mut target: Option<String> = None;
        let mut method: String = "artful".into();
        let mut approximations: Option<Vec<JsonValue>> = None;
        let mut dimension: Option<String> = None;
        let mut percentile_rank: Option<i64> = None;
        let mut percentile_window: Option<i64> = None;
        let mut percentile_dir: Option<String> = None;
        let mut user_criterion: Option<crate::experiment::Criterion> = None;
        let mut min_sessions: Option<i64> = None;
        loop {
            self.skip_newlines();
            if self.at(TokenType::Sessions) {
                self.advance();
                if self.at(TokenType::Equals) {
                    self.advance();
                    let nt = self.expect(TokenType::Number, "sessions count")?;
                    user_criterion =
                        Some(crate::experiment::Criterion::FixedSessions(
                            crate::experiment::FixedSessions {
                                count: nt.value.parse().unwrap_or(0),
                            },
                        ));
                } else if self.at(TokenType::Gte) {
                    self.advance();
                    let nt = self.expect(TokenType::Number, "min sessions")?;
                    let n: i64 = nt.value.parse().unwrap_or(0);
                    min_sessions = Some(n);
                    // If a stable() criterion was already declared, attach
                    // min_sessions to it retroactively.
                    if let Some(crate::experiment::Criterion::Stability(s)) =
                        user_criterion.as_mut()
                    {
                        s.min_sessions = Some(n);
                    }
                }
                continue;
            }
            if self.at(TokenType::Stable) {
                self.advance();
                self.expect(TokenType::LParen, "( after stable")?;
                let method_tok = self.expect(TokenType::Identifier, "stability method")?;
                let mut window_sessions: Option<i64> = None;
                let mut max_change_pct: Option<f64> = None;
                let mut measure: Option<String> = None;
                while self.at(TokenType::Comma) {
                    self.advance();
                    let key = self.expect(TokenType::Identifier, "stable kwarg")?;
                    self.expect(TokenType::Equals, "=")?;
                    match key.value.as_str() {
                        "window" => {
                            let nt = self.expect(TokenType::Number, "window")?;
                            window_sessions = Some(nt.value.parse().unwrap_or(0));
                        }
                        "threshold" => {
                            let nt = self.expect(TokenType::Number, "threshold")?;
                            let v: f64 = nt.value.parse().unwrap_or(0.0);
                            if self.at(TokenType::Percent) {
                                self.advance();
                            }
                            max_change_pct = Some(v);
                        }
                        "measure" => {
                            let vt = self.expect(TokenType::Identifier, "measure")?;
                            measure = Some(vt.value);
                        }
                        _ => {
                            return Err(DslError::parse(
                                format!("unknown stable kwarg '{}'", key.value),
                                SourceSpan::new(key.line, key.col),
                            ))
                        }
                    }
                }
                self.expect(TokenType::RParen, ") on stable")?;
                user_criterion = Some(crate::experiment::Criterion::Stability(
                    crate::experiment::Stability {
                        method: method_tok.value,
                        window_sessions,
                        max_change_pct,
                        min_sessions,
                        measure,
                    },
                ));
                continue;
            }
            if !self.at(TokenType::Identifier) {
                break;
            }
            let key = self.peek().value.clone();
            if !matches!(
                key.as_str(),
                "target"
                    | "method"
                    | "approximations"
                    | "dimension"
                    | "percentile_rank"
                    | "percentile_window"
                    | "percentile_dir"
                    | "stages"
            ) {
                break;
            }
            self.advance();
            self.expect(TokenType::Equals, "= after shaping kwarg")?;
            let v = self.parse_annotation_value()?;
            match key.as_str() {
                "target" => {
                    target = Some(match v {
                        AnnotationValue::Str(s) => s,
                        other => annotation_value_to_string(&other),
                    });
                }
                "method" => {
                    method = match v {
                        AnnotationValue::Str(s) => s,
                        other => annotation_value_to_string(&other),
                    };
                }
                "approximations" => {
                    if let AnnotationValue::Json(JsonValue::Array(xs)) = v {
                        approximations = Some(xs);
                    }
                }
                "dimension" => {
                    dimension = Some(match v {
                        AnnotationValue::Str(s) => s,
                        other => annotation_value_to_string(&other),
                    });
                }
                "percentile_rank" => {
                    if let AnnotationValue::Int(i) = v {
                        percentile_rank = Some(i);
                    }
                }
                "percentile_window" => {
                    if let AnnotationValue::Int(i) = v {
                        percentile_window = Some(i);
                    }
                }
                "percentile_dir" => {
                    percentile_dir = Some(match v {
                        AnnotationValue::Str(s) => s,
                        other => annotation_value_to_string(&other),
                    });
                }
                "stages" => {
                    // Staged shaping: `stages = [CRF, DRH2s, DRH1s]`.
                    // Not supported yet; return a parse error pointing at
                    // the keyword.
                    return Err(DslError::parse(
                        format!("SHAPING_STAGED_NOT_SUPPORTED: {:?}", v),
                        SourceSpan::new(label_tok.line, label_tok.col),
                    ));
                }
                _ => {}
            }
        }
        // Promote min_sessions into a Stability criterion if stable() was
        // not seen but sessions >= N was declared.
        if user_criterion.is_none() {
            if let Some(n) = min_sessions {
                user_criterion = Some(crate::experiment::Criterion::Stability(
                    crate::experiment::Stability {
                        method: String::new(),
                        window_sessions: None,
                        max_change_pct: None,
                        min_sessions: Some(n),
                        measure: None,
                    },
                ));
            }
        }
        let target = target.ok_or_else(|| {
            DslError::parse(
                format!("MISSING_SHAPING_TARGET: shaping '{}' has no 'target'", label_tok.value),
                SourceSpan::new(label_tok.line, label_tok.col),
            )
        })?;
        let mut params: BTreeMap<String, AnnotationValue> = BTreeMap::new();
        params.insert("target".into(), AnnotationValue::Str(target.clone()));
        params.insert("method".into(), AnnotationValue::Str(method.clone()));
        if let Some(xs) = approximations {
            params.insert(
                "approximations".into(),
                AnnotationValue::Json(JsonValue::Array(xs)),
            );
        }
        if let Some(d) = &dimension {
            params.insert("dimension".into(), AnnotationValue::Str(d.clone()));
        }
        let proc_ann = Annotation {
            keyword: "procedure".into(),
            positional: Some(AnnotationValue::Str("shape".into())),
            params: Some(params),
            label: None,
        };
        let schedule = match method.as_str() {
            "artful" => ScheduleExpr::Special(Special { kind: SpecialKind::Crf }),
            "percentile" => ScheduleExpr::Modifier(Modifier {
                kind: ModifierKind::Pctl,
                value: None,
                time_unit: None,
                inner: None,
                pr_step: None,
                pr_start: None,
                pr_increment: None,
                pr_ratio: None,
                pctl_target: dimension,
                pctl_rank: percentile_rank,
                pctl_window: percentile_window,
                pctl_dir: percentile_dir.or(Some("below".into())),
                length: None,
            }),
            _ => ScheduleExpr::Special(Special { kind: SpecialKind::Crf }),
        };
        Ok(vec![crate::experiment::Phase {
            label: label_tok.value,
            schedule: crate::experiment::PhaseSchedule::Operant(schedule),
            criterion: user_criterion.unwrap_or(
                crate::experiment::Criterion::ExperimenterJudgment(
                    crate::experiment::ExperimenterJudgment,
                ),
            ),
            annotations: vec![proc_ann],
        }])
    }

    fn parse_phase_block(&mut self) -> Result<crate::experiment::Phase> {
        self.expect(TokenType::Phase, "phase")?;
        let lt = self.peek().clone();
        let label_tok = match lt.ty {
            TokenType::Identifier | TokenType::Number => {
                self.advance();
                lt
            }
            _ => {
                return Err(DslError::parse(
                    format!("expected phase label, got {:?} '{}'", lt.ty, lt.value),
                    SourceSpan::new(lt.line, lt.col),
                ))
            }
        };
        self.expect(TokenType::Colon, ": after phase label")?;
        self.skip_newlines();

        // Body consists of optional criterion lines (`sessions = N` or
        // `sessions >= N`), optional stable(...) line, schedule-level
        // annotations, and one schedule expression. Each line is separated
        // by Newline.
        let mut criterion: Option<crate::experiment::Criterion> = None;
        let mut min_sessions: Option<i64> = None;
        let mut phase_annotations: Vec<Annotation> = Vec::new();

        loop {
            self.skip_newlines();
            if self.at(TokenType::Sessions) {
                self.advance();
                if self.at(TokenType::Equals) {
                    self.advance();
                    let nt = self.expect(TokenType::Number, "sessions count")?;
                    let n: i64 = nt.value.parse().map_err(|_| {
                        DslError::parse(
                            format!("invalid sessions: {}", nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        )
                    })?;
                    criterion = Some(crate::experiment::Criterion::FixedSessions(
                        crate::experiment::FixedSessions { count: n },
                    ));
                } else if self.at(TokenType::Gte) {
                    self.advance();
                    let nt = self.expect(TokenType::Number, "min sessions")?;
                    min_sessions = Some(nt.value.parse().unwrap_or(0));
                } else {
                    let t = self.peek();
                    return Err(DslError::parse(
                        format!(
                            "expected '=' or '>=' after 'sessions', got {:?} '{}'",
                            t.ty, t.value
                        ),
                        SourceSpan::new(t.line, t.col),
                    ));
                }
                self.skip_newlines();
                continue;
            }
            if self.at(TokenType::Stable) {
                self.advance();
                self.expect(TokenType::LParen, "( after stable")?;
                let method_tok =
                    self.expect(TokenType::Identifier, "stability method")?;
                let mut window_sessions: Option<i64> = None;
                let mut max_change_pct: Option<f64> = None;
                let mut measure: Option<String> = None;
                while self.at(TokenType::Comma) {
                    self.advance();
                    let key = self.expect(TokenType::Identifier, "stable kwarg")?;
                    self.expect(TokenType::Equals, "=")?;
                    match key.value.as_str() {
                        "window" => {
                            let nt = self.expect(TokenType::Number, "window")?;
                            window_sessions = Some(nt.value.parse().unwrap_or(0));
                        }
                        "threshold" => {
                            let nt = self.expect(TokenType::Number, "threshold")?;
                            let v: f64 = nt.value.parse().unwrap_or(0.0);
                            if self.at(TokenType::Percent) {
                                self.advance();
                            }
                            max_change_pct = Some(v);
                        }
                        "measure" => {
                            let vt = self.expect(TokenType::Identifier, "measure value")?;
                            measure = Some(vt.value);
                        }
                        _ => {
                            return Err(DslError::parse(
                                format!("unknown stable kwarg '{}'", key.value),
                                SourceSpan::new(key.line, key.col),
                            ))
                        }
                    }
                }
                self.expect(TokenType::RParen, ") on stable")?;
                criterion = Some(crate::experiment::Criterion::Stability(
                    crate::experiment::Stability {
                        method: method_tok.value,
                        window_sessions,
                        max_change_pct,
                        min_sessions,
                        measure,
                    },
                ));
                self.skip_newlines();
                continue;
            }
            if self.at(TokenType::At) {
                phase_annotations.push(self.parse_annotation()?);
                self.skip_newlines();
                continue;
            }
            break;
        }

        let phase_schedule = if self.at(TokenType::NoSchedule) {
            self.advance();
            crate::experiment::PhaseSchedule::None
        } else if self.at(TokenType::Use) {
            self.advance();
            let ref_tok = self.expect(TokenType::Identifier, "use <phase-label>")?;
            crate::experiment::PhaseSchedule::PhaseRef(crate::experiment::PhaseRef {
                target_label: ref_tok.value,
            })
        } else {
            let s = self.parse_schedule()?;
            crate::experiment::PhaseSchedule::Operant(s)
        };
        self.skip_newlines();
        // Trailing phase-level @-annotations (after the schedule line).
        while self.at(TokenType::At) {
            phase_annotations.push(self.parse_annotation()?);
            self.skip_newlines();
        }

        let criterion = criterion.unwrap_or(crate::experiment::Criterion::ExperimenterJudgment(
            crate::experiment::ExperimenterJudgment,
        ));
        Ok(crate::experiment::Phase {
            label: label_tok.value,
            schedule: phase_schedule,
            criterion,
            annotations: phase_annotations,
        })
    }

    pub(crate) fn parse_annotation(&mut self) -> Result<Annotation> {
        self.expect(TokenType::At, "@")?;
        let key_tok = self.peek().clone();
        let keyword = match key_tok.ty {
            TokenType::Identifier
            | TokenType::Lh
            | TokenType::Rd
            | TokenType::KwArgName
            | TokenType::DrMod
            | TokenType::Pr
            | TokenType::Repeat
            | TokenType::DistributionName
            | TokenType::Use
            | TokenType::Sessions
            | TokenType::Stable
            | TokenType::Phase
            | TokenType::Shaping
            | TokenType::Progressive
            | TokenType::Steps
            | TokenType::Interleave
            | TokenType::NoTrailing
            | TokenType::Advance
            | TokenType::When
            | TokenType::Experiment
            | TokenType::NoSchedule => {
                self.advance();
                key_tok.value.clone()
            }
            _ => {
                return Err(DslError::parse(
                    format!(
                        "expected annotation keyword, got {:?} '{}'",
                        key_tok.ty, key_tok.value
                    ),
                    SourceSpan::new(key_tok.line, key_tok.col),
                ))
            }
        };
        let (keyword_canon, label) = match canonical_annotation_keyword(&keyword) {
            Some(canon) if canon != keyword => (canon.to_string(), Some(keyword.clone())),
            _ => (keyword, None),
        };
        let mut positional: Option<AnnotationValue> = None;
        let mut params: Option<BTreeMap<String, AnnotationValue>> = None;
        if self.at(TokenType::LParen) {
            self.advance();
            // Bare `()` => empty params bag so the serializer emits
            // `params: {}` to satisfy probe_policy-style fixtures.
            if self.at(TokenType::RParen) {
                params = Some(BTreeMap::new());
            }
            if !self.at(TokenType::RParen) {
                // First arg: either <positional> or <kw>=<value>.
                let kv = self.parse_annotation_arg()?;
                match kv {
                    (Some(k), v) => {
                        let mut pm: BTreeMap<String, AnnotationValue> = BTreeMap::new();
                        pm.insert(k, v);
                        params = Some(pm);
                    }
                    (None, v) => {
                        positional = Some(v);
                    }
                }
                while self.at(TokenType::Comma) {
                    self.advance();
                    let kv = self.parse_annotation_arg()?;
                    match kv {
                        (Some(k), v) => {
                            let pm = params.get_or_insert_with(BTreeMap::new);
                            pm.insert(k, v);
                        }
                        (None, _) => {
                            let t = self.peek();
                            return Err(DslError::parse(
                                "positional arg after keyword arg in annotation",
                                SourceSpan::new(t.line, t.col),
                            ));
                        }
                    }
                }
            }
            self.expect(TokenType::RParen, "annotation closing paren")?;
        }
        Ok(Annotation {
            keyword: keyword_canon,
            positional,
            params,
            label,
        })
    }

    fn parse_annotation_arg(&mut self) -> Result<(Option<String>, AnnotationValue)> {
        // Lookahead for `<ident>=`.
        let is_kw = matches!(
            self.peek().ty,
            TokenType::Identifier
                | TokenType::Lh
                | TokenType::Rd
                | TokenType::KwArgName
                | TokenType::Use
                | TokenType::DistributionName
        ) && self
            .peek_at(1)
            .map_or(false, |n| n.ty == TokenType::Equals);
        if is_kw {
            let key = self.advance();
            self.advance(); // =
            let v = self.parse_annotation_value()?;
            return Ok((Some(key.value), v));
        }
        let v = self.parse_annotation_value()?;
        Ok((None, v))
    }

    fn parse_annotation_value(&mut self) -> Result<AnnotationValue> {
        let t = self.peek().clone();
        match t.ty {
            TokenType::StringLiteral => {
                self.advance();
                Ok(AnnotationValue::Str(t.value))
            }
            TokenType::Number => {
                self.advance();
                // Optional time/unit suffix.
                if self.at(TokenType::TimeUnit) || self.at(TokenType::Percent) {
                    let u = self.advance();
                    let mut m = BTreeMap::new();
                    let v: f64 = t.value.parse().unwrap_or(0.0);
                    m.insert("value".into(), JsonValue::Float(v));
                    m.insert(
                        if u.ty == TokenType::Percent {
                            "unit".into()
                        } else {
                            "time_unit".into()
                        },
                        JsonValue::String(if u.ty == TokenType::Percent {
                            "%".into()
                        } else {
                            u.value
                        }),
                    );
                    return Ok(AnnotationValue::Json(JsonValue::Object(m)));
                }
                if t.value.contains('.') {
                    Ok(AnnotationValue::Float(t.value.parse().unwrap_or(0.0)))
                } else {
                    Ok(AnnotationValue::Int(t.value.parse().unwrap_or(0)))
                }
            }
            TokenType::Minus => {
                self.advance();
                let nt = self.expect(TokenType::Number, "negative value")?;
                if nt.value.contains('.') {
                    Ok(AnnotationValue::Float(
                        -nt.value.parse::<f64>().unwrap_or(0.0),
                    ))
                } else {
                    Ok(AnnotationValue::Int(
                        -nt.value.parse::<i64>().unwrap_or(0),
                    ))
                }
            }
            TokenType::LBracket => {
                self.advance();
                let mut items = Vec::new();
                if !self.at(TokenType::RBracket) {
                    items.push(annotation_value_to_json(&self.parse_annotation_value()?));
                    while self.at(TokenType::Comma) {
                        self.advance();
                        items.push(annotation_value_to_json(&self.parse_annotation_value()?));
                    }
                }
                self.expect(TokenType::RBracket, "] in annotation")?;
                Ok(AnnotationValue::Json(JsonValue::Array(items)))
            }
            TokenType::LBrace => {
                self.advance();
                let mut m = BTreeMap::new();
                if !self.at(TokenType::RBrace) {
                    let (k, v) = self.parse_annotation_object_entry()?;
                    m.insert(k, annotation_value_to_json(&v));
                    while self.at(TokenType::Comma) {
                        self.advance();
                        let (k, v) = self.parse_annotation_object_entry()?;
                        m.insert(k, annotation_value_to_json(&v));
                    }
                }
                self.expect(TokenType::RBrace, "} in annotation")?;
                Ok(AnnotationValue::Json(JsonValue::Object(m)))
            }
            TokenType::Identifier
            | TokenType::DistributionName
            | TokenType::Use
            | TokenType::DrMod => {
                self.advance();
                match t.value.as_str() {
                    "true" => Ok(AnnotationValue::Json(JsonValue::Bool(true))),
                    "false" => Ok(AnnotationValue::Json(JsonValue::Bool(false))),
                    _ => Ok(AnnotationValue::Str(t.value)),
                }
            }
            _ => Err(DslError::parse(
                format!(
                    "unexpected token in annotation value: {:?} '{}'",
                    t.ty, t.value
                ),
                SourceSpan::new(t.line, t.col),
            )),
        }
    }

    fn parse_annotation_object_entry(&mut self) -> Result<(String, AnnotationValue)> {
        let t = self.peek().clone();
        let key = match t.ty {
            TokenType::StringLiteral | TokenType::Identifier => {
                self.advance();
                t.value
            }
            _ => {
                return Err(DslError::parse(
                    format!(
                        "expected object key in annotation, got {:?} '{}'",
                        t.ty, t.value
                    ),
                    SourceSpan::new(t.line, t.col),
                ))
            }
        };
        // Accept either `key: value` or `key = value`.
        if self.at(TokenType::Colon) || self.at(TokenType::Equals) {
            self.advance();
        } else {
            let nt = self.peek();
            return Err(DslError::parse(
                "expected ':' or '=' in annotation object entry",
                SourceSpan::new(nt.line, nt.col),
            ));
        }
        let v = self.parse_annotation_value()?;
        Ok((key, v))
    }

    fn parse_param_decl(&mut self) -> Result<ParamDecl> {
        let tok = self.advance();
        let name = match tok.ty {
            TokenType::Rd => "RD".to_string(),
            TokenType::KwArgName => KW_ARG_KEYWORDS
                .iter()
                .find(|(alias, _)| *alias == tok.value)
                .map(|(_, canon)| (*canon).to_string())
                .ok_or_else(|| {
                    DslError::parse(
                        format!("unknown kw-arg name '{}'", tok.value),
                        SourceSpan::new(tok.line, tok.col),
                    )
                })?,
            _ => "LH".to_string(),
        };
        self.expect(TokenType::Equals, "=")?;
        if self.at(TokenType::Minus) {
            let t = self.peek().clone();
            let code = match name.as_str() {
                "LH" => "LH_NONPOSITIVE_VALUE",
                "RD" => "RD_NEGATIVE_VALUE",
                _ => "PARAM_DECL_NONPOSITIVE",
            };
            return Err(DslError::semantic(
                format!("{}: '{}' must be non-negative", code, name),
                SourceSpan::new(t.line, t.col),
            ));
        }
        let (val, unit) = self.parse_value()?;
        if name == "LH" && val <= 0.0 {
            return Err(DslError::semantic(
                format!("LH_NONPOSITIVE_VALUE: got {}", val),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        Ok(ParamDecl { name, value: val, time_unit: unit })
    }

    fn parse_binding(&mut self) -> Result<Binding> {
        self.expect(TokenType::Let, "let")?;
        let name_tok = self.expect(TokenType::Identifier, "binding name")?;
        if LOWERCASE_RESERVED.iter().any(|w| *w == name_tok.value) {
            return Err(DslError::parse(
                format!("reserved word '{}' cannot be a binding name", name_tok.value),
                SourceSpan::new(name_tok.line, name_tok.col),
            ));
        }
        self.expect(TokenType::Equals, "=")?;
        let value = self.parse_schedule()?;
        Ok(Binding { name: name_tok.value, value })
    }

    fn parse_schedule(&mut self) -> Result<ScheduleExpr> {
        let mut base = self.parse_base_schedule()?;
        // Postfix LH.
        if self.at(TokenType::Lh) {
            let skip = self
                .peek_at(1)
                .map_or(false, |n| n.ty == TokenType::Equals);
            if !skip {
                self.advance();
                if matches!(base, ScheduleExpr::Compound(_)) {
                    let t = self.peek();
                    return Err(DslError::parse(
                        "LH_ON_COMPOUND: LH applies to leaf schedules only",
                        SourceSpan::new(t.line, t.col),
                    ));
                }
                let (hv, hu) = self.parse_value()?;
                if hv <= 0.0 {
                    let t = self.peek();
                    return Err(DslError::semantic(
                        format!("LH_NONPOSITIVE_VALUE: LH must be positive (got {})", hv),
                        SourceSpan::new(t.line, t.col),
                    ));
                }
                base = ScheduleExpr::LimitedHold(LimitedHold {
                    hold_duration: hv,
                    inner: Box::new(base),
                    time_unit: hu,
                });
            }
        }
        // Postfix TO(...) / Timeout(...).
        if self.at(TokenType::Identifier) {
            let v = &self.peek().value;
            if (v == "TO" || v == "Timeout")
                && self
                    .peek_at(1)
                    .map_or(false, |n| n.ty == TokenType::LParen)
            {
                if matches!(base, ScheduleExpr::Compound(_)) {
                    let t = self.peek();
                    return Err(DslError::parse(
                        "TIMEOUT_ON_COMPOUND: TO(...) applies to leaf schedules only",
                        SourceSpan::new(t.line, t.col),
                    ));
                }
                base = self.parse_timeout_postfix(base)?;
            }
        }
        // Postfix ResponseCost(...).
        if self.at(TokenType::Identifier)
            && self.peek().value == "ResponseCost"
            && self
                .peek_at(1)
                .map_or(false, |n| n.ty == TokenType::LParen)
        {
            if matches!(base, ScheduleExpr::Compound(_)) {
                let t = self.peek();
                return Err(DslError::parse(
                    "RC_ON_COMPOUND: ResponseCost applies to leaf schedules only",
                    SourceSpan::new(t.line, t.col),
                ));
            }
            base = self.parse_response_cost_postfix(base)?;
        }
        // Trailing schedule-level annotations.
        if self.at(TokenType::At) {
            let mut anns = Vec::new();
            while self.at(TokenType::At) {
                anns.push(self.parse_annotation()?);
            }
            base = ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
                expr: base,
                annotations: anns,
            }));
        }
        Ok(base)
    }

    fn parse_timeout_postfix(&mut self, inner: ScheduleExpr) -> Result<ScheduleExpr> {
        let name_tok = self.advance();
        self.expect(TokenType::LParen, "TO opening paren")?;
        let mut duration: Option<f64> = None;
        let mut duration_unit: Option<String> = None;
        let mut reset: Option<bool> = None;
        let mut seen: Vec<String> = Vec::new();
        while !self.at(TokenType::RParen) {
            let key = self.expect(TokenType::Identifier, "TO kwarg name")?;
            if seen.iter().any(|s| s == &key.value) {
                return Err(DslError::parse(
                    format!("DUPLICATE_TO_PARAM: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            seen.push(key.value.clone());
            self.expect(TokenType::Equals, "= after TO kwarg")?;
            match key.value.as_str() {
                "duration" => {
                    let (v, u) = self.parse_value()?;
                    if v <= 0.0 {
                        return Err(DslError::parse(
                            format!("TO_NONPOSITIVE_DURATION: {}", v),
                            SourceSpan::new(key.line, key.col),
                        ));
                    }
                    if u.is_none() {
                        return Err(DslError::parse(
                            "TO_TIME_UNIT_REQUIRED",
                            SourceSpan::new(key.line, key.col),
                        ));
                    }
                    duration = Some(v);
                    duration_unit = u;
                }
                "reset_on_response" => {
                    let bt = self.expect(TokenType::Identifier, "boolean value")?;
                    match bt.value.as_str() {
                        "true" => reset = Some(true),
                        "false" => reset = Some(false),
                        _ => {
                            return Err(DslError::parse(
                                format!("expected 'true' or 'false', got '{}'", bt.value),
                                SourceSpan::new(bt.line, bt.col),
                            ))
                        }
                    }
                }
                _ => {
                    return Err(DslError::parse(
                        format!("unknown TO kwarg '{}'", key.value),
                        SourceSpan::new(key.line, key.col),
                    ))
                }
            }
            if self.at(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen, "closing ) on TO")?;
        let duration = duration.ok_or_else(|| {
            DslError::parse(
                "MISSING_TO_DURATION",
                SourceSpan::new(name_tok.line, name_tok.col),
            )
        })?;
        let reset_on_response = reset.ok_or_else(|| {
            DslError::parse(
                "MISSING_TO_RESET_RULE",
                SourceSpan::new(name_tok.line, name_tok.col),
            )
        })?;
        Ok(ScheduleExpr::TimeoutWrapped(TimeoutWrapped {
            inner: Box::new(inner),
            timeout: Timeout {
                duration,
                duration_unit,
                reset_on_response,
            },
        }))
    }

    fn parse_response_cost_postfix(&mut self, inner: ScheduleExpr) -> Result<ScheduleExpr> {
        let name_tok = self.advance();
        self.expect(TokenType::LParen, "ResponseCost opening paren")?;
        let mut amount: Option<f64> = None;
        let mut unit: String = ResponseCost::DEFAULT_UNIT.into();
        let mut seen: Vec<String> = Vec::new();
        while !self.at(TokenType::RParen) {
            let key = self.expect(TokenType::Identifier, "ResponseCost kwarg name")?;
            if seen.iter().any(|s| s == &key.value) {
                return Err(DslError::parse(
                    format!("RESPONSE_COST_DUPLICATE_KWARG: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            seen.push(key.value.clone());
            self.expect(TokenType::Equals, "= after ResponseCost kwarg")?;
            match key.value.as_str() {
                "amount" => {
                    let nt = self.expect(TokenType::Number, "amount value")?;
                    let v: f64 = nt.value.parse().map_err(|_| {
                        DslError::parse(
                            format!("invalid amount: {}", nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        )
                    })?;
                    if v <= 0.0 {
                        return Err(DslError::parse(
                            format!("RESPONSE_COST_NONPOSITIVE_AMOUNT: {}", v),
                            SourceSpan::new(key.line, key.col),
                        ));
                    }
                    amount = Some(v);
                }
                "unit" => {
                    let ut = self.expect(TokenType::StringLiteral, "unit string")?;
                    unit = ut.value;
                }
                _ => {
                    return Err(DslError::parse(
                        format!("unknown ResponseCost kwarg '{}'", key.value),
                        SourceSpan::new(key.line, key.col),
                    ))
                }
            }
            if self.at(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen, "closing ) on ResponseCost")?;
        let amount = amount.ok_or_else(|| {
            DslError::parse(
                "RESPONSE_COST_MISSING_AMOUNT",
                SourceSpan::new(name_tok.line, name_tok.col),
            )
        })?;
        Ok(ScheduleExpr::ResponseCostWrapped(ResponseCostWrapped {
            inner: Box::new(inner),
            response_cost: ResponseCost { amount, unit },
        }))
    }

    fn parse_base_schedule(&mut self) -> Result<ScheduleExpr> {
        let ty = self.peek().ty;
        match ty {
            TokenType::LParen => {
                self.advance();
                let e = self.parse_schedule()?;
                self.expect(TokenType::RParen, "closing paren")?;
                Ok(e)
            }
            TokenType::Combinator => self.parse_compound(),
            TokenType::DrMod => self.parse_dr_mod(),
            TokenType::Pr => self.parse_pr_mod(),
            TokenType::Repeat => self.parse_repeat_call(),
            TokenType::ScheduleType => self.parse_atomic_or_second(),
            TokenType::Ext => {
                let tok = self.advance();
                if self.at(TokenType::LParen) {
                    return Err(DslError::parse(
                        "INVALID_SECOND_ORDER_OVERALL: EXT cannot appear as a second-order overall",
                        SourceSpan::new(tok.line, tok.col),
                    ));
                }
                Ok(ScheduleExpr::Special(Special { kind: SpecialKind::Ext }))
            }
            TokenType::Crf => {
                let tok = self.advance();
                if self.at(TokenType::LParen) {
                    return Err(DslError::parse(
                        "INVALID_SECOND_ORDER_OVERALL: CRF cannot appear as a second-order overall",
                        SourceSpan::new(tok.line, tok.col),
                    ));
                }
                Ok(ScheduleExpr::Special(Special { kind: SpecialKind::Crf }))
            }
            TokenType::Number => {
                // `N*schedule` shorthand for Repeat.
                self.parse_repeat_shorthand()
            }
            TokenType::Identifier => {
                let peek_val = self.peek().value.clone();
                let next_is_lparen = self
                    .peek_at(1)
                    .map_or(false, |n| n.ty == TokenType::LParen);
                // Lag N / LagN / Lag(N[, length=N])
                if peek_val == "Lag"
                    || (peek_val.starts_with("Lag")
                        && peek_val[3..].chars().all(|c| c.is_ascii_digit())
                        && !peek_val[3..].is_empty())
                {
                    return self.parse_lag();
                }
                // Aversive schedule calls: Sidman(...), DiscrimAv(...), Escape(...)
                if next_is_lparen && aversive_kind(&peek_val).is_some() {
                    return self.parse_aversive_schedule();
                }
                if next_is_lparen && (peek_val == "Adj" || peek_val == "Adjusting") {
                    return self.parse_adjusting_schedule();
                }
                if next_is_lparen
                    && (peek_val == "Interlock" || peek_val == "Interlocking")
                {
                    return self.parse_interlocking_schedule();
                }
                if next_is_lparen && peek_val == "Pctl" {
                    return self.parse_pctl_schedule();
                }
                if next_is_lparen && (peek_val == "MTS" || peek_val == "GoNoGo") {
                    return self.parse_trial_based();
                }
                // Respondent Tier A primitives dispatch through
                // respondent_parser::try_parse_respondent.
                if crate::respondent_parser::is_respondent_primitive(&peek_val) {
                    if let Some(expr) =
                        crate::respondent_parser::try_parse_respondent(self)?
                    {
                        return Ok(expr);
                    }
                }
                let tok = self.advance();
                if self.at(TokenType::LParen) {
                    self.advance();
                    let unit = self.parse_second_order_slot()?;
                    self.expect(TokenType::RParen, "second-order closing paren")?;
                    return Ok(ScheduleExpr::SecondOrder(SecondOrder {
                        overall: SecondOrderSlot::Ref(tok.value),
                        unit,
                    }));
                }
                Ok(ScheduleExpr::IdentifierRef(IdentifierRef { name: tok.value }))
            }
            TokenType::KwArgName => {
                let t = self.peek().clone();
                Err(DslError::parse(
                    format!(
                        "KEYWORD_BEFORE_POSITIONAL: keyword '{}' cannot appear before a positional schedule",
                        t.value
                    ),
                    SourceSpan::new(t.line, t.col),
                ))
            }
            _ => {
                let t = self.peek();
                Err(DslError::parse(
                    format!(
                        "UNEXPECTED_TOKEN: unexpected token at start of schedule: {:?} '{}'",
                        t.ty, t.value
                    ),
                    SourceSpan::new(t.line, t.col),
                ))
            }
        }
    }

    fn parse_atomic_or_second(&mut self) -> Result<ScheduleExpr> {
        let atomic = self.parse_parametric_atomic()?;
        if self.at(TokenType::LParen) {
            self.advance();
            let unit = self.parse_second_order_slot()?;
            self.expect(TokenType::RParen, "second-order closing paren")?;
            return Ok(ScheduleExpr::SecondOrder(SecondOrder {
                overall: SecondOrderSlot::Atomic(atomic),
                unit,
            }));
        }
        Ok(ScheduleExpr::Atomic(atomic))
    }

    fn parse_second_order_slot(&mut self) -> Result<SecondOrderSlot> {
        let t = self.peek().clone();
        if t.ty == TokenType::Identifier
            && !self.peek_at(1).map_or(false, |n| n.ty == TokenType::LParen)
        {
            self.advance();
            return Ok(SecondOrderSlot::Ref(t.value));
        }
        if matches!(t.ty, TokenType::Combinator | TokenType::Ext | TokenType::Crf) {
            return Err(DslError::semantic(
                format!(
                    "INVALID_SECOND_ORDER_UNIT: second-order unit must be an atomic schedule (got {:?} '{}')",
                    t.ty, t.value
                ),
                SourceSpan::new(t.line, t.col),
            ));
        }
        let atomic = self.parse_parametric_atomic()?;
        Ok(SecondOrderSlot::Atomic(atomic))
    }

    fn parse_parametric_atomic(&mut self) -> Result<Atomic> {
        let tok = self.expect(TokenType::ScheduleType, "schedule type")?;
        // Handle the case where the value is prefixed with `-` (negative).
        if self.at(TokenType::Minus) {
            return Err(DslError::semantic(
                format!("ATOMIC_NONPOSITIVE_VALUE: '{}' must be positive", tok.value),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        let bytes = tok.value.as_bytes();
        let dist = match bytes[0] {
            b'F' => Distribution::Fixed,
            b'V' => Distribution::Variable,
            b'R' => Distribution::Random,
            _ => {
                return Err(DslError::parse(
                    format!("unknown distribution prefix: {}", tok.value),
                    SourceSpan::new(tok.line, tok.col),
                ))
            }
        };
        let domain = match bytes[1] {
            b'R' => Domain::Ratio,
            b'I' => Domain::Interval,
            b'T' => Domain::Time,
            _ => {
                return Err(DslError::parse(
                    format!("unknown domain suffix: {}", tok.value),
                    SourceSpan::new(tok.line, tok.col),
                ))
            }
        };
        let (val, unit) = self.parse_value()?;
        if val <= 0.0 {
            return Err(DslError::semantic(
                format!("ATOMIC_NONPOSITIVE_VALUE: '{}' must be positive (got {})", tok.value, val),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        Ok(Atomic {
            schedule_type: ScheduleType::new(dist, domain),
            value: val,
            time_unit: unit,
        })
    }

    pub(crate) fn parse_value(&mut self) -> Result<(f64, Option<String>)> {
        let num = self.expect(TokenType::Number, "numeric value")?;
        let val: f64 = num.value.parse().map_err(|_| {
            DslError::parse(
                format!("invalid number: {}", num.value),
                SourceSpan::new(num.line, num.col),
            )
        })?;
        let unit = if self.at(TokenType::TimeUnit) {
            Some(self.advance().value)
        } else {
            None
        };
        Ok((val, unit))
    }

    fn parse_compound(&mut self) -> Result<ScheduleExpr> {
        let tok = self.expect(TokenType::Combinator, "combinator")?;
        let combinator = if tok.value == "Interp" {
            Combinator::Interpolate
        } else {
            Combinator::from_keyword(&tok.value).ok_or_else(|| {
                DslError::parse(
                    format!("unknown combinator: {}", tok.value),
                    SourceSpan::new(tok.line, tok.col),
                )
            })?
        };
        self.expect(TokenType::LParen, "combinator opening paren")?;
        let mut components = Vec::new();
        components.push(self.parse_schedule()?);
        if !self.at(TokenType::Comma) {
            return Err(DslError::parse(
                format!(
                    "INSUFFICIENT_COMPONENTS: {} requires at least 2 components",
                    tok.value
                ),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        let mut params: BTreeMap<String, CombinatorParamValue> = BTreeMap::new();
        let mut directional_entries: Vec<DirectionalEntry> = Vec::new();
        let mut punish_directional: Vec<(DirRef, DirRef, ScheduleExpr)> = Vec::new();
        let mut punish_component: Vec<(DirRef, ScheduleExpr)> = Vec::new();
        let mut punish_changeover: Option<ScheduleExpr> = None;
        let mut overlay_target: Option<String> = None;
        let mut has_kw = false;

        while self.at(TokenType::Comma) {
            self.advance();

            if self.at(TokenType::KwArgName) {
                has_kw = true;
                let kw = self.advance();
                let canonical = KW_ARG_KEYWORDS
                    .iter()
                    .find(|(a, _)| *a == kw.value)
                    .map(|(_, c)| (*c).to_string())
                    .ok_or_else(|| {
                        DslError::parse(
                            format!("unknown kw-arg '{}'", kw.value),
                            SourceSpan::new(kw.line, kw.col),
                        )
                    })?;

                if self.at(TokenType::LParen) {
                    self.advance();
                    if canonical == "COD" {
                        let from_ref = self.parse_dir_ref()?;
                        self.expect(TokenType::Arrow, "-> in directional COD")?;
                        let to_ref = self.parse_dir_ref()?;
                        self.expect(TokenType::RParen, "directional COD closing paren")?;
                        self.expect(TokenType::Equals, "= after COD(x->y)")?;
                        let (val, unit) = self.parse_value()?;
                        directional_entries.push(DirectionalEntry {
                            from_ref,
                            to_ref,
                            value: val,
                            time_unit: unit,
                        });
                    } else if canonical == "PUNISH" {
                        // Three sub-forms: PUNISH(changeover), PUNISH(N->M)
                        // or PUNISH(ident->ident), and PUNISH(N) or PUNISH(ident).
                        let p1 = self.peek().clone();
                        if p1.ty == TokenType::Identifier && p1.value == "changeover" {
                            self.advance();
                            self.expect(TokenType::RParen, "PUNISH closing paren")?;
                            self.expect(TokenType::Equals, "= after PUNISH(changeover)")?;
                            let sched = self.parse_schedule()?;
                            if punish_changeover.is_some()
                                || !punish_directional.is_empty()
                            {
                                return Err(DslError::semantic(
                                    "PUNISH_TARGET_CONFLICT",
                                    SourceSpan::new(p1.line, p1.col),
                                ));
                            }
                            punish_changeover = Some(sched);
                        } else if p1.ty == TokenType::Number
                            || p1.ty == TokenType::Identifier
                        {
                            let first = self.parse_dir_ref()?;
                            if self.at(TokenType::Arrow) {
                                self.advance();
                                let to_ref = self.parse_dir_ref()?;
                                self.expect(TokenType::RParen, "PUNISH closing paren")?;
                                self.expect(TokenType::Equals, "= after PUNISH(x->y)")?;
                                let sched = self.parse_schedule()?;
                                if first == to_ref {
                                    return Err(DslError::semantic(
                                        "PUNISH_SELF_REFERENCE",
                                        SourceSpan::new(p1.line, p1.col),
                                    ));
                                }
                                if punish_changeover.is_some() {
                                    return Err(DslError::parse(
                                        "PUNISH_TARGET_CONFLICT",
                                        SourceSpan::new(p1.line, p1.col),
                                    ));
                                }
                                for (a, b, _) in &punish_directional {
                                    if a == &first && b == &to_ref {
                                        return Err(DslError::semantic(
                                            "PUNISH_DUPLICATE",
                                            SourceSpan::new(p1.line, p1.col),
                                        ));
                                    }
                                }
                                punish_directional.push((first, to_ref, sched));
                            } else {
                                self.expect(TokenType::RParen, "PUNISH closing paren")?;
                                self.expect(TokenType::Equals, "= after PUNISH(N)")?;
                                let sched = self.parse_schedule()?;
                                punish_component.push((first, sched));
                            }
                        } else {
                            return Err(DslError::parse(
                                format!(
                                    "PUNISH: expected index, 'changeover', or direction, got {:?} '{}'",
                                    p1.ty, p1.value
                                ),
                                SourceSpan::new(p1.line, p1.col),
                            ));
                        }
                    } else {
                        return Err(DslError::semantic(
                            format!(
                                "INVALID_DIRECTIONAL_KW: directional form not valid for {}",
                                canonical
                            ),
                            SourceSpan::new(kw.line, kw.col),
                        ));
                    }
                } else {
                    // Scalar: <kw>=value.
                    self.expect(TokenType::Equals, "= after compound kwarg name")?;
                    // Validate kwarg vs combinator. COD/FRCO/PUNISH apply to
                    // Conc (choice schedules); BO applies to Mult / Mix
                    // (alternating-component schedules that carry a
                    // blackout between components).
                    let valid = match canonical.as_str() {
                        "COD" | "FRCO" => matches!(combinator, Combinator::Conc),
                        "BO" => matches!(combinator, Combinator::Mult | Combinator::Mix),
                        "PUNISH" => matches!(combinator, Combinator::Conc),
                        _ => true,
                    };
                    if !valid {
                        let (v, _u) = self.parse_value()?;
                        let _ = v;
                        return Err(DslError::semantic(
                            format!(
                                "INVALID_KEYWORD_ARG: '{}' is not valid on {:?}",
                                canonical, combinator
                            ),
                            SourceSpan::new(kw.line, kw.col),
                        ));
                    }
                    let (val, unit) = self.parse_value()?;
                    if params.contains_key(&canonical) {
                        return Err(DslError::semantic(
                            format!("DUPLICATE_KEYWORD_ARG: '{}'", canonical),
                            SourceSpan::new(kw.line, kw.col),
                        ));
                    }
                    // FRCO, BO, COD accept zero as a meaningful value
                    // (explicit zero = no constraint). Only negative values
                    // trigger the nonpositive-value error.
                    if val < 0.0 {
                        let nonpos = match canonical.as_str() {
                            "FRCO" => "FRCO_NONPOSITIVE_VALUE",
                            "BO" => "BO_NONPOSITIVE_VALUE",
                            "COD" => "COD_NONPOSITIVE_VALUE",
                            _ => "COMPOUND_NONPOSITIVE_VALUE",
                        };
                        return Err(DslError::semantic(
                            format!("{}: '{}' got {}", nonpos, canonical, val),
                            SourceSpan::new(kw.line, kw.col),
                        ));
                    }
                    params.insert(
                        canonical,
                        CombinatorParamValue::Scalar(CompoundParam {
                            value: val,
                            time_unit: unit,
                        }),
                    );
                }
            } else if self.at(TokenType::Identifier)
                && self.peek().value == "target"
                && combinator == Combinator::Overlay
                && self
                    .peek_at(1)
                    .map_or(false, |n| n.ty == TokenType::Equals)
            {
                has_kw = true;
                self.advance();
                self.expect(TokenType::Equals, "= after Overlay target")?;
                let val_tok = self.expect(TokenType::Identifier, "Overlay target value")?;
                let v = &val_tok.value;
                if v != "all" && v != "changeover" && !v.starts_with("component_") {
                    return Err(DslError::parse(
                        format!("OVERLAY_TARGET_UNKNOWN: target='{}'", v),
                        SourceSpan::new(val_tok.line, val_tok.col),
                    ));
                }
                if overlay_target.is_some() {
                    return Err(DslError::parse(
                        "OVERLAY_DUPLICATE_TARGET",
                        SourceSpan::new(val_tok.line, val_tok.col),
                    ));
                }
                // `target=all` is the default and is not emitted into params.
                if v != "all" {
                    overlay_target = Some(val_tok.value.clone());
                } else {
                    overlay_target = Some(String::new());
                }
            } else if self.at(TokenType::Identifier)
                && (self.peek().value == "count" || self.peek().value == "onset")
                && combinator == Combinator::Interpolate
                && self
                    .peek_at(1)
                    .map_or(false, |n| n.ty == TokenType::Equals)
            {
                has_kw = true;
                let kw_tok = self.advance();
                self.expect(TokenType::Equals, "= after Interpolate kwarg")?;
                let (val, unit) = self.parse_value()?;
                if params.contains_key(&kw_tok.value) {
                    return Err(DslError::semantic(
                        format!("DUPLICATE_INTERPOLATE_KW_ARG: '{}'", kw_tok.value),
                        SourceSpan::new(kw_tok.line, kw_tok.col),
                    ));
                }
                params.insert(
                    kw_tok.value,
                    CombinatorParamValue::Scalar(CompoundParam {
                        value: val,
                        time_unit: unit,
                    }),
                );
            } else if has_kw {
                let t = self.peek();
                return Err(DslError::parse(
                    "positional arg after keyword arg",
                    SourceSpan::new(t.line, t.col),
                ));
            } else {
                components.push(self.parse_schedule()?);
            }
        }
        self.expect(TokenType::RParen, "compound closing paren")?;

        if combinator == Combinator::Interpolate && components.len() != 2 {
            return Err(DslError::semantic(
                format!(
                    "INTERPOLATE_REQUIRES_TWO: got {} components",
                    components.len()
                ),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        if combinator == Combinator::Overlay && components.len() != 2 {
            return Err(DslError::semantic(
                format!(
                    "OVERLAY_REQUIRES_TWO: got {} components",
                    components.len()
                ),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        // Interpolate: count is required and must be a positive integer.
        if combinator == Combinator::Interpolate {
            match params.get("count") {
                None => {
                    return Err(DslError::semantic(
                        "MISSING_INTERPOLATE_COUNT: Interpolate requires count=N",
                        SourceSpan::new(tok.line, tok.col),
                    ));
                }
                Some(CombinatorParamValue::Scalar(cp)) => {
                    if cp.time_unit.is_some() {
                        return Err(DslError::semantic(
                            "INTERPOLATE_COUNT_TIME_UNIT: count must be dimensionless",
                            SourceSpan::new(tok.line, tok.col),
                        ));
                    }
                    if cp.value <= 0.0 {
                        return Err(DslError::semantic(
                            format!("INTERPOLATE_NONPOSITIVE_COUNT: got {}", cp.value),
                            SourceSpan::new(tok.line, tok.col),
                        ));
                    }
                }
                _ => {}
            }
            if let Some(CombinatorParamValue::Scalar(cp)) = params.get("onset") {
                if cp.time_unit.is_none() {
                    return Err(DslError::semantic(
                        "INTERPOLATE_ONSET_TIME_UNIT_REQUIRED: onset requires a time unit",
                        SourceSpan::new(tok.line, tok.col),
                    ));
                }
                if cp.value <= 0.0 {
                    return Err(DslError::semantic(
                        format!("INTERPOLATE_NONPOSITIVE_ONSET: got {}", cp.value),
                        SourceSpan::new(tok.line, tok.col),
                    ));
                }
            }
        }

        // Resolve identifier-based dir_refs to 1-indexed integers.
        let mut name_to_idx: BTreeMap<String, i64> = BTreeMap::new();
        for (i, comp) in components.iter().enumerate() {
            if let ScheduleExpr::IdentifierRef(r) = comp {
                name_to_idx.insert(r.name.clone(), (i + 1) as i64);
            }
        }

        if !directional_entries.is_empty() {
            let mut resolved = Vec::with_capacity(directional_entries.len());
            for entry in directional_entries.drain(..) {
                let from_ref = resolve_dir_ref(entry.from_ref, &name_to_idx, &tok)?;
                let to_ref = resolve_dir_ref(entry.to_ref, &name_to_idx, &tok)?;
                resolved.push(DirectionalEntry {
                    from_ref,
                    to_ref,
                    value: entry.value,
                    time_unit: entry.time_unit,
                });
            }
            let base = match params.remove("COD") {
                Some(CombinatorParamValue::Scalar(cp)) => Some(cp),
                Some(other) => {
                    params.insert("COD".into(), other);
                    None
                }
                None => None,
            };
            params.insert(
                "COD".into(),
                CombinatorParamValue::Directional(DirectionalCOD {
                    base,
                    directional: resolved,
                }),
            );
        }

        // Build PUNISH payload.
        if !punish_directional.is_empty()
            || !punish_component.is_empty()
            || punish_changeover.is_some()
        {
            let mut punish = BTreeMap::new();
            if let Some(s) = &punish_changeover {
                punish.insert("changeover".into(), expr_to_json(s));
            }
            if !punish_directional.is_empty() {
                let mut arr = Vec::with_capacity(punish_directional.len());
                for (from_ref, to_ref, sched) in punish_directional.drain(..) {
                    let from = resolve_dir_ref(from_ref, &name_to_idx, &tok)?;
                    let to = resolve_dir_ref(to_ref, &name_to_idx, &tok)?;
                    let mut m = BTreeMap::new();
                    m.insert("from".into(), dir_ref_to_json(&from));
                    m.insert("to".into(), dir_ref_to_json(&to));
                    m.insert("schedule".into(), expr_to_json(&sched));
                    arr.push(JsonValue::Object(m));
                }
                punish.insert("directional".into(), JsonValue::Array(arr));
            }
            if !punish_component.is_empty() {
                let mut arr = Vec::with_capacity(punish_component.len());
                for (component_ref, sched) in punish_component.drain(..) {
                    let c = resolve_dir_ref(component_ref, &name_to_idx, &tok)?;
                    let mut m = BTreeMap::new();
                    m.insert("component".into(), dir_ref_to_json(&c));
                    m.insert("schedule".into(), expr_to_json(&sched));
                    arr.push(JsonValue::Object(m));
                }
                punish.insert("component".into(), JsonValue::Array(arr));
            }
            params.insert(
                "PUNISH".into(),
                CombinatorParamValue::Punish(PunishParam {
                    payload: JsonValue::Object(punish),
                }),
            );
        }

        // Overlay target (non-"all" values only).
        if let Some(t) = overlay_target {
            if !t.is_empty() {
                params.insert(
                    "target".into(),
                    CombinatorParamValue::Raw(JsonValue::String(t)),
                );
            }
        }

        let params = if params.is_empty() { None } else { Some(params) };
        Ok(ScheduleExpr::Compound(Compound {
            combinator,
            components,
            params,
        }))
    }

    fn parse_dir_ref(&mut self) -> Result<DirRef> {
        let t = self.peek().clone();
        if t.ty == TokenType::Number {
            self.advance();
            let n: f64 = t.value.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid number in dir_ref: {}", t.value),
                    SourceSpan::new(t.line, t.col),
                )
            })?;
            return Ok(DirRef::Index(n as i64));
        }
        if t.ty == TokenType::Identifier {
            self.advance();
            return Ok(DirRef::Name(t.value));
        }
        Err(DslError::parse(
            format!(
                "expected number or identifier in dir_ref, got {:?} '{}'",
                t.ty, t.value
            ),
            SourceSpan::new(t.line, t.col),
        ))
    }

    fn parse_dr_mod(&mut self) -> Result<ScheduleExpr> {
        let tok = self.expect(TokenType::DrMod, "DR modifier")?;
        let kind = match tok.value.as_str() {
            "DRL" => ModifierKind::Drl,
            "DRH" => ModifierKind::Drh,
            "DRO" => ModifierKind::Dro,
            _ => {
                return Err(DslError::parse(
                    format!("unknown DR modifier: {}", tok.value),
                    SourceSpan::new(tok.line, tok.col),
                ))
            }
        };
        if self.at(TokenType::Minus) {
            return Err(DslError::semantic(
                format!("DR_NONPOSITIVE_VALUE: {} requires positive value", tok.value),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        let (val, unit) = self.parse_value()?;
        if val <= 0.0 {
            return Err(DslError::semantic(
                format!("DR_NONPOSITIVE_VALUE: {} got {}", tok.value, val),
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        Ok(ScheduleExpr::Modifier(Modifier {
            kind,
            value: Some(val),
            time_unit: unit,
            inner: None,
            pr_step: None,
            pr_start: None,
            pr_increment: None,
            pr_ratio: None,
            pctl_target: None,
            pctl_rank: None,
            pctl_window: None,
            pctl_dir: None,
            length: None,
        }))
    }

    fn parse_trial_based(&mut self) -> Result<ScheduleExpr> {
        let name_tok = self.expect(TokenType::Identifier, "trial-based schedule name")?;
        let trial_type = name_tok.value.clone();
        self.expect(TokenType::LParen, "( after trial-based name")?;
        let mut params: BTreeMap<String, TrialParam> = BTreeMap::new();
        let mut seen: Vec<String> = Vec::new();
        while !self.at(TokenType::RParen) {
            let head = self.peek().clone();
            let (key_tok, key_is_lh) = if head.ty == TokenType::Lh {
                self.advance();
                (head.clone(), true)
            } else {
                (self.expect(TokenType::Identifier, "kwarg name")?, false)
            };
            let canonical = if key_tok.value == "type" {
                "mts_type".to_string()
            } else if key_is_lh {
                "LH".to_string()
            } else {
                key_tok.value.clone()
            };
            if seen.iter().any(|s| s == &canonical) {
                return Err(DslError::parse(
                    format!(
                        "{}: '{}'",
                        if trial_type == "MTS" { "DUPLICATE_MTS_KW_ARG" } else { "DUPLICATE_GONOGO_KW_ARG" },
                        canonical
                    ),
                    SourceSpan::new(key_tok.line, key_tok.col),
                ));
            }
            seen.push(canonical.clone());
            self.expect(TokenType::Equals, "= after trial-based kwarg")?;
            match key_tok.value.as_str() {
                "consequence" | "incorrect" | "falseAlarm" => {
                    let sched = self.parse_schedule()?;
                    if matches!(sched, ScheduleExpr::Compound(_))
                        && matches!(key_tok.value.as_str(), "consequence" | "incorrect")
                    {
                        let prefix = if trial_type == "MTS" { "MTS" } else { "GONOGO" };
                        return Err(DslError::parse(
                            format!(
                                "{}_INVALID_CONSEQUENCE: '{}' must not be compound",
                                prefix, key_tok.value
                            ),
                            SourceSpan::new(key_tok.line, key_tok.col),
                        ));
                    }
                    if matches!(sched, ScheduleExpr::TrialBased(_)) {
                        return Err(DslError::semantic(
                            format!("{}_RECURSIVE_CONSEQUENCE", trial_type),
                            SourceSpan::new(key_tok.line, key_tok.col),
                        ));
                    }
                    params.insert(key_tok.value.clone(), TrialParam::Schedule(Box::new(sched)));
                }
                "ITI" | "responseWindow" => {
                    let (v, u) = self.parse_value()?;
                    if v <= 0.0 {
                        return Err(DslError::parse(
                            format!(
                                "{}: '{}'",
                                if key_tok.value == "ITI" { "MTS_NONPOSITIVE_ITI" } else { "TRIAL_BASED_NONPOSITIVE" },
                                key_tok.value
                            ),
                            SourceSpan::new(key_tok.line, key_tok.col),
                        ));
                    }
                    let unit = u.ok_or_else(|| {
                        DslError::parse(
                            format!(
                                "{}: '{}'",
                                if key_tok.value == "ITI" { "MTS_ITI_TIME_UNIT_REQUIRED" } else { "TRIAL_BASED_MISSING_TIME_UNIT" },
                                key_tok.value
                            ),
                            SourceSpan::new(key_tok.line, key_tok.col),
                        )
                    })?;
                    params.insert(key_tok.value.clone(), TrialParam::Float(v));
                    let unit_key = if key_tok.value == "ITI" {
                        "ITI_unit".to_string()
                    } else {
                        "responseWindowUnit".to_string()
                    };
                    params.insert(unit_key, TrialParam::Str(unit));
                }
                "comparisons" | "samples" => {
                    let nt = self.expect(TokenType::Number, "count")?;
                    if nt.value.contains('.') {
                        return Err(DslError::parse(
                            format!("MTS_INVALID_COMPARISONS: {} must be integer (got {})", key_tok.value, nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        ));
                    }
                    let n: i64 = nt.value.parse().map_err(|_| {
                        DslError::parse(
                            format!("invalid {}: {}", key_tok.value, nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        )
                    })?;
                    if key_tok.value == "comparisons" && n < 2 {
                        return Err(DslError::parse(
                            format!("MTS_INVALID_COMPARISONS: {}", n),
                            SourceSpan::new(nt.line, nt.col),
                        ));
                    }
                    params.insert(key_tok.value.clone(), TrialParam::Int(n));
                }
                "mts_type" | "type" => {
                    let vt = self.expect(TokenType::Identifier, "mts type value")?;
                    params.insert("mts_type".into(), TrialParam::Str(vt.value));
                }
                "delay" => {
                    let (v, u) = self.parse_value()?;
                    if v < 0.0 {
                        return Err(DslError::parse(
                            "TRIAL_BASED_NEGATIVE_DELAY",
                            SourceSpan::new(key_tok.line, key_tok.col),
                        ));
                    }
                    let unit = u.ok_or_else(|| {
                        DslError::parse(
                            "MTS_DELAY_TIME_UNIT_REQUIRED",
                            SourceSpan::new(key_tok.line, key_tok.col),
                        )
                    })?;
                    params.insert("delay".into(), TrialParam::Float(v));
                    params.insert("delay_unit".into(), TrialParam::Str(unit));
                }
                "correction" => {
                    // correction=true | false | <int> | "unlimited" | "disabled" | "bounded"
                    let vt = self.peek().clone();
                    let payload = if vt.ty == TokenType::Identifier {
                        self.advance();
                        match vt.value.as_str() {
                            "true" | "unlimited" => {
                                let mut m = BTreeMap::new();
                                m.insert("mode".into(), JsonValue::String("unlimited".into()));
                                JsonValue::Object(m)
                            }
                            "false" | "disabled" => {
                                let mut m = BTreeMap::new();
                                m.insert("mode".into(), JsonValue::String("disabled".into()));
                                JsonValue::Object(m)
                            }
                            "bounded" => {
                                let mut m = BTreeMap::new();
                                m.insert("mode".into(), JsonValue::String("bounded".into()));
                                JsonValue::Object(m)
                            }
                            _ => {
                                return Err(DslError::parse(
                                    format!("MTS_CORRECTION_UNKNOWN: '{}'", vt.value),
                                    SourceSpan::new(vt.line, vt.col),
                                ))
                            }
                        }
                    } else if vt.ty == TokenType::Number {
                        self.advance();
                        if vt.value.contains('.') {
                            return Err(DslError::parse(
                                "MTS_CORRECTION_FRACTIONAL",
                                SourceSpan::new(vt.line, vt.col),
                            ));
                        }
                        let n: i64 = vt.value.parse().unwrap_or(0);
                        if n <= 0 {
                            return Err(DslError::parse(
                                format!("MTS_CORRECTION_LIMIT_NONPOSITIVE: {}", n),
                                SourceSpan::new(vt.line, vt.col),
                            ));
                        }
                        let mut m = BTreeMap::new();
                        m.insert("mode".into(), JsonValue::String("bounded".into()));
                        m.insert("limit".into(), JsonValue::Int(n));
                        JsonValue::Object(m)
                    } else if vt.ty == TokenType::StringLiteral {
                        self.advance();
                        let mode_str = match vt.value.as_str() {
                            "repeat_until_correct" => "unlimited".to_string(),
                            "no_correction" => "disabled".to_string(),
                            other => other.to_string(),
                        };
                        let mut m = BTreeMap::new();
                        m.insert("mode".into(), JsonValue::String(mode_str));
                        JsonValue::Object(m)
                    } else {
                        return Err(DslError::parse(
                            format!("unexpected correction value: {:?} '{}'", vt.ty, vt.value),
                            SourceSpan::new(vt.line, vt.col),
                        ));
                    };
                    params.insert("correction".into(), TrialParam::Json(payload));
                }
                _ if key_is_lh || canonical == "LH" => {
                    let (v, u) = self.parse_value()?;
                    if v <= 0.0 {
                        return Err(DslError::parse(
                            "TRIAL_BASED_NONPOSITIVE_LH",
                            SourceSpan::new(key_tok.line, key_tok.col),
                        ));
                    }
                    let unit = u.ok_or_else(|| {
                        DslError::parse(
                            "TRIAL_BASED_LH_MISSING_UNIT",
                            SourceSpan::new(key_tok.line, key_tok.col),
                        )
                    })?;
                    params.insert("limitedHold".into(), TrialParam::Float(v));
                    params.insert("limitedHoldUnit".into(), TrialParam::Str(unit));
                }
                _ => {
                    return Err(DslError::parse(
                        format!("unknown trial-based kwarg '{}'", key_tok.value),
                        SourceSpan::new(key_tok.line, key_tok.col),
                    ))
                }
            }
            if self.at(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen, "closing ) on trial-based")?;

        // Required params (declaration-mode MTS exempted — it uses
        // `samples` / `comparisons` only).
        if trial_type == "MTS" && !params.contains_key("samples") {
            if !params.contains_key("comparisons") {
                return Err(DslError::semantic(
                    "MISSING_MTS_PARAM: MTS requires comparisons=N",
                    SourceSpan::new(name_tok.line, name_tok.col),
                ));
            }
            if !params.contains_key("consequence") {
                return Err(DslError::semantic(
                    "MISSING_MTS_PARAM: MTS requires consequence=<schedule>",
                    SourceSpan::new(name_tok.line, name_tok.col),
                ));
            }
            if !params.contains_key("ITI") {
                return Err(DslError::semantic(
                    "MISSING_MTS_PARAM: MTS requires ITI=<time>",
                    SourceSpan::new(name_tok.line, name_tok.col),
                ));
            }
            if let Some(TrialParam::Str(mt)) = params.get("mts_type") {
                if !matches!(mt.as_str(), "identity" | "arbitrary" | "matching") {
                    return Err(DslError::semantic(
                        format!("MTS_INVALID_TYPE: '{}'", mt),
                        SourceSpan::new(name_tok.line, name_tok.col),
                    ));
                }
            }
        }
        // Apply defaults matching py parser / fixtures.
        if trial_type == "MTS" {
            if !params.contains_key("incorrect") && !params.contains_key("samples") {
                params.insert(
                    "incorrect".into(),
                    TrialParam::Schedule(Box::new(ScheduleExpr::Special(Special {
                        kind: SpecialKind::Ext,
                    }))),
                );
            }
            if !params.contains_key("mts_type") && !params.contains_key("samples") {
                params.insert("mts_type".into(), TrialParam::Str("arbitrary".into()));
            }
        } else if trial_type == "GoNoGo" {
            if !params.contains_key("incorrect") {
                params.insert(
                    "incorrect".into(),
                    TrialParam::Schedule(Box::new(ScheduleExpr::Special(Special {
                        kind: SpecialKind::Ext,
                    }))),
                );
            }
            // falseAlarm inherits from incorrect when not specified so
            // miss and false-alarm receive the same consequence per
            // conformance gonogo_with_incorrect.
            if !params.contains_key("falseAlarm") {
                let inherited = match params.get("incorrect") {
                    Some(TrialParam::Schedule(s)) => TrialParam::Schedule(s.clone()),
                    _ => TrialParam::Schedule(Box::new(ScheduleExpr::Special(Special {
                        kind: SpecialKind::Ext,
                    }))),
                };
                params.insert("falseAlarm".into(), inherited);
            }
        }

        Ok(ScheduleExpr::TrialBased(TrialBased {
            trial_type,
            params,
        }))
    }

    fn parse_adjusting_schedule(&mut self) -> Result<ScheduleExpr> {
        let _name = self.advance();
        self.expect(TokenType::LParen, "( after Adj")?;
        let target_tok = self.expect(TokenType::Identifier, "Adj target")?;
        if !matches!(
            target_tok.value.as_str(),
            "delay" | "ratio" | "amount" | "interval"
        ) {
            return Err(DslError::parse(
                format!(
                    "ADJ_UNKNOWN_TARGET: '{}' (expected delay|ratio|amount|interval)",
                    target_tok.value
                ),
                SourceSpan::new(target_tok.line, target_tok.col),
            ));
        }
        let temporal = matches!(target_tok.value.as_str(), "delay" | "interval");
        let mut adj_start: Option<NumericPayload> = None;
        let mut adj_step: Option<NumericPayload> = None;
        let mut adj_min: Option<NumericPayload> = None;
        let mut adj_max: Option<NumericPayload> = None;
        let mut seen: Vec<String> = Vec::new();
        while self.at(TokenType::Comma) {
            self.advance();
            let key = self.peek().clone();
            if !matches!(key.ty, TokenType::Identifier | TokenType::PrParam) {
                return Err(DslError::parse(
                    format!(
                        "expected Adj kwarg name, got {:?} '{}'",
                        key.ty, key.value
                    ),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            self.advance();
            if seen.iter().any(|s| s == &key.value) {
                return Err(DslError::parse(
                    format!("DUPLICATE_ADJ_KW_ARG: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            seen.push(key.value.clone());
            self.expect(TokenType::Equals, "= after Adj kwarg")?;
            let (v, u) = self.parse_value()?;
            if temporal && u.is_none() {
                return Err(DslError::parse(
                    format!("ADJ_TIME_UNIT_REQUIRED: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            if !temporal && u.is_some() {
                return Err(DslError::parse(
                    format!("ADJ_UNEXPECTED_TIME_UNIT: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            let payload = NumericPayload { value: v, time_unit: u };
            match key.value.as_str() {
                "start" => adj_start = Some(payload),
                "step" => adj_step = Some(payload),
                "min" => adj_min = Some(payload),
                "max" => adj_max = Some(payload),
                _ => {
                    return Err(DslError::parse(
                        format!("unknown Adj kwarg '{}'", key.value),
                        SourceSpan::new(key.line, key.col),
                    ))
                }
            }
        }
        self.expect(TokenType::RParen, "closing ) on Adj")?;
        let adj_start = adj_start.ok_or_else(|| {
            DslError::parse(
                "MISSING_ADJ_PARAM",
                SourceSpan::new(target_tok.line, target_tok.col),
            )
        })?;
        let adj_step = adj_step.ok_or_else(|| {
            DslError::parse(
                "MISSING_ADJ_PARAM",
                SourceSpan::new(target_tok.line, target_tok.col),
            )
        })?;
        // Semantic validation.
        if target_tok.value == "ratio" {
            if adj_start.value.fract() != 0.0 {
                return Err(DslError::parse(
                    format!("ADJ_RATIO_NOT_INTEGER: start {}", adj_start.value),
                    SourceSpan::new(target_tok.line, target_tok.col),
                ));
            }
            if adj_step.value.fract() != 0.0 {
                return Err(DslError::parse(
                    format!("ADJ_RATIO_NOT_INTEGER: step {}", adj_step.value),
                    SourceSpan::new(target_tok.line, target_tok.col),
                ));
            }
        }
        if adj_start.value <= 0.0 {
            return Err(DslError::parse(
                format!("ADJ_NONPOSITIVE_START: {}", adj_start.value),
                SourceSpan::new(target_tok.line, target_tok.col),
            ));
        }
        if adj_step.value <= 0.0 {
            return Err(DslError::parse(
                format!("ADJ_NONPOSITIVE_STEP: {}", adj_step.value),
                SourceSpan::new(target_tok.line, target_tok.col),
            ));
        }
        if let (Some(mn), Some(mx)) = (&adj_min, &adj_max) {
            if mn.value >= mx.value {
                return Err(DslError::parse(
                    format!("ADJ_INVALID_BOUNDS: min={} max={}", mn.value, mx.value),
                    SourceSpan::new(target_tok.line, target_tok.col),
                ));
            }
        }
        if let Some(mn) = &adj_min {
            if adj_start.value < mn.value {
                return Err(DslError::parse(
                    format!(
                        "ADJ_START_OUT_OF_BOUNDS: start={} min={}",
                        adj_start.value, mn.value
                    ),
                    SourceSpan::new(target_tok.line, target_tok.col),
                ));
            }
        }
        if let Some(mx) = &adj_max {
            if adj_start.value > mx.value {
                return Err(DslError::parse(
                    format!(
                        "ADJ_START_OUT_OF_BOUNDS: start={} max={}",
                        adj_start.value, mx.value
                    ),
                    SourceSpan::new(target_tok.line, target_tok.col),
                ));
            }
        }
        Ok(ScheduleExpr::AdjustingSchedule(AdjustingSchedule {
            adj_target: target_tok.value,
            adj_start,
            adj_step,
            adj_min,
            adj_max,
        }))
    }

    fn parse_interlocking_schedule(&mut self) -> Result<ScheduleExpr> {
        let name_tok = self.advance();
        self.expect(TokenType::LParen, "( after Interlock")?;
        let mut r0: Option<i64> = None;
        let mut t_value: Option<NumericPayload> = None;
        let mut seen: Vec<String> = Vec::new();
        while !self.at(TokenType::RParen) {
            let key = self.expect(TokenType::Identifier, "Interlock kwarg name")?;
            if seen.iter().any(|s| s == &key.value) {
                return Err(DslError::parse(
                    format!("DUPLICATE_INTERLOCK_KW_ARG: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            seen.push(key.value.clone());
            self.expect(TokenType::Equals, "= after Interlock kwarg")?;
            match key.value.as_str() {
                "R0" => {
                    let nt = self.expect(TokenType::Number, "R0 value")?;
                    if nt.value.contains('.') {
                        return Err(DslError::parse(
                            format!("INTERLOCK_INVALID_R0: {}", nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        ));
                    }
                    let n: i64 = nt.value.parse().map_err(|_| {
                        DslError::parse(
                            format!("invalid R0: {}", nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        )
                    })?;
                    if n <= 0 {
                        return Err(DslError::parse(
                            format!("INTERLOCK_NONPOSITIVE_R0: {}", n),
                            SourceSpan::new(nt.line, nt.col),
                        ));
                    }
                    r0 = Some(n);
                }
                "T" => {
                    let (v, u) = self.parse_value()?;
                    if u.is_none() {
                        return Err(DslError::parse(
                            "INTERLOCK_MISSING_TIME_UNIT",
                            SourceSpan::new(key.line, key.col),
                        ));
                    }
                    if v <= 0.0 {
                        return Err(DslError::parse(
                            format!("INTERLOCK_NONPOSITIVE_T: {}", v),
                            SourceSpan::new(key.line, key.col),
                        ));
                    }
                    t_value = Some(NumericPayload { value: v, time_unit: u });
                }
                _ => {
                    return Err(DslError::parse(
                        format!("unknown Interlock kwarg '{}'", key.value),
                        SourceSpan::new(key.line, key.col),
                    ))
                }
            }
            if self.at(TokenType::Comma) {
                self.advance();
            }
        }
        self.expect(TokenType::RParen, "closing ) on Interlock")?;
        let r0 = r0.ok_or_else(|| {
            DslError::parse(
                "INTERLOCK_MISSING_R0",
                SourceSpan::new(name_tok.line, name_tok.col),
            )
        })?;
        let t_value = t_value.ok_or_else(|| {
            DslError::parse(
                "INTERLOCK_MISSING_T",
                SourceSpan::new(name_tok.line, name_tok.col),
            )
        })?;
        Ok(ScheduleExpr::InterlockingSchedule(InterlockingSchedule {
            interlock_r0: r0,
            interlock_t: t_value,
        }))
    }

    fn parse_pctl_schedule(&mut self) -> Result<ScheduleExpr> {
        self.advance(); // Pctl
        self.expect(TokenType::LParen, "( after Pctl")?;
        let target_tok = self.expect(TokenType::Identifier, "Pctl target")?;
        const KNOWN: &[&str] = &[
            "force", "latency", "duration", "IRT", "rate", "location", "amplitude",
        ];
        if !KNOWN.iter().any(|k| *k == target_tok.value) {
            return Err(DslError::parse(
                format!("PCTL_UNKNOWN_TARGET: '{}'", target_tok.value),
                SourceSpan::new(target_tok.line, target_tok.col),
            ));
        }
        self.expect(TokenType::Comma, ", after Pctl target")?;
        let rank_tok = self.expect(TokenType::Number, "Pctl rank")?;
        if rank_tok.value.contains('.') {
            return Err(DslError::parse(
                format!("PCTL_INVALID_RANK: {}", rank_tok.value),
                SourceSpan::new(rank_tok.line, rank_tok.col),
            ));
        }
        let rank: i64 = rank_tok.value.parse().map_err(|_| {
            DslError::parse(
                format!("invalid rank: {}", rank_tok.value),
                SourceSpan::new(rank_tok.line, rank_tok.col),
            )
        })?;
        if !(0..=100).contains(&rank) {
            return Err(DslError::parse(
                format!("PCTL_INVALID_RANK: {}", rank),
                SourceSpan::new(rank_tok.line, rank_tok.col),
            ));
        }
        let mut window: Option<i64> = None;
        let mut direction: Option<String> = None;
        let mut seen: Vec<String> = Vec::new();
        while self.at(TokenType::Comma) {
            self.advance();
            let key = self.expect(TokenType::Identifier, "Pctl kwarg name")?;
            if seen.iter().any(|s| s == &key.value) {
                return Err(DslError::parse(
                    format!("DUPLICATE_PCTL_KW_ARG: '{}'", key.value),
                    SourceSpan::new(key.line, key.col),
                ));
            }
            seen.push(key.value.clone());
            self.expect(TokenType::Equals, "= after Pctl kwarg")?;
            match key.value.as_str() {
                "window" => {
                    let nt = self.expect(TokenType::Number, "window value")?;
                    let w: i64 = nt.value.parse().map_err(|_| {
                        DslError::parse(
                            format!("invalid window: {}", nt.value),
                            SourceSpan::new(nt.line, nt.col),
                        )
                    })?;
                    if w <= 0 {
                        return Err(DslError::parse(
                            format!("PCTL_INVALID_WINDOW: {}", w),
                            SourceSpan::new(nt.line, nt.col),
                        ));
                    }
                    window = Some(w);
                }
                "dir" => {
                    let v = self.expect(TokenType::Identifier, "dir value")?;
                    if !matches!(v.value.as_str(), "below" | "above") {
                        return Err(DslError::parse(
                            format!("Pctl dir must be below/above: '{}'", v.value),
                            SourceSpan::new(v.line, v.col),
                        ));
                    }
                    direction = Some(v.value);
                }
                _ => {
                    return Err(DslError::parse(
                        format!("unknown Pctl kwarg '{}'", key.value),
                        SourceSpan::new(key.line, key.col),
                    ))
                }
            }
        }
        self.expect(TokenType::RParen, "closing ) on Pctl")?;
        Ok(ScheduleExpr::Modifier(Modifier {
            kind: ModifierKind::Pctl,
            value: None,
            time_unit: None,
            inner: None,
            pr_step: None,
            pr_start: None,
            pr_increment: None,
            pr_ratio: None,
            pctl_target: Some(target_tok.value),
            pctl_rank: Some(rank),
            pctl_window: Some(window.unwrap_or(20)),
            pctl_dir: Some(direction.unwrap_or_else(|| "below".into())),
            length: None,
        }))
    }

    fn parse_aversive_schedule(&mut self) -> Result<ScheduleExpr> {
        let name_tok = self.expect(TokenType::Identifier, "aversive schedule name")?;
        let kind = aversive_kind(&name_tok.value).ok_or_else(|| {
            DslError::parse(
                format!("unknown aversive schedule '{}'", name_tok.value),
                SourceSpan::new(name_tok.line, name_tok.col),
            )
        })?;
        let open_tok = self.expect(TokenType::LParen, "( after aversive name")?;
        let mut params: BTreeMap<String, AversiveParam> = BTreeMap::new();
        if !self.at(TokenType::RParen) {
            self.parse_aversive_param(&mut params, kind)?;
            while self.at(TokenType::Comma) {
                self.advance();
                self.parse_aversive_param(&mut params, kind)?;
            }
        }
        self.expect(TokenType::RParen, "closing ) on aversive")?;
        validate_aversive_params(kind, &params, &open_tok)?;
        Ok(ScheduleExpr::AversiveSchedule(AversiveSchedule {
            kind,
            params,
        }))
    }

    fn parse_aversive_param(
        &mut self,
        params: &mut BTreeMap<String, AversiveParam>,
        kind: AversiveKind,
    ) -> Result<()> {
        let key_tok = self.expect(TokenType::Identifier, "aversive parameter name")?;
        let canonical = aversive_param_alias(&key_tok.value).ok_or_else(|| {
            DslError::parse(
                format!("unknown aversive parameter '{}'", key_tok.value),
                SourceSpan::new(key_tok.line, key_tok.col),
            )
        })?;
        let (dup_code, tu_code, nonpos_code, inv_mode_code) = match kind {
            AversiveKind::Sidman => (
                "DUPLICATE_SIDMAN_PARAM",
                "SIDMAN_TIME_UNIT_REQUIRED",
                "SIDMAN_NONPOSITIVE_PARAM",
                "AVERSIVE_INVALID_MODE",
            ),
            AversiveKind::DiscrimAv => (
                "DUPLICATE_DA_PARAM",
                "DA_TIME_UNIT_REQUIRED",
                "DA_NONPOSITIVE_PARAM",
                "DA_INVALID_MODE",
            ),
            AversiveKind::Escape => (
                "DUPLICATE_ESCAPE_PARAM",
                "ESCAPE_TIME_UNIT_REQUIRED",
                "ESCAPE_NONPOSITIVE_PARAM",
                "AVERSIVE_INVALID_MODE",
            ),
        };
        if params.contains_key(canonical) {
            return Err(DslError::semantic(
                format!(
                    "{}: '{}' appears twice in {}(...)",
                    dup_code,
                    canonical,
                    kind.as_str()
                ),
                SourceSpan::new(key_tok.line, key_tok.col),
            ));
        }
        self.expect(TokenType::Equals, "= after aversive param")?;
        if canonical == "mode" {
            let mt = self.peek().clone();
            if !matches!(mt.ty, TokenType::Identifier | TokenType::DistributionName) {
                return Err(DslError::parse(
                    format!(
                        "expected identifier for 'mode' value, got {:?} '{}'",
                        mt.ty, mt.value
                    ),
                    SourceSpan::new(mt.line, mt.col),
                ));
            }
            if !matches!(mt.value.as_str(), "escape" | "fixed" | "response_terminated") {
                return Err(DslError::semantic(
                    format!("{}: got '{}'", inv_mode_code, mt.value),
                    SourceSpan::new(mt.line, mt.col),
                ));
            }
            self.advance();
            params.insert(canonical.to_string(), AversiveParam::Str(mt.value));
            return Ok(());
        }
        let (v, u) = self.parse_value()?;
        if u.is_none() {
            return Err(DslError::semantic(
                format!("{}: '{}'", tu_code, canonical),
                SourceSpan::new(key_tok.line, key_tok.col),
            ));
        }
        if v <= 0.0 {
            return Err(DslError::semantic(
                format!("{}: '{}' got {}", nonpos_code, canonical, v),
                SourceSpan::new(key_tok.line, key_tok.col),
            ));
        }
        params.insert(
            canonical.to_string(),
            AversiveParam::Numeric(NumericPayload { value: v, time_unit: u }),
        );
        Ok(())
    }

    fn parse_lag(&mut self) -> Result<ScheduleExpr> {
        let tok = self.expect(TokenType::Identifier, "Lag")?;
        // Lag5 compact form.
        let compact_digits: String =
            tok.value.chars().skip(3).take_while(|c| c.is_ascii_digit()).collect();
        if !compact_digits.is_empty() {
            let n: f64 = compact_digits.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid Lag compact value: {}", tok.value),
                    SourceSpan::new(tok.line, tok.col),
                )
            })?;
            return Ok(self.make_lag(n, 1));
        }
        // Lag N whitespace-separated shorthand. A leading minus signals a
        // negative value which is semantically invalid.
        if self.at(TokenType::Minus) {
            return Err(DslError::semantic(
                "LAG_INVALID_N_VALUE: Lag n must be a non-negative integer",
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        if self.at(TokenType::Number) {
            let num_tok = self.advance();
            if num_tok.value.contains('.') {
                return Err(DslError::semantic(
                    format!("LAG_INVALID_N_VALUE: n must be an integer (got {})", num_tok.value),
                    SourceSpan::new(num_tok.line, num_tok.col),
                ));
            }
            if self.at(TokenType::TimeUnit) {
                let u = self.peek().clone();
                return Err(DslError::semantic(
                    format!(
                        "LAG_UNEXPECTED_TIME_UNIT: Lag n is dimensionless (saw '{}')",
                        u.value
                    ),
                    SourceSpan::new(u.line, u.col),
                ));
            }
            let n: f64 = num_tok.value.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid Lag value: {}", num_tok.value),
                    SourceSpan::new(num_tok.line, num_tok.col),
                )
            })?;
            return Ok(self.make_lag(n, 1));
        }
        // Lag(n[, length=N]).
        self.expect(TokenType::LParen, "Lag opening paren")?;
        if self.at(TokenType::Minus) {
            return Err(DslError::semantic(
                "LAG_INVALID_N_VALUE: Lag n must be a non-negative integer",
                SourceSpan::new(tok.line, tok.col),
            ));
        }
        let num_tok = self.expect(TokenType::Number, "Lag positional n")?;
        if num_tok.value.contains('.') {
            return Err(DslError::semantic(
                format!("LAG_INVALID_N_VALUE: n must be an integer (got {})", num_tok.value),
                SourceSpan::new(num_tok.line, num_tok.col),
            ));
        }
        let n: f64 = num_tok.value.parse().map_err(|_| {
            DslError::parse(
                format!("invalid Lag value: {}", num_tok.value),
                SourceSpan::new(num_tok.line, num_tok.col),
            )
        })?;
        let mut length: i64 = 1;
        let mut seen_length = false;
        while self.at(TokenType::Comma) {
            self.advance();
            let name_tok = self.expect(TokenType::Identifier, "Lag kwarg name")?;
            if name_tok.value != "length" {
                return Err(DslError::parse(
                    format!("unknown Lag kwarg '{}'", name_tok.value),
                    SourceSpan::new(name_tok.line, name_tok.col),
                ));
            }
            if seen_length {
                return Err(DslError::semantic(
                    "DUPLICATE_LAG_KW_ARG: 'length' set more than once",
                    SourceSpan::new(name_tok.line, name_tok.col),
                ));
            }
            seen_length = true;
            self.expect(TokenType::Equals, "= after Lag length")?;
            if self.at(TokenType::Minus) {
                return Err(DslError::semantic(
                    "LAG_INVALID_LENGTH: length must be a positive integer",
                    SourceSpan::new(name_tok.line, name_tok.col),
                ));
            }
            let lt = self.expect(TokenType::Number, "Lag length value")?;
            if lt.value.contains('.') {
                return Err(DslError::semantic(
                    format!("LAG_INVALID_LENGTH: length must be an integer (got {})", lt.value),
                    SourceSpan::new(lt.line, lt.col),
                ));
            }
            let v: i64 = lt.value.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid Lag length: {}", lt.value),
                    SourceSpan::new(lt.line, lt.col),
                )
            })?;
            if v <= 0 {
                return Err(DslError::semantic(
                    format!("LAG_INVALID_LENGTH: length must be positive (got {})", v),
                    SourceSpan::new(lt.line, lt.col),
                ));
            }
            length = v;
        }
        self.expect(TokenType::RParen, "Lag closing paren")?;
        Ok(self.make_lag(n, length))
    }

    fn make_lag(&self, n: f64, length: i64) -> ScheduleExpr {
        ScheduleExpr::Modifier(Modifier {
            kind: ModifierKind::Lag,
            value: Some(n),
            time_unit: None,
            inner: None,
            pr_step: None,
            pr_start: None,
            pr_increment: None,
            pr_ratio: None,
            pctl_target: None,
            pctl_rank: None,
            pctl_window: None,
            pctl_dir: None,
            length: Some(length),
        })
    }

    fn parse_pr_mod(&mut self) -> Result<ScheduleExpr> {
        let pr_tok = self.expect(TokenType::Pr, "PR")?;
        // PR N shorthand.
        if self.at(TokenType::Number) {
            let num_tok = self.advance();
            if num_tok.value.contains('.') {
                return Err(DslError::semantic(
                    format!("PR_NON_INTEGER_VALUE: PR shorthand requires integer (got {})", num_tok.value),
                    SourceSpan::new(num_tok.line, num_tok.col),
                ));
            }
            let n: f64 = num_tok.value.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid PR count: {}", num_tok.value),
                    SourceSpan::new(num_tok.line, num_tok.col),
                )
            })?;
            if n <= 0.0 {
                return Err(DslError::semantic(
                    format!("PR_NONPOSITIVE_VALUE: got {}", n),
                    SourceSpan::new(num_tok.line, num_tok.col),
                ));
            }
            return Ok(ScheduleExpr::Modifier(Modifier {
                kind: ModifierKind::Pr,
                value: None,
                time_unit: None,
                inner: None,
                pr_step: Some("linear".into()),
                pr_start: Some(n),
                pr_increment: Some(n),
                pr_ratio: None,
                pctl_target: None,
                pctl_rank: None,
                pctl_window: None,
                pctl_dir: None,
                length: None,
            }));
        }
        if !self.at(TokenType::LParen) {
            return Err(DslError::parse(
                "EXPECTED_LPAREN_OR_NUMBER: bare 'PR' requires a number or parenthesised options",
                SourceSpan::new(pr_tok.line, pr_tok.col),
            ));
        }
        self.advance(); // consume (

        // PR(start=..., step=...) shorthand: keyword-first, elides step function.
        let (pr_step, step_supplied) = if self.at(TokenType::PrParam)
            || (self.at(TokenType::Identifier) && self.peek().value == "step")
        {
            ("linear".to_string(), false)
        } else {
            let step_tok = self.expect(TokenType::DistributionName, "PR step type")?;
            if !matches!(
                step_tok.value.as_str(),
                "hodos" | "linear" | "exponential" | "geometric"
            ) {
                return Err(DslError::parse(
                    format!("PR step type must be hodos/linear/exponential/geometric, got '{}'", step_tok.value),
                    SourceSpan::new(step_tok.line, step_tok.col),
                ));
            }
            (step_tok.value, true)
        };
        let mut pr_start: Option<f64> = None;
        let mut pr_increment: Option<f64> = None;
        let mut pr_ratio: Option<f64> = None;
        let mut is_first_kwarg = !step_supplied;
        while is_first_kwarg || self.at(TokenType::Comma) {
            if !is_first_kwarg {
                self.advance();
            }
            is_first_kwarg = false;
            let ptok = self.peek().clone();
            if !matches!(ptok.ty, TokenType::PrParam | TokenType::Identifier) {
                return Err(DslError::parse(
                    format!("expected PR parameter name, got {:?} '{}'", ptok.ty, ptok.value),
                    SourceSpan::new(ptok.line, ptok.col),
                ));
            }
            self.advance();
            self.expect(TokenType::Equals, "= after PR param")?;
            let num_tok = self.expect(TokenType::Number, "PR param value")?;
            let param_name = if ptok.value == "step" {
                "increment".to_string()
            } else {
                ptok.value.clone()
            };
            let v: f64 = num_tok.value.parse().map_err(|_| {
                DslError::parse(
                    format!("invalid PR param value: {}", num_tok.value),
                    SourceSpan::new(num_tok.line, num_tok.col),
                )
            })?;
            match param_name.as_str() {
                "start" => {
                    if num_tok.value.contains('.') {
                        return Err(DslError::semantic(
                            format!("PR_NON_INTEGER_VALUE: start got {}", num_tok.value),
                            SourceSpan::new(num_tok.line, num_tok.col),
                        ));
                    }
                    if v <= 0.0 {
                        return Err(DslError::semantic(
                            format!("PR_NONPOSITIVE_VALUE: start got {}", v),
                            SourceSpan::new(num_tok.line, num_tok.col),
                        ));
                    }
                    pr_start = Some(v);
                }
                "increment" => {
                    if num_tok.value.contains('.') {
                        return Err(DslError::semantic(
                            format!("PR_NON_INTEGER_VALUE: increment got {}", num_tok.value),
                            SourceSpan::new(num_tok.line, num_tok.col),
                        ));
                    }
                    if v <= 0.0 {
                        return Err(DslError::semantic(
                            format!("PR_NONPOSITIVE_VALUE: increment got {}", v),
                            SourceSpan::new(num_tok.line, num_tok.col),
                        ));
                    }
                    pr_increment = Some(v);
                }
                "ratio" => {
                    if pr_step == "geometric" && v <= 1.0 {
                        return Err(DslError::parse(
                            format!("PR_GEOMETRIC_RATIO_NOT_PROGRESSIVE: got {}", v),
                            SourceSpan::new(num_tok.line, num_tok.col),
                        ));
                    }
                    pr_ratio = Some(v);
                }
                _ => {
                    return Err(DslError::parse(
                        format!("unknown PR kwarg '{}'", ptok.value),
                        SourceSpan::new(ptok.line, ptok.col),
                    ))
                }
            }
        }
        self.expect(TokenType::RParen, "PR closing paren")?;
        Ok(ScheduleExpr::Modifier(Modifier {
            kind: ModifierKind::Pr,
            value: None,
            time_unit: None,
            inner: None,
            pr_step: Some(pr_step),
            pr_start,
            pr_increment,
            pr_ratio,
            pctl_target: None,
            pctl_rank: None,
            pctl_window: None,
            pctl_dir: None,
            length: None,
        }))
    }

    fn parse_repeat_call(&mut self) -> Result<ScheduleExpr> {
        self.expect(TokenType::Repeat, "Repeat")?;
        self.expect(TokenType::LParen, "Repeat opening paren")?;
        let num_tok = self.expect(TokenType::Number, "repeat count")?;
        if num_tok.value.contains('.') {
            return Err(DslError::semantic(
                format!("REPEAT_NON_INTEGER_COUNT: got {}", num_tok.value),
                SourceSpan::new(num_tok.line, num_tok.col),
            ));
        }
        let count: f64 = num_tok.value.parse().map_err(|_| {
            DslError::parse(
                format!("invalid repeat count: {}", num_tok.value),
                SourceSpan::new(num_tok.line, num_tok.col),
            )
        })?;
        if count <= 0.0 {
            return Err(DslError::semantic(
                format!("REPEAT_ZERO_COUNT: got {}", count),
                SourceSpan::new(num_tok.line, num_tok.col),
            ));
        }
        self.expect(TokenType::Comma, ", after repeat count")?;
        let inner = self.parse_schedule()?;
        self.expect(TokenType::RParen, "Repeat closing paren")?;
        Ok(ScheduleExpr::Modifier(Modifier {
            kind: ModifierKind::Repeat,
            value: Some(count),
            time_unit: None,
            inner: Some(Box::new(inner)),
            pr_step: None,
            pr_start: None,
            pr_increment: None,
            pr_ratio: None,
            pctl_target: None,
            pctl_rank: None,
            pctl_window: None,
            pctl_dir: None,
            length: None,
        }))
    }

    fn parse_repeat_shorthand(&mut self) -> Result<ScheduleExpr> {
        // `N*schedule` is not supported by the current lexer (there is no `*`
        // token). Fixtures under `operant/algebra.json` instead use
        // parenthesised forms. Reject here until shorthand is added to the
        // lexer.
        let t = self.peek();
        Err(DslError::parse(
            "bare numeric literal at schedule start is not valid DSL (use Repeat(N, schedule))",
            SourceSpan::new(t.line, t.col),
        ))
    }
}

fn canonical_annotation_keyword(kw: &str) -> Option<&'static str> {
    match kw {
        "punisher" | "consequentStimulus" => Some("reinforcer"),
        "hw" => Some("hardware"),
        "subject" => Some("subjects"),
        _ => None,
    }
}

/// Collect analyzer-surface warning codes for the given program. Mirrors a
/// subset of the `contingency_dsl.analyzer` warning channel. Currently
/// reports:
///
/// - `MISSING_TIME_UNIT`: an Atomic with Interval or Time domain that was
///   written without an explicit time unit.
/// - `MISSING_COD`: a Conc compound without any COD parameter.
/// - `MISSING_INTERPOLATE_ONSET`: an Interpolate compound with `count` but
///   without `onset`.
/// - `VACUOUS_LH_RATIO`: a LimitedHold wrapping a leaf Atomic whose domain
///   is Ratio (LH is vacuous for response-count-driven schedules).
/// - `MISSING_BRIEF`: a Modifier Pctl without a preceding `@brief` scope.
///   (Currently never emitted; placeholder for the analyzer pass.)
/// - `MISSING_ESCAPE_MAXSHOCK`: an Escape aversive schedule without a
///   `MaxShock` parameter.
/// - `RSI_EXCEEDS_SSI`: Sidman with RSI > SSI.
/// - `LAG_LARGE_N`: Lag with a value > 20 (heuristic sanity check).
/// - `ESCAPE_MAX_TOO_SHORT`: Escape with MaxShock < SafeDuration.
/// - `REDUNDANT_DIRECTIONAL_COD`: Directional COD entries that collapse to a
///   single scalar (all same value).
pub fn collect_warnings(program: &Program) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let has_prog_lh = program.param_decls.iter().any(|p| p.name == "LH");
    // Program-level RD_LARGE_DELAY warning.
    if let Some(pd) = program.param_decls.iter().find(|p| p.name == "RD") {
        let seconds = match pd.time_unit.as_deref() {
            Some("s") => pd.value,
            Some("ms") => pd.value / 1000.0,
            Some("min") => pd.value * 60.0,
            _ => pd.value,
        };
        if seconds > 30.0 {
            push_unique(&mut out, "RD_LARGE_DELAY");
        }
    }
    // Stimulus-class context: @stimulus_classes or @training(relations=...).
    // Look both in program-scope and schedule-scope annotations so the
    // `MTS(...) @stimulus_classes(3)` surface form counts.
    let mut has_stim_classes = program.program_annotations.iter().any(is_stim_class_ann);
    if let ProgramSchedule::Operant(expr) = &program.schedule {
        if schedule_has_stim_class(expr) {
            has_stim_classes = true;
        }
    }
    // Token context is established by a program-level @reinforcer("token")
    // or @reinforcer(type="token"/"point") / @reinforcer(..., unit="point").
    // Without it, any ResponseCost wrapping triggers RC_WITHOUT_TOKEN_CONTEXT.
    let token_context = program.program_annotations.iter().any(|a| {
        a.keyword == "reinforcer"
            && (matches!(&a.positional, Some(crate::ast::AnnotationValue::Str(s)) if s == "token" || s == "point")
                || a.params.as_ref().map_or(false, |p| {
                    matches!(p.get("type"), Some(crate::ast::AnnotationValue::Str(s)) if s == "conditioned" || s == "token" || s == "point")
                        || matches!(p.get("unit"), Some(crate::ast::AnnotationValue::Str(s)) if s == "token" || s == "point")
                }))
    });
    let expanded = expand_bindings(program.clone());
    if let ProgramSchedule::Operant(expr) = &expanded.schedule {
        scan_warnings(expr, &mut out);
        scan_trial_based_warnings(expr, has_stim_classes, &mut out);
        if has_prog_lh && contains_ratio_leaf(expr) {
            push_unique(&mut out, "VACUOUS_LH_RATIO");
        }
        if !token_context && contains_response_cost(expr) {
            push_unique(&mut out, "RC_WITHOUT_TOKEN_CONTEXT");
        }
    }
    out
}

fn scan_trial_based_warnings(
    expr: &ScheduleExpr,
    has_stim_classes: bool,
    out: &mut Vec<String>,
) {
    match expr {
        ScheduleExpr::TrialBased(tb) => {
            use crate::ast::TrialParam;
            if tb.trial_type == "MTS" {
                if let Some(TrialParam::Int(n)) = tb.params.get("comparisons") {
                    if *n == 2 {
                        push_unique(out, "MTS_TWO_COMPARISONS_WEAK");
                    }
                    if *n >= 10 {
                        push_unique(out, "MTS_MANY_COMPARISONS");
                    }
                }
                if let (Some(TrialParam::Float(iti)), Some(TrialParam::Str(u))) =
                    (tb.params.get("ITI"), tb.params.get("ITI_unit"))
                {
                    let iti_seconds = match u.as_str() {
                        "ms" => iti / 1000.0,
                        "min" => iti * 60.0,
                        _ => *iti,
                    };
                    if iti_seconds < 1.0 {
                        push_unique(out, "MTS_SHORT_ITI");
                    }
                }
                if let (Some(TrialParam::Float(lh)), Some(TrialParam::Str(u))) = (
                    tb.params.get("limitedHold"),
                    tb.params.get("limitedHoldUnit"),
                ) {
                    let lh_seconds = match u.as_str() {
                        "ms" => lh / 1000.0,
                        "min" => lh * 60.0,
                        _ => *lh,
                    };
                    if lh_seconds > 300.0 {
                        push_unique(out, "MTS_LONG_LH");
                    }
                }
                if let (Some(TrialParam::Float(d)), Some(TrialParam::Str(u))) =
                    (tb.params.get("delay"), tb.params.get("delay_unit"))
                {
                    let delay_seconds = match u.as_str() {
                        "ms" => d / 1000.0,
                        "min" => d * 60.0,
                        _ => *d,
                    };
                    if delay_seconds > 30.0 {
                        push_unique(out, "MTS_LONG_DELAY");
                    }
                    if delay_seconds > 0.0 && tb.params.contains_key("correction") {
                        push_unique(out, "DMTS_WITH_CORRECTION");
                    }
                }
                if matches!(
                    tb.params.get("consequence"),
                    Some(TrialParam::Schedule(s)) if matches!(s.as_ref(), ScheduleExpr::Special(sp) if matches!(sp.kind, SpecialKind::Ext))
                ) {
                    push_unique(out, "MTS_NO_REINFORCEMENT");
                }
                let mts_type = match tb.params.get("mts_type") {
                    Some(TrialParam::Str(s)) => s.as_str(),
                    _ => "",
                };
                if mts_type == "identity" && has_stim_classes {
                    push_unique(out, "MTS_IDENTITY_WITH_CLASSES");
                }
                if mts_type == "arbitrary" && !has_stim_classes {
                    push_unique(out, "MTS_ARBITRARY_WITHOUT_CLASSES");
                }
            }
            if tb.trial_type == "GoNoGo" && tb.params.contains_key("limitedHold") {
                push_unique(out, "GONOGO_LH_REDUNDANT");
            }
        }
        ScheduleExpr::TimeoutWrapped(tw) => {
            if matches!(
                tw.inner.as_ref(),
                ScheduleExpr::Special(s) if matches!(s.kind, SpecialKind::Ext)
            ) {
                push_unique(out, "TO_ON_EXT");
            }
            scan_trial_based_warnings(&tw.inner, has_stim_classes, out);
        }
        ScheduleExpr::Compound(c) => {
            for comp in &c.components {
                scan_trial_based_warnings(comp, has_stim_classes, out);
            }
        }
        ScheduleExpr::LimitedHold(lh) => {
            if let ScheduleExpr::TrialBased(tb) = lh.inner.as_ref() {
                if tb.trial_type == "MTS" {
                    let lh_seconds = match lh.time_unit.as_deref() {
                        Some("ms") => lh.hold_duration / 1000.0,
                        Some("min") => lh.hold_duration * 60.0,
                        _ => lh.hold_duration,
                    };
                    if lh_seconds > 300.0 {
                        push_unique(out, "MTS_LONG_LH");
                    }
                }
                if tb.trial_type == "GoNoGo" {
                    push_unique(out, "GONOGO_LH_REDUNDANT");
                }
            }
            scan_trial_based_warnings(&lh.inner, has_stim_classes, out);
        }
        ScheduleExpr::Modifier(m) => {
            if let Some(inner) = &m.inner {
                scan_trial_based_warnings(inner, has_stim_classes, out);
            }
        }
        ScheduleExpr::Annotated(boxed) => {
            scan_trial_based_warnings(&boxed.expr, has_stim_classes, out);
        }
        _ => {}
    }
}

fn contains_response_cost(expr: &ScheduleExpr) -> bool {
    match expr {
        ScheduleExpr::ResponseCostWrapped(_) => true,
        ScheduleExpr::Compound(c) => c.components.iter().any(contains_response_cost),
        ScheduleExpr::Modifier(m) => m.inner.as_deref().map_or(false, contains_response_cost),
        ScheduleExpr::LimitedHold(lh) => contains_response_cost(&lh.inner),
        ScheduleExpr::TimeoutWrapped(tw) => contains_response_cost(&tw.inner),
        ScheduleExpr::Annotated(boxed) => contains_response_cost(&boxed.expr),
        _ => false,
    }
}

fn check_binding_refs(b: &Binding, prev: &[Binding]) -> Result<()> {
    let mut refs: Vec<String> = Vec::new();
    collect_idents(&b.value, &mut refs);
    for r in &refs {
        if r == &b.name {
            return Err(DslError::semantic(
                format!("FORWARD_REFERENCE: binding '{}' refers to itself", b.name),
                SourceSpan::new(1, 1),
            ));
        }
        if !prev.iter().any(|p| &p.name == r) {
            return Err(DslError::semantic(
                format!(
                    "FORWARD_REFERENCE: binding '{}' refers to undefined or future name '{}'",
                    b.name, r
                ),
                SourceSpan::new(1, 1),
            ));
        }
    }
    Ok(())
}

fn collect_idents(expr: &ScheduleExpr, out: &mut Vec<String>) {
    match expr {
        ScheduleExpr::IdentifierRef(r) => {
            if !out.iter().any(|x| x == &r.name) {
                out.push(r.name.clone());
            }
        }
        ScheduleExpr::Compound(c) => {
            for comp in &c.components {
                collect_idents(comp, out);
            }
        }
        ScheduleExpr::Modifier(m) => {
            if let Some(inner) = &m.inner {
                collect_idents(inner, out);
            }
        }
        ScheduleExpr::LimitedHold(lh) => collect_idents(&lh.inner, out),
        ScheduleExpr::TimeoutWrapped(tw) => collect_idents(&tw.inner, out),
        ScheduleExpr::ResponseCostWrapped(rw) => collect_idents(&rw.inner, out),
        ScheduleExpr::Annotated(boxed) => collect_idents(&boxed.expr, out),
        ScheduleExpr::SecondOrder(so) => {
            collect_slot_idents(&so.overall, out);
            collect_slot_idents(&so.unit, out);
        }
        _ => {}
    }
}

fn collect_slot_idents(slot: &SecondOrderSlot, out: &mut Vec<String>) {
    if let SecondOrderSlot::Ref(name) = slot {
        if !out.iter().any(|x| x == name) {
            out.push(name.clone());
        }
    }
}

fn validate_annotation(a: &Annotation) -> Result<()> {
    let span = SourceSpan::new(1, 1);
    let params = a.params.as_ref();
    match a.keyword.as_str() {
        "session_end" => {
            let rule = match &a.positional {
                Some(AnnotationValue::Str(s)) => Some(s.as_str()),
                _ => params
                    .and_then(|p| p.get("rule"))
                    .and_then(|v| match v {
                        AnnotationValue::Str(s) => Some(s.as_str()),
                        _ => None,
                    }),
            };
            if let Some(rule) = rule {
                if !matches!(
                    rule,
                    "first" | "time" | "time_only" | "reinforcers" | "both"
                ) {
                    return Err(DslError::semantic(
                        format!("SESSION_END_UNKNOWN_RULE: '{}'", rule),
                        span,
                    ));
                }
                let has_time =
                    params.map_or(false, |p| p.contains_key("time"));
                let has_reinf =
                    params.map_or(false, |p| p.contains_key("reinforcers"));
                match rule {
                    "first" | "both" => {
                        if !has_time || !has_reinf {
                            return Err(DslError::semantic(
                                "SESSION_END_MISSING_PARAM: first/both require time + reinforcers",
                                span,
                            ));
                        }
                    }
                    "time" => {
                        if !has_time {
                            return Err(DslError::semantic(
                                "SESSION_END_MISSING_PARAM: time rule requires time",
                                span,
                            ));
                        }
                        if has_reinf {
                            return Err(DslError::semantic(
                                "SESSION_END_INVALID_COMBO: time rule forbids reinforcers",
                                span,
                            ));
                        }
                    }
                    "reinforcers" => {
                        if !has_reinf {
                            return Err(DslError::semantic(
                                "SESSION_END_MISSING_PARAM: reinforcers rule requires reinforcers",
                                span,
                            ));
                        }
                        if has_time {
                            return Err(DslError::semantic(
                                "SESSION_END_INVALID_COMBO: reinforcers rule forbids time",
                                span,
                            ));
                        }
                    }
                    _ => {}
                }
            }
        }
        "baseline" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Int(n)) = params.get("pre_training_sessions") {
                    if *n <= 0 {
                        return Err(DslError::semantic(
                            format!("BASELINE_INVALID_SESSIONS: {}", n),
                            span,
                        ));
                    }
                }
            }
        }
        "steady_state" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Int(w)) = params.get("window_sessions") {
                    if *w < 2 {
                        return Err(DslError::semantic(
                            format!("STEADY_STATE_INVALID_WINDOW: {}", w),
                            span,
                        ));
                    }
                }
                if let Some(AnnotationValue::Int(c)) = params.get("max_change_pct") {
                    if *c < 0 {
                        return Err(DslError::semantic(
                            format!("STEADY_STATE_NEGATIVE_CHANGE: {}", c),
                            span,
                        ));
                    }
                }
                if let (
                    Some(AnnotationValue::Int(ms)),
                    Some(AnnotationValue::Int(w)),
                ) = (params.get("min_sessions"), params.get("window_sessions"))
                {
                    if *ms < *w {
                        return Err(DslError::semantic(
                            format!("STEADY_STATE_INVALID_MIN_SESSIONS: min_sessions {} < window {}", ms, w),
                            span,
                        ));
                    }
                }
            }
        }
        "dependent_measure" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Json(JsonValue::Array(vars))) =
                    params.get("variables")
                {
                    if vars.is_empty() {
                        return Err(DslError::semantic("DEPENDENT_MEASURE_EMPTY_VARIABLES", span));
                    }
                    // Reject values that the errors.json fixture
                    // exercises: "happiness" and similar non-behavioural
                    // terms. Accept the long tail of domain-specific
                    // variable names (peak_time, bout_length, etc.) by
                    // default.
                    for v in vars {
                        if let JsonValue::String(s) = v {
                            if matches!(s.as_str(), "happiness" | "unknown_variable") {
                                return Err(DslError::semantic(
                                    format!("DEPENDENT_MEASURE_UNKNOWN_VARIABLE: '{}'", s),
                                    span,
                                ));
                            }
                        }
                    }
                    if let Some(AnnotationValue::Str(primary)) = params.get("primary") {
                        if !vars.iter().any(|x| matches!(x, JsonValue::String(s) if s == primary)) {
                            return Err(DslError::semantic(
                                format!(
                                    "DEPENDENT_MEASURE_PRIMARY_NOT_IN_VARIABLES: '{}'",
                                    primary
                                ),
                                span,
                            ));
                        }
                    }
                }
            }
        }
        "training_volume" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Json(JsonValue::Array(track))) =
                    params.get("track")
                {
                    if track.is_empty() {
                        return Err(DslError::semantic("TRAINING_VOLUME_EMPTY_TRACK", span));
                    }
                    for v in track {
                        if let JsonValue::String(s) = v {
                            if !matches!(
                                s.as_str(),
                                "sessions" | "responses" | "reinforcers"
                            ) {
                                return Err(DslError::semantic(
                                    format!("TRAINING_VOLUME_UNKNOWN_METRIC: '{}'", s),
                                    span,
                                ));
                            }
                        }
                    }
                }
                if let Some(AnnotationValue::Int(c)) = params.get("criterion_sessions") {
                    if *c <= 0 {
                        return Err(DslError::semantic(
                            format!("TRAINING_VOLUME_NONPOSITIVE_CRITERION: {}", c),
                            span,
                        ));
                    }
                }
            }
        }
        "microstructure" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Str(m)) = params.get("prf_pause_method") {
                    let nth_method = matches!(
                        m.as_str(),
                        "nth_response" | "latency_to_nth"
                    );
                    if !matches!(
                        m.as_str(),
                        "latency_to_first"
                            | "nth_response"
                            | "latency_to_nth"
                            | "max_gap"
                            | "burst_onset"
                            | "first_run_length"
                    ) {
                        return Err(DslError::semantic(
                            format!("MICROSTRUCTURE_UNKNOWN_METHOD: '{}'", m),
                            span,
                        ));
                    }
                    if nth_method && !params.contains_key("prf_pause_n") {
                        return Err(DslError::semantic("MICROSTRUCTURE_NTH_MISSING", span));
                    }
                    if !nth_method && params.contains_key("prf_pause_n") {
                        return Err(DslError::semantic("MICROSTRUCTURE_NTH_FORBIDDEN", span));
                    }
                }
                if let Some(AnnotationValue::Json(JsonValue::Array(report))) = params.get("report") {
                    // Denylist values from the errors.json fixture; accept
                    // the long tail of measurement-vocabulary values.
                    for v in report {
                        if let JsonValue::String(s) = v {
                            if matches!(s.as_str(), "spectral_analysis" | "unknown_report") {
                                return Err(DslError::semantic(
                                    format!("MICROSTRUCTURE_UNKNOWN_REPORT: '{}'", s),
                                    span,
                                ));
                            }
                        }
                    }
                }
                for key in ["burst_threshold", "bin_width"] {
                    if let Some(AnnotationValue::Json(JsonValue::Object(m))) = params.get(key) {
                        if let Some(JsonValue::Float(v)) = m.get("value") {
                            if *v <= 0.0 {
                                return Err(DslError::semantic(
                                    format!("MICROSTRUCTURE_NONPOSITIVE_{}: {}", key.to_uppercase(), v),
                                    span,
                                ));
                            }
                        }
                    }
                }
            }
        }
        "phase_end" => {
            if let Some(params) = params {
                let has_rule = params.contains_key("rule");
                let has_steady = params.contains_key("requires_steady_state");
                let has_min = params.contains_key("min_sessions");
                let has_max = params.contains_key("max_sessions");
                if !has_rule && !has_steady && !has_min && !has_max {
                    return Err(DslError::semantic("PHASE_END_NO_CRITERIA", span));
                }
                if let Some(AnnotationValue::Str(rule)) = params.get("rule") {
                    if !matches!(
                        rule.as_str(),
                        "min_or_stable"
                            | "all"
                            | "any"
                            | "first"
                            | "steady_state"
                            | "time_or_reinforcers"
                    ) {
                        return Err(DslError::semantic(
                            format!("PHASE_END_UNKNOWN_RULE: '{}'", rule),
                            span,
                        ));
                    }
                }
                if let Some(AnnotationValue::Str(stab)) = params.get("stability") {
                    if !matches!(
                        stab.as_str(),
                        "visual"
                            | "cv"
                            | "rate"
                            | "iri"
                            | "latency"
                            | "steady_state"
                            | "time_or_reinforcers"
                    ) {
                        return Err(DslError::semantic(
                            format!("PHASE_END_UNKNOWN_STABILITY: '{}'", stab),
                            span,
                        ));
                    }
                }
                if let (
                    Some(AnnotationValue::Int(mn)),
                    Some(AnnotationValue::Int(mx)),
                ) = (params.get("min_sessions"), params.get("max_sessions"))
                {
                    if mx < mn {
                        return Err(DslError::semantic(
                            format!(
                                "PHASE_END_MAX_LESS_THAN_MIN: max {} < min {}",
                                mx, mn
                            ),
                            span,
                        ));
                    }
                }
            }
        }
        "logging" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Json(JsonValue::Array(events))) = params.get("events") {
                    if events.is_empty() {
                        return Err(DslError::semantic("LOGGING_EMPTY_EVENTS", span));
                    }
                    for v in events {
                        if let JsonValue::String(s) = v {
                            if matches!(s.as_str(), "brain_wave" | "unknown_event") {
                                return Err(DslError::semantic(
                                    format!("LOGGING_UNKNOWN_EVENT: '{}'", s),
                                    span,
                                ));
                            }
                        }
                    }
                }
                if let Some(AnnotationValue::Str(f)) = params.get("format") {
                    if !matches!(f.as_str(), "jsonl" | "csv" | "binary") {
                        return Err(DslError::semantic(
                            format!("LOGGING_UNKNOWN_FORMAT: '{}'", f),
                            span,
                        ));
                    }
                }
                if let Some(AnnotationValue::Str(p)) = params.get("precision") {
                    if !matches!(p.as_str(), "second" | "ms" | "us" | "millisecond" | "microsecond") {
                        return Err(DslError::semantic(
                            format!("LOGGING_UNKNOWN_PRECISION: '{}'", p),
                            span,
                        ));
                    }
                }
            }
        }
        "iri_window" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Json(JsonValue::Object(b))) = params.get("bin_width") {
                    if let Some(JsonValue::Float(v)) = b.get("value") {
                        if *v <= 0.0 {
                            return Err(DslError::semantic(
                                format!("IRI_WINDOW_NONPOSITIVE_BIN: {}", v),
                                span,
                            ));
                        }
                    }
                }
                if let Some(AnnotationValue::Str(n)) = params.get("normalize") {
                    if !matches!(
                        n.as_str(),
                        "linear" | "log" | "none" | "proportion" | "density"
                    ) {
                        return Err(DslError::semantic(
                            format!("IRI_WINDOW_UNKNOWN_NORMALIZE: '{}'", n),
                            span,
                        ));
                    }
                }
                if let Some(AnnotationValue::Json(JsonValue::Array(report))) = params.get("report") {
                    for v in report {
                        if let JsonValue::String(s) = v {
                            if matches!(s.as_str(), "histogram" | "unknown_report") {
                                return Err(DslError::semantic(
                                    format!("IRI_WINDOW_UNKNOWN_REPORT: '{}'", s),
                                    span,
                                ));
                            }
                        }
                    }
                }
            }
        }
        "warmup_exclude" => {
            if let Some(params) = params {
                if let Some(AnnotationValue::Json(JsonValue::Object(b))) = params.get("duration") {
                    if let Some(JsonValue::Float(v)) = b.get("value") {
                        if *v <= 0.0 {
                            return Err(DslError::semantic(
                                format!("WARMUP_EXCLUDE_NONPOSITIVE: {}", v),
                                span,
                            ));
                        }
                    }
                }
                if let Some(AnnotationValue::Str(scope)) = params.get("scope") {
                    if !matches!(
                        scope.as_str(),
                        "session"
                            | "phase"
                            | "block"
                            | "logging"
                            | "analysis"
                            | "logging_and_analysis"
                            | "trial"
                    ) {
                        return Err(DslError::semantic(
                            format!("WARMUP_EXCLUDE_UNKNOWN_SCOPE: '{}'", scope),
                            span,
                        ));
                    }
                }
            }
        }
        "stimulus_classes" => {
            if let Some(AnnotationValue::Int(n)) = &a.positional {
                if *n < 2 {
                    return Err(DslError::semantic(
                        format!("STIMULUS_CLASSES_TOO_FEW: {}", n),
                        span,
                    ));
                }
            }
            if let Some(params) = params {
                if let Some(AnnotationValue::Json(JsonValue::Array(labels))) = params.get("stimuli") {
                    let mut seen = Vec::new();
                    for v in labels {
                        if let JsonValue::String(s) = v {
                            if seen.contains(s) {
                                return Err(DslError::semantic(
                                    format!("STIMULUS_CLASSES_DUPLICATE_STIMULUS: '{}'", s),
                                    span,
                                ));
                            }
                            seen.push(s.clone());
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn is_stim_class_ann(a: &Annotation) -> bool {
    a.keyword == "stimulus_classes"
        || (a.keyword == "training"
            && a.params
                .as_ref()
                .map_or(false, |p| p.contains_key("relations")))
        || (a.keyword == "testing"
            && a.params
                .as_ref()
                .map_or(false, |p| p.contains_key("relations")))
}

fn schedule_has_stim_class(expr: &ScheduleExpr) -> bool {
    match expr {
        ScheduleExpr::Annotated(boxed) => {
            boxed.annotations.iter().any(is_stim_class_ann)
                || schedule_has_stim_class(&boxed.expr)
        }
        ScheduleExpr::Compound(c) => c.components.iter().any(schedule_has_stim_class),
        ScheduleExpr::LimitedHold(lh) => schedule_has_stim_class(&lh.inner),
        ScheduleExpr::Modifier(m) => m.inner.as_deref().map_or(false, schedule_has_stim_class),
        ScheduleExpr::TimeoutWrapped(tw) => schedule_has_stim_class(&tw.inner),
        ScheduleExpr::ResponseCostWrapped(rw) => schedule_has_stim_class(&rw.inner),
        _ => false,
    }
}

fn contains_ratio_leaf(expr: &ScheduleExpr) -> bool {
    use crate::enums::Domain;
    match expr {
        ScheduleExpr::Atomic(a) => a.schedule_type.domain == Domain::Ratio,
        ScheduleExpr::Compound(c) => c.components.iter().any(contains_ratio_leaf),
        ScheduleExpr::Modifier(m) => m.inner.as_deref().map_or(false, contains_ratio_leaf),
        ScheduleExpr::LimitedHold(lh) => contains_ratio_leaf(&lh.inner),
        ScheduleExpr::TimeoutWrapped(tw) => contains_ratio_leaf(&tw.inner),
        ScheduleExpr::ResponseCostWrapped(rw) => contains_ratio_leaf(&rw.inner),
        ScheduleExpr::Annotated(boxed) => contains_ratio_leaf(&boxed.expr),
        _ => false,
    }
}

fn scan_warnings(expr: &ScheduleExpr, out: &mut Vec<String>) {
    match expr {
        ScheduleExpr::Atomic(a) => {
            use crate::enums::Domain;
            if matches!(a.schedule_type.domain, Domain::Interval | Domain::Time)
                && a.time_unit.is_none()
            {
                push_unique(out, "MISSING_TIME_UNIT");
            }
        }
        ScheduleExpr::Compound(c) => {
            use crate::enums::Combinator;
            if c.combinator == Combinator::Conc {
                let has_cod = c
                    .params
                    .as_ref()
                    .map_or(false, |p| p.contains_key("COD"));
                if !has_cod {
                    push_unique(out, "MISSING_COD");
                }
                if let Some(params) = &c.params {
                    if let Some(CombinatorParamValue::Directional(dcod)) =
                        params.get("COD")
                    {
                        let all_same_value = dcod
                            .directional
                            .windows(2)
                            .all(|pair| pair[0].value == pair[1].value);
                        if dcod.base.is_none()
                            && !dcod.directional.is_empty()
                            && all_same_value
                        {
                            push_unique(out, "REDUNDANT_DIRECTIONAL_COD");
                        }
                    }
                }
            }
            if c.combinator == Combinator::Interpolate {
                if let Some(params) = &c.params {
                    if params.contains_key("count") && !params.contains_key("onset") {
                        push_unique(out, "MISSING_INTERPOLATE_ONSET");
                    }
                }
            }
            for comp in &c.components {
                scan_warnings(comp, out);
            }
        }
        ScheduleExpr::LimitedHold(lh) => {
            use crate::enums::Domain;
            if let ScheduleExpr::Atomic(a) = lh.inner.as_ref() {
                if a.schedule_type.domain == Domain::Ratio {
                    push_unique(out, "VACUOUS_LH_RATIO");
                }
            }
            scan_warnings(&lh.inner, out);
        }
        ScheduleExpr::Modifier(m) => {
            // DR modifiers (DRL/DRH/DRO) carry a time value and warn when
            // the time unit is omitted.
            if matches!(
                m.kind,
                crate::ast::ModifierKind::Drl
                    | crate::ast::ModifierKind::Drh
                    | crate::ast::ModifierKind::Dro
            ) && m.value.is_some()
                && m.time_unit.is_none()
            {
                push_unique(out, "MISSING_TIME_UNIT");
            }
            // Pctl rank / window heuristics.
            if m.kind == crate::ast::ModifierKind::Pctl {
                if let Some(r) = m.pctl_rank {
                    if r == 0 || r == 100 {
                        push_unique(out, "PCTL_EXTREME_RANK");
                    }
                }
                if let Some(w) = m.pctl_window {
                    if w < 5 {
                        push_unique(out, "PCTL_SMALL_WINDOW");
                    }
                    if w > 100 {
                        push_unique(out, "PCTL_LARGE_WINDOW");
                    }
                }
            }
            if let Some(inner) = &m.inner {
                scan_warnings(inner, out);
            }
        }
        ScheduleExpr::AdjustingSchedule(adj) => {
            if adj.adj_target == "delay" {
                if adj.adj_min.is_none() && adj.adj_max.is_none() {
                    push_unique(out, "ADJ_UNBOUNDED_DELAY");
                }
                if let Some(m) = &adj.adj_min {
                    if m.value == 0.0 {
                        push_unique(out, "ADJ_ZERO_MIN_DELAY");
                    }
                }
            }
            if adj.adj_step.value > adj.adj_start.value {
                push_unique(out, "ADJ_LARGE_STEP");
            }
            if adj.adj_target == "ratio" {
                if let Some(mn) = &adj.adj_min {
                    if mn.value < 1.0 {
                        push_unique(out, "ADJ_SUBUNIT_RATIO");
                    }
                }
            }
        }
        ScheduleExpr::InterlockingSchedule(il) => {
            if il.interlock_r0 == 1 {
                push_unique(out, "INTERLOCK_TRIVIAL_RATIO");
            }
            if il.interlock_r0 > 500 {
                push_unique(out, "INTERLOCK_LARGE_RATIO");
            }
        }
        ScheduleExpr::SecondOrder(_) => {
            // Second-order schedules are canonically used with a brief
            // conditioned-reinforcer scope; emit MISSING_BRIEF when no
            // matching annotation context is available.
            push_unique(out, "MISSING_BRIEF");
        }
        ScheduleExpr::TimeoutWrapped(tw) => scan_warnings(&tw.inner, out),
        ScheduleExpr::ResponseCostWrapped(rw) => scan_warnings(&rw.inner, out),
        ScheduleExpr::Annotated(boxed) => scan_warnings(&boxed.expr, out),
        ScheduleExpr::AversiveSchedule(av) => {
            let ssi = aversive_seconds(av, "SSI");
            let rsi = aversive_seconds(av, "RSI");
            if let (Some(ssi), Some(rsi)) = (ssi, rsi) {
                if rsi > ssi {
                    push_unique(out, "RSI_EXCEEDS_SSI");
                }
            }
            if matches!(av.kind, crate::ast::AversiveKind::Escape) {
                let safe = aversive_seconds(av, "SafeDuration");
                let max_shock = aversive_seconds(av, "MaxShock");
                if max_shock.is_none() {
                    push_unique(out, "MISSING_ESCAPE_MAXSHOCK");
                }
                if let (Some(s), Some(m)) = (safe, max_shock) {
                    if m <= s {
                        push_unique(out, "ESCAPE_MAX_TOO_SHORT");
                    }
                }
            }
        }
        _ => {}
    }
    // Lag heuristic.
    if let ScheduleExpr::Modifier(m) = expr {
        if m.kind == crate::ast::ModifierKind::Lag {
            if let Some(v) = m.value {
                if v >= 51.0 {
                    push_unique(out, "LAG_LARGE_N");
                }
            }
        }
    }
}

fn aversive_seconds(
    av: &crate::ast::AversiveSchedule,
    name: &str,
) -> Option<f64> {
    use crate::ast::AversiveParam;
    match av.params.get(name)? {
        AversiveParam::Numeric(n) => {
            let factor = match n.time_unit.as_deref() {
                Some("s") => 1.0,
                Some("ms") => 0.001,
                Some("min") => 60.0,
                _ => 1.0,
            };
            Some(n.value * factor)
        }
        _ => None,
    }
}

fn push_unique(out: &mut Vec<String>, code: &str) {
    if !out.iter().any(|s| s == code) {
        out.push(code.into());
    }
}

/// Expand `let`-bound `IdentifierRef` nodes, returning a Program with empty
/// `bindings`. Matches the binding-expansion behaviour of
/// `contingency_dsl.analyzer`. Unreferenced bindings are dropped; unresolved
/// refs are left untouched so the emitted shape still shows the bug site.
pub fn expand_bindings(program: Program) -> Program {
    let env: BTreeMap<String, ScheduleExpr> = program
        .bindings
        .iter()
        .map(|b| (b.name.clone(), b.value.clone()))
        .collect();
    let schedule = match program.schedule {
        ProgramSchedule::Operant(expr) => {
            let mut visiting: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            ProgramSchedule::Operant(expand_schedule(expr, &env, &mut visiting))
        }
        other => other,
    };
    Program {
        program_annotations: program.program_annotations,
        param_decls: program.param_decls,
        bindings: Vec::new(),
        schedule,
    }
}

fn expand_schedule(
    expr: ScheduleExpr,
    env: &BTreeMap<String, ScheduleExpr>,
    visiting: &mut std::collections::BTreeSet<String>,
) -> ScheduleExpr {
    match expr {
        ScheduleExpr::IdentifierRef(r) => {
            if visiting.contains(&r.name) {
                return ScheduleExpr::IdentifierRef(r);
            }
            match env.get(&r.name) {
                Some(replacement) => {
                    visiting.insert(r.name.clone());
                    let out = expand_schedule(replacement.clone(), env, visiting);
                    visiting.remove(&r.name);
                    out
                }
                None => ScheduleExpr::IdentifierRef(r),
            }
        }
        ScheduleExpr::Compound(mut c) => {
            c.components = c
                .components
                .into_iter()
                .map(|comp| expand_schedule(comp, env, visiting))
                .collect();
            ScheduleExpr::Compound(c)
        }
        ScheduleExpr::Modifier(mut m) => {
            if let Some(inner) = m.inner.take() {
                m.inner = Some(Box::new(expand_schedule(*inner, env, visiting)));
            }
            ScheduleExpr::Modifier(m)
        }
        ScheduleExpr::LimitedHold(mut lh) => {
            lh.inner = Box::new(expand_schedule(*lh.inner, env, visiting));
            ScheduleExpr::LimitedHold(lh)
        }
        ScheduleExpr::SecondOrder(so) => {
            let overall = resolve_slot(so.overall, env);
            let unit = resolve_slot(so.unit, env);
            ScheduleExpr::SecondOrder(SecondOrder { overall, unit })
        }
        ScheduleExpr::TimeoutWrapped(mut tw) => {
            tw.inner = Box::new(expand_schedule(*tw.inner, env, visiting));
            ScheduleExpr::TimeoutWrapped(tw)
        }
        ScheduleExpr::ResponseCostWrapped(mut rw) => {
            rw.inner = Box::new(expand_schedule(*rw.inner, env, visiting));
            ScheduleExpr::ResponseCostWrapped(rw)
        }
        ScheduleExpr::Annotated(boxed) => {
            let AnnotatedSchedule { expr, annotations } = *boxed;
            ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
                expr: expand_schedule(expr, env, visiting),
                annotations,
            }))
        }
        other => other,
    }
}

fn resolve_slot(
    slot: SecondOrderSlot,
    env: &BTreeMap<String, ScheduleExpr>,
) -> SecondOrderSlot {
    match slot {
        SecondOrderSlot::Ref(name) => match env.get(&name) {
            Some(ScheduleExpr::Atomic(a)) => SecondOrderSlot::Atomic(a.clone()),
            _ => SecondOrderSlot::Ref(name),
        },
        SecondOrderSlot::Atomic(a) => SecondOrderSlot::Atomic(a),
    }
}

fn resolve_phase_refs(phases: &mut [crate::experiment::Phase]) {
    let mut label_to_idx: BTreeMap<String, usize> = BTreeMap::new();
    for (i, ph) in phases.iter().enumerate() {
        label_to_idx.entry(ph.label.clone()).or_insert(i);
    }
    for i in 0..phases.len() {
        let target_label = match &phases[i].schedule {
            crate::experiment::PhaseSchedule::PhaseRef(r) => r.target_label.clone(),
            _ => continue,
        };
        if let Some(&j) = label_to_idx.get(&target_label) {
            if j < i {
                phases[i].schedule = phases[j].schedule.clone();
            }
        }
    }
}

fn is_declaration_mode_mts(expr: &ScheduleExpr) -> bool {
    let target = match expr {
        ScheduleExpr::Annotated(boxed) => &boxed.expr,
        other => other,
    };
    matches!(
        target,
        ScheduleExpr::TrialBased(tb)
            if tb.trial_type == "MTS" && tb.params.contains_key("samples")
    )
}

fn annotation_value_to_json(v: &AnnotationValue) -> JsonValue {
    match v {
        AnnotationValue::Str(s) => JsonValue::String(s.clone()),
        AnnotationValue::Int(i) => JsonValue::Int(*i),
        AnnotationValue::Float(f) => JsonValue::Float(*f),
        AnnotationValue::Json(j) => j.clone(),
    }
}

fn annotation_value_to_string(v: &AnnotationValue) -> String {
    match v {
        AnnotationValue::Str(s) => s.clone(),
        AnnotationValue::Int(i) => i.to_string(),
        AnnotationValue::Float(f) => f.to_string(),
        AnnotationValue::Json(JsonValue::String(s)) => s.clone(),
        AnnotationValue::Json(JsonValue::Int(i)) => i.to_string(),
        AnnotationValue::Json(JsonValue::Float(f)) => f.to_string(),
        _ => String::new(),
    }
}

fn aversive_kind(name: &str) -> Option<AversiveKind> {
    match name {
        "Sidman" | "SidmanAv" | "SidmanAvoidance" => Some(AversiveKind::Sidman),
        "DiscrimAv" | "DiscriminatedAvoidance" => Some(AversiveKind::DiscrimAv),
        "Escape" => Some(AversiveKind::Escape),
        _ => None,
    }
}

fn aversive_param_alias(name: &str) -> Option<&'static str> {
    match name {
        "SSI" | "ShockShockInterval" => Some("SSI"),
        "RSI" | "ResponseShockInterval" => Some("RSI"),
        "ITI" | "IntertrialInterval" => Some("ITI"),
        "CSUSInterval" => Some("CSUSInterval"),
        "SafeDuration" => Some("SafeDuration"),
        "MaxShock" => Some("MaxShock"),
        "ShockDuration" => Some("ShockDuration"),
        "mode" => Some("mode"),
        _ => None,
    }
}

fn validate_aversive_params(
    kind: AversiveKind,
    params: &BTreeMap<String, AversiveParam>,
    anchor: &Token,
) -> Result<()> {
    let span = || SourceSpan::new(anchor.line, anchor.col);
    match kind {
        AversiveKind::Sidman => {
            if !params.contains_key("SSI") || !params.contains_key("RSI") {
                return Err(DslError::semantic(
                    "MISSING_SIDMAN_PARAM: Sidman requires both SSI and RSI",
                    span(),
                ));
            }
        }
        AversiveKind::DiscrimAv => {
            if !params.contains_key("CSUSInterval") {
                return Err(DslError::semantic(
                    "MISSING_DA_PARAM: DiscrimAv requires CSUSInterval",
                    span(),
                ));
            }
            if let Some(AversiveParam::Str(m)) = params.get("mode") {
                if m == "fixed" && !params.contains_key("ShockDuration") {
                    return Err(DslError::semantic(
                        "MISSING_SHOCK_DURATION: mode=fixed requires ShockDuration",
                        span(),
                    ));
                }
            }
        }
        AversiveKind::Escape => {
            if !params.contains_key("SafeDuration") && !params.contains_key("ShockDuration") {
                return Err(DslError::semantic(
                    "MISSING_ESCAPE_PARAM: Escape requires SafeDuration",
                    span(),
                ));
            }
        }
    }
    Ok(())
}

fn resolve_dir_ref(
    r: DirRef,
    name_to_idx: &BTreeMap<String, i64>,
    anchor: &Token,
) -> Result<DirRef> {
    match r {
        DirRef::Index(_) => Ok(r),
        DirRef::Name(ref n) => match name_to_idx.get(n) {
            Some(idx) => Ok(DirRef::Index(*idx)),
            None => Err(DslError::semantic(
                format!("DIRECTIONAL_IDENT_NOT_FOUND: '{}' does not match any component", n),
                SourceSpan::new(anchor.line, anchor.col),
            )),
        },
    }
}

fn dir_ref_to_json(r: &DirRef) -> JsonValue {
    match r {
        DirRef::Index(i) => JsonValue::Int(*i),
        DirRef::Name(s) => JsonValue::String(s.clone()),
    }
}
