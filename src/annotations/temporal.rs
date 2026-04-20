//! `temporal-ext` annotation extension.
//!
//! Ported from `contingency_dsl/annotations/temporal/`. Contributes
//! `@clock`, `@warmup`, `@algorithm`.

use crate::annotations::util::{
    coerce_duration_unit, coerce_duration_value, param, param_int, param_str, positional_str,
    repr_annotation_value,
};
use crate::ast::{Annotation, AnnotationValue};
use crate::extensions::AnnotationExtension;

const VALID_CLOCK_UNITS: &[&str] = &["min", "ms", "s"]; // sorted for messages
const KNOWN_ALGORITHMS: &[&str] = &["arithmetic", "fleshler-hoffman"]; // sorted

/// Parsed `@clock` annotation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockAnnotation {
    pub unit: String,
}

/// Parsed `@warmup` annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct WarmupAnnotation {
    pub duration: f64,
    pub time_unit: Option<String>,
}

/// Parsed `@algorithm` annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct AlgorithmAnnotation {
    pub name: String,
    pub n: Option<i64>,
    pub seed: Option<i64>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct TemporalExtension;

impl TemporalExtension {
    pub const NAME: &'static str = "temporal-ext";
    pub const VERSION: &'static str = "0.1.0";
    pub const KEYWORDS: &'static [&'static str] = &["clock", "warmup", "algorithm"];
}

impl AnnotationExtension for TemporalExtension {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn version(&self) -> &'static str {
        Self::VERSION
    }
    fn annotation_keywords(&self) -> &'static [&'static str] {
        Self::KEYWORDS
    }
    fn validate(&self, annotations: &[Annotation]) -> Vec<String> {
        let mut diagnostics: Vec<String> = Vec::new();
        let mut clock_count = 0;
        for ann in annotations {
            match ann.keyword.as_str() {
                "clock" => {
                    clock_count += 1;
                    diagnostics.extend(validate_clock(ann));
                }
                "warmup" => diagnostics.extend(validate_warmup(ann)),
                "algorithm" => diagnostics.extend(validate_algorithm(ann)),
                _ => {}
            }
        }
        if clock_count > 1 {
            diagnostics.push(
                "CLOCK_MULTIPLE: at most one @clock annotation is permitted per program".into(),
            );
        }
        diagnostics
    }
}

fn validate_clock(ann: &Annotation) -> Vec<String> {
    let unit = param_str(ann, "unit");
    match unit {
        None => vec!["CLOCK_MISSING_UNIT: @clock requires a 'unit' parameter".into()],
        Some(u) if !VALID_CLOCK_UNITS.contains(&u) => vec![format!(
            "CLOCK_INVALID_UNIT: @clock unit must be one of {:?}, got '{}'",
            VALID_CLOCK_UNITS, u
        )],
        _ => Vec::new(),
    }
}

fn validate_warmup(ann: &Annotation) -> Vec<String> {
    let Some(dur_val) = param(ann, "duration") else {
        return vec!["WARMUP_MISSING_DURATION: @warmup requires a 'duration' parameter".into()];
    };
    let Some(numeric) = coerce_duration_value(dur_val) else {
        return vec!["WARMUP_MISSING_DURATION: @warmup 'duration' must be a number".into()];
    };
    if numeric < 0.0 {
        return vec![format!(
            "WARMUP_NEGATIVE_DURATION: @warmup duration must be non-negative, got {}",
            numeric
        )];
    }
    Vec::new()
}

fn validate_algorithm(ann: &Annotation) -> Vec<String> {
    let mut diagnostics: Vec<String> = Vec::new();
    let name = positional_str(ann);
    match name {
        None | Some("") => {
            diagnostics.push(
                "ALGORITHM_MISSING_NAME: @algorithm requires a positional name argument".into(),
            );
        }
        Some(n) if !KNOWN_ALGORITHMS.contains(&n) => {
            diagnostics.push(format!(
                "ALGORITHM_UNKNOWN: @algorithm name must be one of {:?}, got '{}'",
                KNOWN_ALGORITHMS, n
            ));
        }
        _ => {}
    }

    if let Some(n_val) = param(ann, "n") {
        match n_val {
            AnnotationValue::Int(i) if *i >= 1 => {}
            AnnotationValue::Float(f) if *f >= 1.0 && f.fract() == 0.0 => {}
            other => diagnostics.push(format!(
                "ALGORITHM_INVALID_N: @algorithm 'n' must be a positive integer, got {}",
                repr_annotation_value(other)
            )),
        }
    }
    diagnostics
}

