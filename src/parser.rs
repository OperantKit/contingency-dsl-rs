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
    Atomic, Binding, CombinatorParamValue, Compound, CompoundParam, DirRef, DirectionalCOD,
    DirectionalEntry, IdentifierRef, JsonValue, LimitedHold, Modifier, ModifierKind, ParamDecl,
    Program, ProgramSchedule, PunishParam, ScheduleExpr, ScheduleType, SecondOrder, Special,
    SpecialKind,
};
use crate::enums::{Combinator, Distribution, Domain, KW_ARG_KEYWORDS, LOWERCASE_RESERVED};
use crate::errors::{DslError, Result, SourceSpan};
use crate::lexer::tokenize;
use crate::serializer::expr_to_json;
use crate::tokens::{Token, TokenType};

pub fn parse(source: &str) -> Result<Program> {
    let tokens = tokenize(source)?;
    let mut p = Parser::new(tokens);
    p.parse_program()
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn peek_at(&self, offset: usize) -> Option<&Token> {
        self.tokens.get(self.pos + offset)
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        self.pos += 1;
        t
    }

    fn at(&self, ty: TokenType) -> bool {
        self.peek().ty == ty
    }

    fn at_any(&self, types: &[TokenType]) -> bool {
        let cur = self.peek().ty;
        types.iter().any(|t| *t == cur)
    }

    fn expect(&mut self, ty: TokenType, ctx: &str) -> Result<Token> {
        if self.peek().ty != ty {
            let t = self.peek();
            return Err(DslError::parse(
                format!(
                    "expected {:?} ({}), got {:?} '{}'",
                    ty, ctx, t.ty, t.value
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

    fn parse_program(&mut self) -> Result<Program> {
        self.skip_newlines();

        if self.at(TokenType::At) {
            return Err(DslError::parse(
                "annotations not yet supported by rs parser",
                SourceSpan::new(self.peek().line, self.peek().col),
            ));
        }

        if self.at_any(&[
            TokenType::Experiment,
            TokenType::Phase,
            TokenType::Shaping,
            TokenType::Progressive,
        ]) {
            return Err(DslError::parse(
                "experiment-layer blocks not yet supported by rs parser",
                SourceSpan::new(self.peek().line, self.peek().col),
            ));
        }

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

        let mut bindings = Vec::new();
        while self.at(TokenType::Let) {
            bindings.push(self.parse_binding()?);
        }

        // Empty-program special case: after bindings, if we're at EOF, default
        // schedule is EXT (matches py fixture `program_minimal`).
        let schedule = if self.at(TokenType::Eof) {
            ScheduleExpr::Special(Special { kind: SpecialKind::Ext })
        } else {
            self.parse_schedule()?
        };

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

        Ok(Program {
            program_annotations: Vec::new(),
            param_decls,
            bindings,
            schedule: ProgramSchedule::Operant(schedule),
        })
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
        let (val, unit) = self.parse_value()?;
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
        let base = self.parse_base_schedule()?;
        // Postfix LH?
        if self.at(TokenType::Lh) {
            if let Some(next) = self.peek_at(1) {
                if next.ty == TokenType::Equals {
                    return Ok(base);
                }
            }
            self.advance();
            if matches!(base, ScheduleExpr::Compound(_)) {
                let t = self.peek();
                return Err(DslError::parse(
                    "LH_ON_COMPOUND: LH applies to leaf schedules only",
                    SourceSpan::new(t.line, t.col),
                ));
            }
            let (hv, hu) = self.parse_value()?;
            return Ok(ScheduleExpr::LimitedHold(LimitedHold {
                hold_duration: hv,
                inner: Box::new(base),
                time_unit: hu,
            }));
        }
        Ok(base)
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
            TokenType::Repeat => self.parse_repeat_call(),
            TokenType::ScheduleType => self.parse_atomic_or_second(),
            TokenType::Ext => {
                self.advance();
                Ok(ScheduleExpr::Special(Special { kind: SpecialKind::Ext }))
            }
            TokenType::Crf => {
                self.advance();
                Ok(ScheduleExpr::Special(Special { kind: SpecialKind::Crf }))
            }
            TokenType::Number => {
                // `N*schedule` shorthand for Repeat.
                self.parse_repeat_shorthand()
            }
            TokenType::Identifier => {
                let tok = self.advance();
                let r = ScheduleExpr::IdentifierRef(IdentifierRef { name: tok.value });
                if self.at(TokenType::LParen) {
                    self.advance();
                    let unit = self.parse_second_order_unit()?;
                    self.expect(TokenType::RParen, "second-order closing paren")?;
                    // IdentifierRef as overall is not wrapped in Atomic; we
                    // emit a SecondOrder whose overall will be resolved by the
                    // analyzer. For now we only accept the overall as an
                    // Atomic-compatible identifier by keeping it via the
                    // IdentifierRef (matches py AST `SecondOrder(overall=ref)`).
                    // To avoid a wider AST change in this pass, reject the
                    // identifier form for now.
                    let _ = r;
                    let _ = unit;
                    return Err(DslError::parse(
                        "second-order with identifier overall not yet supported by rs parser",
                        SourceSpan::new(tok.line, tok.col),
                    ));
                }
                Ok(r)
            }
            _ => {
                let t = self.peek();
                Err(DslError::parse(
                    format!("unexpected token at start of schedule: {:?} '{}'", t.ty, t.value),
                    SourceSpan::new(t.line, t.col),
                ))
            }
        }
    }

    fn parse_atomic_or_second(&mut self) -> Result<ScheduleExpr> {
        let atomic = self.parse_parametric_atomic()?;
        if self.at(TokenType::LParen) {
            self.advance();
            let unit = self.parse_second_order_unit()?;
            self.expect(TokenType::RParen, "second-order closing paren")?;
            // py restricts unit to Atomic; we enforce the same.
            let unit_atomic = match unit {
                ScheduleExpr::Atomic(a) => a,
                _ => {
                    let t = self.peek();
                    return Err(DslError::parse(
                        "second-order unit must be Atomic",
                        SourceSpan::new(t.line, t.col),
                    ));
                }
            };
            return Ok(ScheduleExpr::SecondOrder(SecondOrder { overall: atomic, unit: unit_atomic }));
        }
        Ok(ScheduleExpr::Atomic(atomic))
    }

    fn parse_parametric_atomic(&mut self) -> Result<Atomic> {
        let tok = self.expect(TokenType::ScheduleType, "schedule type")?;
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
        Ok(Atomic {
            schedule_type: ScheduleType::new(dist, domain),
            value: val,
            time_unit: unit,
        })
    }

    fn parse_second_order_unit(&mut self) -> Result<ScheduleExpr> {
        let t = self.peek();
        if t.ty == TokenType::Identifier {
            if let Some(next) = self.peek_at(1) {
                if next.ty != TokenType::LParen {
                    let tok = self.advance();
                    return Ok(ScheduleExpr::IdentifierRef(IdentifierRef { name: tok.value }));
                }
            } else {
                let tok = self.advance();
                return Ok(ScheduleExpr::IdentifierRef(IdentifierRef { name: tok.value }));
            }
        }
        let atomic = self.parse_parametric_atomic()?;
        Ok(ScheduleExpr::Atomic(atomic))
    }

    fn parse_value(&mut self) -> Result<(f64, Option<String>)> {
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
                format!("{} requires at least 2 components", tok.value),
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
                                return Err(DslError::parse(
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
                                    return Err(DslError::parse(
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
                                        return Err(DslError::parse(
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
                        return Err(DslError::parse(
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
                    let (val, unit) = self.parse_value()?;
                    if params.contains_key(&canonical) {
                        return Err(DslError::parse(
                            format!("duplicate keyword argument '{}'", canonical),
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
                    return Err(DslError::parse(
                        format!("duplicate keyword argument '{}'", kw_tok.value),
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
            return Err(DslError::parse(
                format!(
                    "INTERPOLATE_REQUIRES_TWO: got {} components",
                    components.len()
                ),
                SourceSpan::new(tok.line, tok.col),
            ));
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
        let (val, unit) = self.parse_value()?;
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

    fn parse_repeat_call(&mut self) -> Result<ScheduleExpr> {
        self.expect(TokenType::Repeat, "Repeat")?;
        self.expect(TokenType::LParen, "Repeat opening paren")?;
        let num_tok = self.expect(TokenType::Number, "repeat count")?;
        let count: f64 = num_tok.value.parse().map_err(|_| {
            DslError::parse(
                format!("invalid repeat count: {}", num_tok.value),
                SourceSpan::new(num_tok.line, num_tok.col),
            )
        })?;
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

fn resolve_dir_ref(
    r: DirRef,
    name_to_idx: &BTreeMap<String, i64>,
    anchor: &Token,
) -> Result<DirRef> {
    match r {
        DirRef::Index(_) => Ok(r),
        DirRef::Name(ref n) => match name_to_idx.get(n) {
            Some(idx) => Ok(DirRef::Index(*idx)),
            None => Err(DslError::parse(
                format!("directional ref '{}' does not match any component", n),
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
