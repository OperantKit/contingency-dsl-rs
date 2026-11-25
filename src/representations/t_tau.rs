//! T-τ representation (Schoenfeld & Cole, 1972).
//!
//! A time-based schedule is described as a recurring cycle of `T` seconds in
//! which a window of `τ` seconds permits reinforcement.
//!
//! | Conversion | Rule |
//! |---|---|
//! | [`to_ttau`] | `FI/VI/RI/FT/VT/RT (t)` to `T = τ = t` |
//! | [`from_ttau`] | `τ = T` to an atomic schedule; `τ < T` to the same with a limited hold of `τ` |
//! | [`approximate_ttau`] | `FR/VR/RR (n)` to `T = τ = n · IRT`, in a type of its own |
//!
//! Ratio schedules count responses. Turning that count into time needs a
//! response rate, which is a dependent variable, so [`to_ttau`] and
//! [`from_ttau`] reject the Ratio domain.
//!
//! `T` and `τ` do not say whether reinforcement depends on a response, nor
//! how the values vary between cycles; [`from_ttau`] therefore takes the
//! domain and the distribution. For `τ < T` the result is a structural
//! approximation: the τ window opens at the start of the cycle, a limited
//! hold after the interval has elapsed.
//!
//! Reference: Schoenfeld, W. N., & Cole, B. K. (1972). *Stimulus schedules:
//! The t-τ systems*. Harper & Row.

use std::fmt;

use crate::ast::{
    AnnotatedSchedule, Annotation, Atomic, LimitedHold, ScheduleExpr, ScheduleType,
};
use crate::enums::{Distribution, Domain};

/// Why a conversion was rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum TTauError {
    /// The schedule, or the target domain, is Ratio.
    RatioSchedule(ScheduleType),
    /// A number is outside its range. The message names it.
    Value(String),
    /// The schedule is of a kind the conversion does not take.
    Type(String),
}

impl TTauError {
    /// The error name the conformance cases use.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::RatioSchedule(_) => "RatioScheduleError",
            Self::Value(_) => "ValueError",
            Self::Type(_) => "TypeError",
        }
    }
}

impl fmt::Display for TTauError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RatioSchedule(st) => write!(
                f,
                "Ratio schedule {}{} cannot be converted to T-tau: response count \
                 is not reducible to time without assuming a response rate",
                st.distribution.code(),
                st.domain.code()
            ),
            Self::Value(message) | Self::Type(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TTauError {}

/// T-τ parameters in seconds, with `0 < τ ≤ T`.
///
/// The fields are private so that a value that breaks the constraint cannot
/// be built.
#[derive(Debug, Clone, PartialEq)]
pub struct TTauParams {
    t: f64,
    tau: f64,
    annotations: Vec<Annotation>,
}

impl TTauParams {
    /// `T` and `τ` must be finite.
    pub fn new(t: f64, tau: f64) -> Result<Self, TTauError> {
        if !is_positive(t) {
            return Err(TTauError::Value(format!("T must be > 0 and finite, got {t}")));
        }
        if !is_positive(tau) || tau > t {
            return Err(TTauError::Value(format!(
                "tau must be in (0, T], got tau={tau}, T={t}"
            )));
        }
        Ok(Self { t, tau, annotations: Vec::new() })
    }

    /// Replace the annotations. They are carried through a conversion and are
    /// not part of the T-τ coordinates.
    pub fn with_annotations(mut self, annotations: Vec<Annotation>) -> Self {
        self.annotations = annotations;
        self
    }

    /// Cycle duration.
    pub fn t(&self) -> f64 {
        self.t
    }

    /// Reinforcement window.
    pub fn tau(&self) -> f64 {
        self.tau
    }

    pub fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    /// `τ / T`, in `(0, 1]`.
    pub fn duty_cycle(&self) -> f64 {
        self.tau / self.t
    }
}

/// Assumed mean inter-response time in seconds. A dependent variable, kept
/// apart from the schedule.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IrtAssumption {
    pub irt: f64,
}

/// An approximation that holds under an [`IrtAssumption`]. It is a type of
/// its own, and there is no conversion from it to [`TTauParams`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TTauApprox {
    t: f64,
    tau: f64,
    assumption: IrtAssumption,
}

