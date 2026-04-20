//! `subjects-ext` annotation extension.
//!
//! Ported from `contingency_dsl/annotations/subjects/`. Contributes
//! `@species`, `@strain`, `@deprivation`, `@history`, `@n`.

use crate::annotations::util::{
    param, param_number, param_str, positional_int, positional_number, positional_str,
    repr_annotation_value,
};
use crate::ast::{Annotation, AnnotationValue};
use crate::extensions::AnnotationExtension;

const KEYWORDS: &[&str] = &["species", "strain", "deprivation", "history", "n"];

#[derive(Debug, Clone, PartialEq)]
pub struct SpeciesAnnotation {
    pub species: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct StrainAnnotation {
    pub strain: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct DeprivationAnnotation {
    pub hours: f64,
    pub target: String,
}
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryAnnotation {
    pub history: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NAnnotation {
    pub n: i64,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SubjectsExtension;

impl SubjectsExtension {
    pub const NAME: &'static str = "subjects-ext";
    pub const VERSION: &'static str = "0.1.0";
}

impl AnnotationExtension for SubjectsExtension {
    fn name(&self) -> &'static str {
        Self::NAME
    }
    fn version(&self) -> &'static str {
        Self::VERSION
    }
    fn annotation_keywords(&self) -> &'static [&'static str] {
        KEYWORDS
    }
    fn validate(&self, annotations: &[Annotation]) -> Vec<String> {
        let mut diag: Vec<String> = Vec::new();
        let mut counts: std::collections::BTreeMap<&str, usize> =
            KEYWORDS.iter().copied().map(|k| (k, 0)).collect();

        for ann in annotations {
            let kw = ann.keyword.as_str();
            if let Some(c) = counts.get_mut(kw) {
                *c += 1;
            } else {
                continue;
            }
            match kw {
                "species" => diag.extend(validate_species(ann)),
                "strain" => diag.extend(validate_strain(ann)),
                "deprivation" => diag.extend(validate_deprivation(ann)),
                "history" => diag.extend(validate_history(ann)),
                "n" => diag.extend(validate_n(ann)),
                _ => {}
            }
        }
        for (kw, count) in &counts {
            if *count > 1 {
                diag.push(format!(
                    "{}_MULTIPLE: at most one @{} annotation is permitted per program",
                    kw.to_uppercase(),
                    kw
                ));
            }
        }
        diag
    }
}

fn validate_species(ann: &Annotation) -> Vec<String> {
    match positional_str(ann) {
        Some(s) if !s.is_empty() => vec![],
        _ => vec!["SPECIES_MISSING_NAME: @species requires a positional string argument".into()],
    }
}
fn validate_strain(ann: &Annotation) -> Vec<String> {
    match positional_str(ann) {
        Some(s) if !s.is_empty() => vec![],
        _ => vec!["STRAIN_MISSING_NAME: @strain requires a positional string argument".into()],
    }
}
fn validate_history(ann: &Annotation) -> Vec<String> {
    match positional_str(ann) {
        Some(s) if !s.is_empty() => vec![],
        _ => vec![
            "HISTORY_MISSING_DESCRIPTOR: @history requires a positional string argument".into(),
        ],
    }
}

fn validate_deprivation(ann: &Annotation) -> Vec<String> {
    let mut diag: Vec<String> = Vec::new();
    if ann.positional.is_some() {
        diag.push(
            "DEPRIVATION_UNEXPECTED_POSITIONAL: @deprivation does not accept positional arguments; use hours= and target= named parameters".into()
        );
    }
    let hours_v = param(ann, "hours");
    let target_v = param(ann, "target");

    match hours_v {
        None => diag.push(
            "DEPRIVATION_MISSING_HOURS: @deprivation requires an 'hours' parameter".into(),
        ),
        Some(v) => match v {
            AnnotationValue::Int(i) => {
                if *i < 0 {
                    diag.push(format!(
                        "DEPRIVATION_NEGATIVE_HOURS: @deprivation 'hours' must be non-negative, got {}",
                        i
                    ));
                }
            }
            AnnotationValue::Float(f) => {
                if *f < 0.0 {
                    diag.push(format!(
                        "DEPRIVATION_NEGATIVE_HOURS: @deprivation 'hours' must be non-negative, got {}",
                        f
                    ));
                }
            }
            other => diag.push(format!(
                "DEPRIVATION_INVALID_HOURS: @deprivation 'hours' must be a non-negative number, got {}",
                repr_annotation_value(other)
            )),
        },
    }

    match target_v {
        None => diag.push(
            "DEPRIVATION_MISSING_TARGET: @deprivation requires a 'target' parameter".into(),
        ),
        Some(AnnotationValue::Str(s)) if !s.is_empty() => {}
        Some(other) => diag.push(format!(
            "DEPRIVATION_INVALID_TARGET: @deprivation 'target' must be a non-empty string, got {}",
            repr_annotation_value(other)
        )),
    }
    diag
}

fn validate_n(ann: &Annotation) -> Vec<String> {
    if ann.positional.is_none() {
        return vec!["N_MISSING_VALUE: @n requires a positional integer argument".into()];
    }
    match positional_int(ann) {
        Some(n) if n >= 1 => Vec::new(),
        _ => {
            let got = ann
                .positional
                .as_ref()
                .map(repr_annotation_value)
                .unwrap_or_else(|| "None".into());
            vec![format!(
                "N_INVALID_VALUE: @n must be a positive integer >= 1, got {}",
                got
            )]
        }
    }
}

pub fn parse_species(ann: &Annotation) -> Option<SpeciesAnnotation> {
    let s = positional_str(ann)?;
    if s.is_empty() {
        return None;
    }
    Some(SpeciesAnnotation {
        species: s.to_string(),
    })
}
pub fn parse_strain(ann: &Annotation) -> Option<StrainAnnotation> {
    let s = positional_str(ann)?;
    if s.is_empty() {
        return None;
    }
    Some(StrainAnnotation {
        strain: s.to_string(),
    })
}
pub fn parse_history(ann: &Annotation) -> Option<HistoryAnnotation> {
    let s = positional_str(ann)?;
    if s.is_empty() {
        return None;
    }
    Some(HistoryAnnotation {
        history: s.to_string(),
    })
}
pub fn parse_deprivation(ann: &Annotation) -> Option<DeprivationAnnotation> {
    let hours = param_number(ann, "hours")?;
    if hours < 0.0 {
        return None;
    }
    let target = param_str(ann, "target")?;
    if target.is_empty() {
        return None;
    }
    Some(DeprivationAnnotation {
        hours,
        target: target.to_string(),
    })
}
pub fn parse_n(ann: &Annotation) -> Option<NAnnotation> {
    let v = positional_number(ann)?;
    if v.fract() != 0.0 || v < 1.0 {
        return None;
    }
    Some(NAnnotation { n: v as i64 })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Annotation, AnnotationValue};
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
    fn species_requires_string() {
        let a = ann("species", None, &[]);
        let d = SubjectsExtension.validate(&[a]);
        assert!(d[0].starts_with("SPECIES_MISSING_NAME"));
    }

