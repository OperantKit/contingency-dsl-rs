//! Shared helpers for extracting typed values from `Annotation` parameters.
//!
//! The Python implementation relies on Python's dynamic typing (params are a
//! `dict[str, Any]` that may hold strings, numbers, or nested duration dicts).
//! In Rust the same data is carried by the `AnnotationValue` enum plus a
//! `JsonValue` tree for structured sub-values; these helpers recover scalar
//! views in the same way Python's validators do.

use crate::ast::{Annotation, AnnotationValue, JsonValue};

/// View the positional argument as a string, if it is one.
pub fn positional_str(ann: &Annotation) -> Option<&str> {
    match &ann.positional {
        Some(AnnotationValue::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

/// Positional argument as an integer, if present and representable as such.
/// Mirrors Python's `isinstance(value, (int, float)) and int(value) == value`.
pub fn positional_int(ann: &Annotation) -> Option<i64> {
    match &ann.positional {
        Some(AnnotationValue::Int(i)) => Some(*i),
        Some(AnnotationValue::Float(f)) => {
            if f.fract() == 0.0 && f.is_finite() {
                Some(*f as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Positional as a numeric (int or float).
pub fn positional_number(ann: &Annotation) -> Option<f64> {
    match &ann.positional {
        Some(AnnotationValue::Int(i)) => Some(*i as f64),
        Some(AnnotationValue::Float(f)) => Some(*f),
        _ => None,
    }
}

/// Retrieve a parameter by key.
pub fn param<'a>(ann: &'a Annotation, key: &str) -> Option<&'a AnnotationValue> {
    ann.params.as_ref().and_then(|m| m.get(key))
}

/// Parameter as string.
pub fn param_str<'a>(ann: &'a Annotation, key: &str) -> Option<&'a str> {
    match param(ann, key)? {
        AnnotationValue::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

/// Parameter as numeric (int or float).
pub fn param_number(ann: &Annotation, key: &str) -> Option<f64> {
    match param(ann, key)? {
        AnnotationValue::Int(i) => Some(*i as f64),
        AnnotationValue::Float(f) => Some(*f),
        _ => None,
    }
}

/// Parameter as integer (accepts integer-valued floats).
pub fn param_int(ann: &Annotation, key: &str) -> Option<i64> {
    match param(ann, key)? {
        AnnotationValue::Int(i) => Some(*i),
        AnnotationValue::Float(f) => {
            if f.fract() == 0.0 && f.is_finite() {
                Some(*f as i64)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// True if the parameter is present.
#[allow(dead_code)]
pub fn has_param(ann: &Annotation, key: &str) -> bool {
    ann.params
        .as_ref()
        .map(|m| m.contains_key(key))
        .unwrap_or(false)
}

/// Sorted list of parameter keys.
pub fn param_keys(ann: &Annotation) -> Vec<String> {
    ann.params
        .as_ref()
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default()
}

/// Duration helper: accept a bare number, or a `{value, time_unit}` JSON
/// object, and return the numeric component.
pub fn coerce_duration_value(v: &AnnotationValue) -> Option<f64> {
    match v {
        AnnotationValue::Int(i) => Some(*i as f64),
        AnnotationValue::Float(f) => Some(*f),
        AnnotationValue::Json(JsonValue::Object(m)) => match m.get("value")? {
            JsonValue::Int(i) => Some(*i as f64),
            JsonValue::Float(f) => Some(*f),
            _ => None,
        },
        _ => None,
    }
}

/// Duration helper: return the `time_unit` of a `{value, time_unit}` object.
pub fn coerce_duration_unit(v: &AnnotationValue) -> Option<&str> {
    match v {
        AnnotationValue::Json(JsonValue::Object(m)) => match m.get("time_unit")? {
            JsonValue::String(s) => Some(s.as_str()),
            _ => None,
        },
        _ => None,
    }
}

/// Render an `AnnotationValue` approximately the way Python's `repr` does
/// for small scalars — used inside diagnostic messages where the Python
/// implementation interpolates `{value!r}`.
pub fn repr_annotation_value(v: &AnnotationValue) -> String {
    match v {
        AnnotationValue::Str(s) => format!("'{}'", s),
        AnnotationValue::Int(i) => i.to_string(),
        AnnotationValue::Float(f) => {
            if f.fract() == 0.0 && f.is_finite() {
                format!("{}.0", *f as i64)
            } else {
                f.to_string()
            }
        }
        AnnotationValue::Json(JsonValue::Null) => "None".into(),
        AnnotationValue::Json(JsonValue::Bool(b)) => {
            if *b {
                "True".into()
            } else {
                "False".into()
            }
        }
        AnnotationValue::Json(other) => format!("{:?}", other),
    }
}
