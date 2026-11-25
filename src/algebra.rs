//! Simplification of schedule expressions by the rewrite rules of the
//! contingency-dsl operant theory (§2.2.5).
//!
//! | Rules | Rewrite |
//! |---|---|
//! | R1, R2 | `Alt(S, EXT)`, `Alt(EXT, S)` to `S` |
//! | R3, R4 | `Conj(S, CRF)`, `Conj(CRF, S)` to `S` |
//! | R5 | `Chain(CRF, S)` to `S` |
//! | R6, R7 | `Conj(S, EXT)`, `Conj(EXT, S)` to `EXT` |
//! | R8, R9 | `Alt(S, CRF)`, `Alt(CRF, S)` to `CRF` |
//! | R10 | `Repeat(1, S)` to `S` (`Repeat(n, S)` to `Tand(S, ..., S)`) |
//! | R13–R17, Theorem 2 | a nested compound of the same combinator is flattened |
//!
//! The order of components is kept, so `Conc(A, B)` and `Conc(B, A)` stay
//! two expressions (Theorem 1 is for the caller to apply). Parsing never
//! simplifies; a caller asks for it with [`simplify_algebra`].

use crate::ast::{
    AnnotatedSchedule, Compound, Modifier, ModifierKind, Program, ProgramSchedule, ScheduleExpr,
    Special, SpecialKind,
};
use crate::enums::Combinator;
use crate::parser::expand_bindings;

/// Expand the `let` bindings of `program`, then simplify its operant
/// schedule. Respondent schedules are returned as they are.
///
/// A compound with keyword arguments (`COD`, `BO`, `PUNISH`, ...) is neither
/// flattened nor collapsed, because the rules are stated without arguments;
/// its components are still simplified. Only a *leading* CRF is dropped from
/// a Chain: `Chain(S, CRF)` has no rule.
///
/// `Repeat(n, S)` becomes `n` copies of `S`, so the result grows with `n`.
pub fn simplify_algebra(program: Program) -> Program {
    let mut program = expand_bindings(program);
    if let ProgramSchedule::Operant(expr) = program.schedule {
        program.schedule = ProgramSchedule::Operant(simplify(expr));
    }
    program
}

fn simplify(expr: ScheduleExpr) -> ScheduleExpr {
    match expr {
        ScheduleExpr::Compound(mut c) => {
            c.components = c.components.into_iter().map(simplify).collect();
            rewrite_compound(c)
        }
        ScheduleExpr::Modifier(m) => simplify_modifier(m),
        ScheduleExpr::LimitedHold(mut lh) => {
            lh.inner = Box::new(simplify(*lh.inner));
            ScheduleExpr::LimitedHold(lh)
        }
        ScheduleExpr::Annotated(boxed) => {
            let AnnotatedSchedule { expr, annotations } = *boxed;
            ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
                expr: simplify(expr),
                annotations,
            }))
        }
        other => other,
    }
}

fn simplify_modifier(mut m: Modifier) -> ScheduleExpr {
    let Some(inner) = m.inner.take() else {
        return ScheduleExpr::Modifier(m);
    };
    let inner = simplify(*inner);
    match (m.kind, m.value) {
        (ModifierKind::Repeat, Some(1.0)) => inner,
        (ModifierKind::Repeat, Some(n)) if n > 1.0 => rewrite_compound(Compound {
            combinator: Combinator::Tand,
            components: vec![inner; n as usize],
            params: None,
        }),
        _ => {
            m.inner = Some(Box::new(inner));
            ScheduleExpr::Modifier(m)
        }
    }
}

/// Apply the rules to one compound whose components are already simplified.
fn rewrite_compound(mut c: Compound) -> ScheduleExpr {
    if has_params(&c) {
        return ScheduleExpr::Compound(c);
    }
    if is_associative(c.combinator) {
        c.components = flatten(c.combinator, c.components);
    }
    match c.combinator {
        Combinator::Alt => collapse(c, SpecialKind::Ext, SpecialKind::Crf),
        Combinator::Conj => collapse(c, SpecialKind::Crf, SpecialKind::Ext),
        Combinator::Chain => drop_leading_crf(c),
        _ => ScheduleExpr::Compound(c),
    }
}

/// Theorem 2 covers these seven. Overlay and Interpolate give each of their
/// two positions its own role.
fn is_associative(combinator: Combinator) -> bool {
    matches!(
        combinator,
        Combinator::Conc
            | Combinator::Alt
            | Combinator::Conj
            | Combinator::Chain
            | Combinator::Tand
            | Combinator::Mult
            | Combinator::Mix
    )
}

fn flatten(combinator: Combinator, components: Vec<ScheduleExpr>) -> Vec<ScheduleExpr> {
    let mut flat = Vec::with_capacity(components.len());
    for component in components {
        match component {
            ScheduleExpr::Compound(inner)
                if inner.combinator == combinator && !has_params(&inner) =>
            {
                flat.extend(inner.components)
            }
            other => flat.push(other),
        }
    }
    flat
}

/// Remove the identity element (Theorem 3); the annihilator (Theorem 4)
/// replaces the whole compound.
fn collapse(mut c: Compound, identity: SpecialKind, annihilator: SpecialKind) -> ScheduleExpr {
    if c.components.iter().any(|x| is_special(x, annihilator)) {
        return special(annihilator);
    }
    c.components.retain(|x| !is_special(x, identity));
    if c.components.is_empty() {
        return special(identity);
    }
    unwrap_single(c)
}

fn drop_leading_crf(mut c: Compound) -> ScheduleExpr {
    let leading = c
        .components
        .iter()
        .take_while(|x| is_special(x, SpecialKind::Crf))
        .count();
    // `Chain(CRF, CRF)` is `Chain(CRF, S)` with `S = CRF`: the last link stays.
    let droppable = leading.min(c.components.len().saturating_sub(1));
    c.components.drain(..droppable);
    unwrap_single(c)
}

fn unwrap_single(mut c: Compound) -> ScheduleExpr {
    if c.components.len() == 1 {
        c.components.remove(0)
    } else {
        ScheduleExpr::Compound(c)
    }
}

fn has_params(c: &Compound) -> bool {
    c.params.as_ref().is_some_and(|p| !p.is_empty())
}

fn is_special(expr: &ScheduleExpr, kind: SpecialKind) -> bool {
    matches!(expr, ScheduleExpr::Special(s) if s.kind == kind)
}

fn special(kind: SpecialKind) -> ScheduleExpr {
    ScheduleExpr::Special(Special { kind })
}