    #[test]
    fn n_rejects_zero() {
        let a = ann("n", Some(AnnotationValue::Int(0)), &[]);
        let d = SubjectsExtension.validate(&[a]);
        assert!(d[0].starts_with("N_INVALID_VALUE"));
    }

    #[test]
    fn deprivation_missing_hours() {
        let a = ann(
            "deprivation",
            None,
            &[("target", AnnotationValue::Str("food".into()))],
        );
        let d = SubjectsExtension.validate(&[a]);
        assert!(d.iter().any(|s| s.starts_with("DEPRIVATION_MISSING_HOURS")));
    }

    #[test]
    fn deprivation_negative_hours() {
        let a = ann(
            "deprivation",
            None,
            &[
                ("hours", AnnotationValue::Int(-2)),
                ("target", AnnotationValue::Str("food".into())),
            ],
        );
        let d = SubjectsExtension.validate(&[a]);
        assert!(d.iter().any(|s| s.starts_with("DEPRIVATION_NEGATIVE_HOURS")));
    }

    #[test]
    fn multiple_species_flagged() {
        let a1 = ann("species", Some(AnnotationValue::Str("rat".into())), &[]);
        let a2 = ann("species", Some(AnnotationValue::Str("pigeon".into())), &[]);
        let d = SubjectsExtension.validate(&[a1, a2]);
        assert!(d.iter().any(|s| s.starts_with("SPECIES_MULTIPLE")));
    }
}
