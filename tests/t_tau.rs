//! Boundaries of the T-τ conversions that the cases in
//! `conformance/representations/t-tau/` do not reach.

use contingency_dsl::ast::{
    AnnotatedSchedule, Annotation, AnnotationValue, Atomic, ProgramSchedule, ScheduleExpr,
    ScheduleType,
};
use contingency_dsl::enums::{Distribution, Domain};
use contingency_dsl::{approximate_ttau, from_ttau, parse, to_ttau, TTauError, TTauParams};

fn atomic(distribution: Distribution, domain: Domain, value: f64, unit: Option<&str>) -> Atomic {
    Atomic {
        schedule_type: ScheduleType::new(distribution, domain),
        value,
        time_unit: unit.map(str::to_string),
    }
}

fn fixed(domain: Domain, value: f64) -> Atomic {
    atomic(Distribution::Fixed, domain, value, None)
}

fn pellet() -> Annotation {
    Annotation {
        keyword: "reinforcer".into(),
        positional: Some(AnnotationValue::Str("pellet".into())),
        params: None,
        label: Some("punisher".into()),
    }
}

fn annotated(expr: ScheduleExpr, annotations: Vec<Annotation>) -> ScheduleExpr {
    ScheduleExpr::Annotated(Box::new(AnnotatedSchedule { expr, annotations }))
}

fn parsed(source: &str) -> ScheduleExpr {
    match parse(source).unwrap_or_else(|e| panic!("{source}: {e}")).schedule {
        ProgramSchedule::Operant(expr) => expr,
        other => panic!("{source}: {other:?}"),
    }
}

#[track_caller]
fn assert_kind<T: std::fmt::Debug>(result: Result<T, TTauError>, kind: &str, fragment: &str) {
    let error = result.expect_err("should be rejected");
    assert_eq!(error.kind_str(), kind, "{error}");
    assert!(error.to_string().contains(fragment), "{error}");
}

#[test]
fn a_parsed_schedule_converts() {
    let params = to_ttau(&parsed("VI 45-s")).unwrap();
    assert_eq!((params.t(), params.tau()), (45.0, 45.0));
    assert_eq!(params.duty_cycle(), 1.0);
}

#[test]
fn a_limited_hold_comes_back_around_the_schedule() {
    let params = TTauParams::new(60.0, 30.0).unwrap();
    assert_eq!(params.duty_cycle(), 0.5);
    let schedule = from_ttau(&params, Domain::Interval, Distribution::Fixed).unwrap();
    let ScheduleExpr::LimitedHold(lh) = schedule else {
        panic!("{schedule:?}")
    };
    assert_eq!((lh.hold_duration, lh.time_unit), (30.0, None));
    assert_eq!(*lh.inner, ScheduleExpr::Atomic(fixed(Domain::Interval, 60.0)));
}

#[test]
fn annotations_wrap_the_limited_hold_and_keep_their_label() {
    let params = TTauParams::new(60.0, 30.0).unwrap().with_annotations(vec![pellet()]);
    let schedule = from_ttau(&params, Domain::Time, Distribution::Random).unwrap();
    let ScheduleExpr::Annotated(a) = schedule else {
        panic!("{schedule:?}")
    };
    assert_eq!(a.annotations, vec![pellet()]);
    assert!(matches!(a.expr, ScheduleExpr::LimitedHold(_)));
}

#[test]
fn a_ratio_schedule_is_named_in_the_error_whatever_its_value_or_annotations() {
    for distribution in Distribution::ALL {
        let schedule = atomic(distribution, Domain::Ratio, 0.0, None);
        let name = format!("Ratio schedule {}R ", distribution.code());
        let expr = ScheduleExpr::Atomic(schedule);
        assert_kind(to_ttau(&expr), "RatioScheduleError", &name);
        assert_kind(to_ttau(&annotated(expr, vec![pellet()])), "RatioScheduleError", &name);
        let params = TTauParams::new(10.0, 5.0).unwrap();
        assert_kind(
            from_ttau(&params, Domain::Ratio, distribution),
            "RatioScheduleError",
            &name,
        );
    }
}

#[test]
fn only_an_atomic_schedule_is_taken() {
    for source in ["FI60s LH30s", "EXT", "CRF", "Conc(FI1s, FI2s)", "DRL10s", "FR5(FI30s)"] {
        let expr = parsed(source);
        assert_kind(to_ttau(&expr), "TypeError", "atomic");
        assert_kind(to_ttau(&annotated(expr, vec![pellet()])), "TypeError", "atomic");
    }
    // The annotations of an inner layer are not dropped in silence.
    let once = annotated(ScheduleExpr::Atomic(fixed(Domain::Time, 5.0)), vec![pellet()]);
    assert_kind(to_ttau(&annotated(once, vec![])), "TypeError", "atomic");
}

#[test]
fn a_unit_other_than_seconds_is_rejected() {
    for unit in ["ms", "min"] {
        let schedule = atomic(Distribution::Fixed, Domain::Interval, 500.0, Some(unit));
        assert_kind(to_ttau(&ScheduleExpr::Atomic(schedule)), "ValueError", "seconds");
    }
}

#[test]
fn values_that_are_not_positive_and_finite_are_rejected() {
    for value in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let schedule = ScheduleExpr::Atomic(fixed(Domain::Interval, value));
        assert_kind(to_ttau(&schedule), "ValueError", "positive");
        assert_kind(TTauParams::new(value, 1.0), "ValueError", "T must be");
        assert_kind(TTauParams::new(1.0, value), "ValueError", "tau must be");
        let ratio = fixed(Domain::Ratio, value);
        assert_kind(approximate_ttau(&ratio, 1.0), "ValueError", "positive");
        let ratio = fixed(Domain::Ratio, 5.0);
        assert_kind(approximate_ttau(&ratio, value), "ValueError", "irt must be");
    }
}

#[test]
fn tau_may_equal_t_but_not_exceed_it() {
    assert!(TTauParams::new(10.0, 10.0).is_ok());
    assert_kind(TTauParams::new(10.0, 10.1), "ValueError", "tau must be");
}

#[test]
fn a_ratio_schedule_is_approximated_under_the_declared_irt() {
    let vr20 = atomic(Distribution::Variable, Domain::Ratio, 20.0, None);
    let approx = approximate_ttau(&vr20, 0.5).unwrap();
    assert_eq!((approx.t(), approx.tau()), (10.0, 10.0));
    assert_eq!(approx.duty_cycle(), 1.0);
    assert_eq!(approx.assumption().irt, 0.5);
}

#[test]
fn an_approximation_out_of_range_is_rejected() {
    let ratio = fixed(Domain::Ratio, 1e300);
    assert_kind(approximate_ttau(&ratio, 1e300), "ValueError", "positive");
    let ratio = fixed(Domain::Ratio, 1e-300);
    assert_kind(approximate_ttau(&ratio, 1e-300), "ValueError", "positive");
}

#[test]
fn a_time_based_schedule_is_not_approximated() {
    for domain in [Domain::Interval, Domain::Time] {
        assert_kind(approximate_ttau(&fixed(domain, 30.0), 2.0), "TypeError", "Ratio");
    }
}
