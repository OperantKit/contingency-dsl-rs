//! `stimulus-ext` annotation extension.
//!
//! Ported from `contingency_dsl/annotations/stimulus/` in the Python
//! reference implementation. Contributes `@reinforcer`, `@sd`,
//! `@operandum`, and `@brief`.

use crate::annotations::util::{param_int, param_number, param_str, positional_str};
use crate::ast::Annotation;
use crate::extensions::AnnotationExtension;

/// Reinforcer classification per Skinner (1953).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReinforcerType {
    Unconditioned,
    Conditioned,
    Generalized,
}

impl ReinforcerType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unconditioned => "unconditioned",
            Self::Conditioned => "conditioned",
            Self::Generalized => "generalized",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "unconditioned" => Some(Self::Unconditioned),
            "conditioned" => Some(Self::Conditioned),
            "generalized" => Some(Self::Generalized),
            _ => None,
        }
    }
}

/// Parsed `@reinforcer` annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct ReinforcerAnnotation {
    pub name: String,
    pub reinforcer_type: ReinforcerType,
    pub backup_ids: Vec<String>,
    pub innate_power: f64,
    pub initial_alpha: f64,
}

impl Default for ReinforcerAnnotation {
    fn default() -> Self {
        Self {
            name: String::new(),
            reinforcer_type: ReinforcerType::Unconditioned,
            backup_ids: Vec::new(),
            innate_power: 1.0,
            initial_alpha: 0.5,
        }
    }
}

/// Parsed `@sd` annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct SDAnnotation {
    pub name: String,
    pub component: Option<i64>,
}

/// Parsed `@operandum` annotation.
#[derive(Debug, Clone, PartialEq)]
pub struct OperandumAnnotation {
    pub name: String,
}

/// Parsed `@brief` annotation for second-order schedules.
#[derive(Debug, Clone, PartialEq)]
pub struct BriefAnnotation {
    pub name: String,
    pub duration: Option<f64>,
    pub time_unit: Option<String>,
}

/// Stimulus-dimension extension.
#[derive(Debug, Default, Clone, Copy)]
pub struct StimulusExtension;

impl StimulusExtension {
    pub const NAME: &'static str = "stimulus-ext";
    pub const VERSION: &'static str = "0.1.0";
    pub const KEYWORDS: &'static [&'static str] = &["reinforcer", "sd", "operandum", "brief"];
}

impl AnnotationExtension for StimulusExtension {
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
        let mut reinforcer_names: std::collections::BTreeSet<String> = Default::default();
        let mut backup_refs: std::collections::BTreeSet<String> = Default::default();

        for ann in annotations {
            if ann.keyword == "reinforcer" {
                let parsed = parse_reinforcer(ann);
                reinforcer_names.insert(parsed.name.clone());
                if parsed.reinforcer_type == ReinforcerType::Conditioned && parsed.backup_ids.is_empty() {
                    diagnostics.push(format!(
                        "conditioned reinforcer '{}' must specify at least one backup",
                        parsed.name
                    ));
                }
                if parsed.reinforcer_type == ReinforcerType::Generalized && parsed.backup_ids.len() < 2 {
                    diagnostics.push(format!(
                        "generalized reinforcer '{}' must specify at least 2 backups",
                        parsed.name
                    ));
                }
                for bid in &parsed.backup_ids {
                    backup_refs.insert(bid.clone());
                }
            }
        }

        for r in &backup_refs {
            if !reinforcer_names.contains(r) {
                diagnostics.push(format!("backup reinforcer '{}' referenced but not declared", r));
            }
        }

        diagnostics
    }
}