/// Parse a `@clock` annotation.
pub fn parse_clock(ann: &Annotation) -> Option<ClockAnnotation> {
    param_str(ann, "unit").map(|u| ClockAnnotation { unit: u.to_string() })
}

/// Parse a `@warmup` annotation.
pub fn parse_warmup(ann: &Annotation) -> Option<WarmupAnnotation> {
    let dv = param(ann, "duration")?;
    let numeric = coerce_duration_value(dv)?;
    let tu = coerce_duration_unit(dv).map(String::from);
    Some(WarmupAnnotation {
        duration: numeric,
        time_unit: tu,
    })
}

/// Parse an `@algorithm` annotation.
pub fn parse_algorithm(ann: &Annotation) -> Option<AlgorithmAnnotation> {
    let name = positional_str(ann)?;
    if name.is_empty() {
        return None;
    }
    Some(AlgorithmAnnotation {
        name: name.to_string(),
        n: param_int(ann, "n"),
        seed: param_int(ann, "seed"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Annotation, AnnotationValue, JsonValue};
    use std::collections::BTreeMap;

    fn ann(kw: &str, positional: Option<AnnotationValue>, params: &[(&str, AnnotationValue)]) -> Annotation {
        let mut m = BTreeMap::new();
        for (k, v) in params {
            m.insert((*k).to_string(), v.clone());
        }
        Annotation {
            keyword: kw.into(),
            positional,
            params: if m.is_empty() { None } else { Some(m) },
            label: None,
        }
    }

    #[test]
    fn clock_missing_unit() {
        let a = ann("clock", None, &[]);
        let d = TemporalExtension.validate(&[a]);
        assert!(d[0].starts_with("CLOCK_MISSING_UNIT"));
    }

    #[test]
    fn clock_invalid_unit() {
        let a = ann("clock", None, &[("unit", AnnotationValue::Str("hours".into()))]);
        let d = TemporalExtension.validate(&[a]);
        assert!(d[0].starts_with("CLOCK_INVALID_UNIT"));
    }

    #[test]
    fn clock_multiple_detected() {
        let a1 = ann("clock", None, &[("unit", AnnotationValue::Str("s".into()))]);
        let a2 = ann("clock", None, &[("unit", AnnotationValue::Str("ms".into()))]);
        let d = TemporalExtension.validate(&[a1, a2]);
        assert!(d.iter().any(|s| s.starts_with("CLOCK_MULTIPLE")));
    }

    #[test]
    fn warmup_negative() {
        let a = ann(
            "warmup",
            None,
            &[("duration", AnnotationValue::Float(-1.0))],
        );
        let d = TemporalExtension.validate(&[a]);
        assert!(d[0].starts_with("WARMUP_NEGATIVE_DURATION"));
    }

    #[test]
    fn warmup_duration_dict_ok() {
        let mut obj = std::collections::BTreeMap::new();
        obj.insert("value".to_string(), JsonValue::Int(60));
        obj.insert("time_unit".to_string(), JsonValue::String("s".into()));
        let a = ann(
            "warmup",
            None,
            &[("duration", AnnotationValue::Json(JsonValue::Object(obj)))],
        );
        let d = TemporalExtension.validate(&[a]);
        assert!(d.is_empty());
    }

    #[test]
    fn algorithm_unknown() {
        let a = ann(
            "algorithm",
            Some(AnnotationValue::Str("randomized".into())),
            &[],
        );
        let d = TemporalExtension.validate(&[a]);
        assert!(d[0].starts_with("ALGORITHM_UNKNOWN"));
    }
}