impl TTauApprox {
    pub fn t(&self) -> f64 {
        self.t
    }

    pub fn tau(&self) -> f64 {
        self.tau
    }

    pub fn assumption(&self) -> IrtAssumption {
        self.assumption
    }

    pub fn duty_cycle(&self) -> f64 {
        self.tau / self.t
    }
}

/// Convert an atomic Interval or Time schedule, with or without annotations.
///
/// `T` and `τ` are in seconds. A schedule without a time unit is read as
/// seconds, and one in another unit is rejected: the parameters do not carry
/// a unit, so its value would be mistaken for seconds. Anything but an atomic
/// schedule is rejected, a limited hold included.
pub fn to_ttau(schedule: &ScheduleExpr) -> Result<TTauParams, TTauError> {
    let (expr, annotations) = match schedule {
        ScheduleExpr::Annotated(a) => (&a.expr, a.annotations.clone()),
        other => (other, Vec::new()),
    };
    let ScheduleExpr::Atomic(atomic) = expr else {
        return Err(TTauError::Type(
            "to_ttau takes an atomic schedule, or an atomic schedule with annotations".into(),
        ));
    };
    if atomic.schedule_type.domain == Domain::Ratio {
        return Err(TTauError::RatioSchedule(atomic.schedule_type));
    }
    if let Some(unit) = atomic.time_unit.as_deref().filter(|u| *u != "s") {
        return Err(TTauError::Value(format!(
            "T-tau parameters are in seconds, got a schedule in {unit}"
        )));
    }
    require_positive_schedule(atomic)?;
    Ok(TTauParams::new(atomic.value, atomic.value)?.with_annotations(annotations))
}

/// Convert back to a schedule. The result has no time unit.
///
/// `τ = T` gives an atomic schedule of `T`, `τ < T` the same schedule with a
/// limited hold of `τ`. Annotations wrap the whole result.
pub fn from_ttau(
    params: &TTauParams,
    domain: Domain,
    distribution: Distribution,
) -> Result<ScheduleExpr, TTauError> {
    let schedule_type = ScheduleType::new(distribution, domain);
    if domain == Domain::Ratio {
        return Err(TTauError::RatioSchedule(schedule_type));
    }
    let mut expr = ScheduleExpr::Atomic(Atomic {
        schedule_type,
        value: params.t,
        time_unit: None,
    });
    if params.tau < params.t {
        expr = ScheduleExpr::LimitedHold(LimitedHold {
            hold_duration: params.tau,
            inner: Box::new(expr),
            time_unit: None,
        });
    }
    if params.annotations.is_empty() {
        return Ok(expr);
    }
    Ok(ScheduleExpr::Annotated(Box::new(AnnotatedSchedule {
        expr,
        annotations: params.annotations.clone(),
    })))
}

/// Approximate a Ratio schedule under a declared mean inter-response time in
/// seconds. Other domains are rejected: [`to_ttau`] converts those without an
/// assumption. It takes the atomic schedule alone, without annotations.
pub fn approximate_ttau(schedule: &Atomic, irt: f64) -> Result<TTauApprox, TTauError> {
    if schedule.schedule_type.domain != Domain::Ratio {
        return Err(TTauError::Type("approximate_ttau takes a Ratio schedule".into()));
    }
    if !is_positive(irt) {
        return Err(TTauError::Value(format!("irt must be > 0 and finite, got {irt}")));
    }
    require_positive_schedule(schedule)?;
    let t = schedule.value * irt;
    if !is_positive(t) {
        return Err(TTauError::Value(format!(
            "The approximated cycle must be positive and finite, got {t}"
        )));
    }
    Ok(TTauApprox { t, tau: t, assumption: IrtAssumption { irt } })
}

fn require_positive_schedule(schedule: &Atomic) -> Result<(), TTauError> {
    if is_positive(schedule.value) {
        return Ok(());
    }
    Err(TTauError::Value(format!(
        "Schedule value must be positive and finite, got {}",
        schedule.value
    )))
}

/// False for NaN and infinity too.
fn is_positive(x: f64) -> bool {
    x > 0.0 && x.is_finite()
}