/// Parse a `@reinforcer` annotation into the typed representation.
pub fn parse_reinforcer(ann: &Annotation) -> ReinforcerAnnotation {
    let name = positional_str(ann).unwrap_or("").to_string();
    let rtype = param_str(ann, "type")
        .and_then(ReinforcerType::parse)
        .unwrap_or(ReinforcerType::Unconditioned);
    let mut backup_ids: Vec<String> = Vec::new();
    if let Some(backup) = param_str(ann, "backup") {
        backup_ids.push(backup.to_string());
    }
    let innate_power = param_number(ann, "power").unwrap_or(1.0);
    let initial_alpha = param_number(ann, "alpha0").unwrap_or(0.5);
    ReinforcerAnnotation {
        name,
        reinforcer_type: rtype,
        backup_ids,
        innate_power,
        initial_alpha,
    }
}

/// Parse an `@sd` annotation.
pub fn parse_sd(ann: &Annotation) -> SDAnnotation {
    SDAnnotation {
        name: positional_str(ann).unwrap_or("").to_string(),
        component: param_int(ann, "component"),
    }
}

/// Parse an `@operandum` annotation.
pub fn parse_operandum(ann: &Annotation) -> OperandumAnnotation {
    OperandumAnnotation {
        name: positional_str(ann).unwrap_or("").to_string(),
    }
}

/// Parse a `@brief` annotation.
pub fn parse_brief(ann: &Annotation) -> BriefAnnotation {
    let name = positional_str(ann).unwrap_or("").to_string();
    let duration = param_number(ann, "duration");
    let time_unit = param_str(ann, "time_unit").map(String::from);
    BriefAnnotation {
        name,
        duration,
        time_unit,
    }
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
    fn conditioned_without_backup_fails() {
        let a = ann(
            "reinforcer",
            Some(AnnotationValue::Str("token".into())),
            &[("type", AnnotationValue::Str("conditioned".into()))],
        );
        let diag = StimulusExtension.validate(std::slice::from_ref(&a));
        assert_eq!(diag.len(), 1);
        assert!(diag[0].contains("conditioned reinforcer 'token'"));
    }

    #[test]
    fn generalized_needs_two_backups() {
        let a = ann(
            "reinforcer",
            Some(AnnotationValue::Str("money".into())),
            &[
                ("type", AnnotationValue::Str("generalized".into())),
                ("backup", AnnotationValue::Str("food".into())),
            ],
        );
        let b = ann(
            "reinforcer",
            Some(AnnotationValue::Str("food".into())),
            &[],
        );
        let diag = StimulusExtension.validate(&[a, b]);
        assert!(diag.iter().any(|d| d.contains("at least 2 backups")));
    }

    #[test]
    fn backup_reference_must_be_declared() {
        let a = ann(
            "reinforcer",
            Some(AnnotationValue::Str("token".into())),
            &[
                ("type", AnnotationValue::Str("conditioned".into())),
                ("backup", AnnotationValue::Str("ghost".into())),
            ],
        );
        let diag = StimulusExtension.validate(std::slice::from_ref(&a));
        assert!(diag.iter().any(|d| d.contains("'ghost' referenced but not declared")));
    }

    #[test]
    fn valid_reinforcer_has_no_diagnostics() {
        let a = ann(
            "reinforcer",
            Some(AnnotationValue::Str("food".into())),
            &[],
        );
        assert!(StimulusExtension.validate(std::slice::from_ref(&a)).is_empty());
    }

    #[test]
    fn parse_reinforcer_defaults() {
        let a = ann(
            "reinforcer",
            Some(AnnotationValue::Str("food".into())),
            &[],
        );
        let p = parse_reinforcer(&a);
        assert_eq!(p.name, "food");
        assert_eq!(p.reinforcer_type, ReinforcerType::Unconditioned);
        assert_eq!(p.innate_power, 1.0);
        assert_eq!(p.initial_alpha, 0.5);
    }

    #[test]
    fn parse_sd_with_component() {
        let a = ann(
            "sd",
            Some(AnnotationValue::Str("red_light".into())),
            &[("component", AnnotationValue::Int(1))],
        );
        let p = parse_sd(&a);
        assert_eq!(p.name, "red_light");
        assert_eq!(p.component, Some(1));
    }
}
