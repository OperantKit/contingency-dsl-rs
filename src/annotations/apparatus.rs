//! `apparatus-ext` annotation extension.
//!
//! Ported from `contingency_dsl/annotations/apparatus/`. Contributes
//! `@chamber`, `@interface`, `@hardware` (with alias `@hw`).

use crate::annotations::util::{param, param_keys, param_str, positional_str, repr_annotation_value};
use crate::ast::{Annotation, AnnotationValue};
use crate::extensions::AnnotationExtension;

const KEYWORDS: &[&str] = &["chamber", "interface", "hardware", "hw"];

#[derive(Debug, Clone, PartialEq)]
pub struct ChamberAnnotation {
    pub chamber: String,
    pub model: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceAnnotation {
    pub interface: String,
    pub port: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct HardwareAnnotation {
    pub hardware: String,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ApparatusExtension;

impl ApparatusExtension {
    pub const NAME: &'static str = "apparatus-ext";
    pub const VERSION: &'static str = "0.1.0";
}

impl AnnotationExtension for ApparatusExtension {
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
        let mut chamber_count = 0;
        let mut interface_count = 0;
        let mut hardware_count = 0;
        let mut hw_alias_count = 0;
        let mut canonical_hardware_count = 0;

        for ann in annotations {
            match ann.keyword.as_str() {
                "chamber" => {
                    chamber_count += 1;
                    diag.extend(validate_chamber(ann));
                }
                "interface" => {
                    interface_count += 1;
                    diag.extend(validate_interface(ann));
                }
                "hardware" | "hw" => {
                    hardware_count += 1;
                    if ann.keyword == "hw" {
                        hw_alias_count += 1;
                    } else {
                        canonical_hardware_count += 1;
                    }
                    diag.extend(validate_hardware(ann));
                }
                _ => {}
            }
        }

        if chamber_count > 1 {
            diag.push(
                "CHAMBER_MULTIPLE: at most one @chamber annotation is permitted per program".into(),
            );
        }
        if interface_count > 1 {
            diag.push(
                "INTERFACE_MULTIPLE: at most one @interface annotation is permitted per program".into(),
            );
        }
        if hardware_count > 1 {
            diag.push(
                "HARDWARE_MULTIPLE: at most one @hardware annotation is permitted per program".into(),
            );
        }
        if hw_alias_count >= 1 && canonical_hardware_count >= 1 {
            diag.push(
                "HARDWARE_ALIAS_COLLISION: @hw and @hardware refer to the same declaration; specify only one".into(),
            );
        }
        diag
    }
}

fn validate_chamber(ann: &Annotation) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    match positional_str(ann) {
        Some(s) if !s.is_empty() => {}
        _ => d.push("CHAMBER_MISSING_NAME: @chamber requires a positional string argument".into()),
    }
    if let Some(model) = param(ann, "model") {
        if !matches!(model, AnnotationValue::Str(_)) {
            d.push(format!(
                "CHAMBER_INVALID_MODEL: @chamber 'model' must be a string, got {}",
                repr_annotation_value(model)
            ));
        }
    }
    d
}

fn validate_interface(ann: &Annotation) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    match positional_str(ann) {
        Some(s) if !s.is_empty() => {}
        _ => d.push(
            "INTERFACE_MISSING_NAME: @interface requires a positional string argument".into(),
        ),
    }
    if let Some(port) = param(ann, "port") {
        if !matches!(port, AnnotationValue::Str(_)) {
            d.push(format!(
                "INTERFACE_INVALID_PORT: @interface 'port' must be a string, got {}",
                repr_annotation_value(port)
            ));
        }
    }
    d
}

fn validate_hardware(ann: &Annotation) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    let label = if ann.keyword == "hw" { "@hw" } else { "@hardware" };
    match positional_str(ann) {
        Some(s) if !s.is_empty() => {}
        _ => d.push(format!(
            "HARDWARE_MISSING_NAME: {} requires a positional string argument",
            label
        )),
    }
    if ann.params.as_ref().map(|m| !m.is_empty()).unwrap_or(false) {
        let keys = param_keys(ann);
        d.push(format!(
            "HARDWARE_UNEXPECTED_PARAMS: {} accepts no named parameters, got {:?}",
            label, keys
        ));
    }
    d
}

pub fn parse_chamber(ann: &Annotation) -> Option<ChamberAnnotation> {
    let s = positional_str(ann)?;
    if s.is_empty() {
        return None;
    }
    Some(ChamberAnnotation {
        chamber: s.to_string(),
        model: param_str(ann, "model").map(String::from),
    })
}
pub fn parse_interface(ann: &Annotation) -> Option<InterfaceAnnotation> {
    let s = positional_str(ann)?;
    if s.is_empty() {
        return None;
    }
    Some(InterfaceAnnotation {
        interface: s.to_string(),
        port: param_str(ann, "port").map(String::from),
    })
}
pub fn parse_hardware(ann: &Annotation) -> Option<HardwareAnnotation> {
    let s = positional_str(ann)?;
    if s.is_empty() {
        return None;
    }
    Some(HardwareAnnotation {
        hardware: s.to_string(),
    })
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
    fn chamber_missing_name() {
        let a = ann("chamber", None, &[]);
        let d = ApparatusExtension.validate(&[a]);
        assert!(d[0].starts_with("CHAMBER_MISSING_NAME"));
    }

    #[test]
    fn hardware_rejects_params() {
        let a = ann(
            "hardware",
            Some(AnnotationValue::Str("med-associates".into())),
            &[("port", AnnotationValue::Str("/dev/x".into()))],
        );
        let d = ApparatusExtension.validate(&[a]);
        assert!(d[0].starts_with("HARDWARE_UNEXPECTED_PARAMS"));
    }

    #[test]
    fn hw_and_hardware_collision() {
        let a1 = ann("hardware", Some(AnnotationValue::Str("a".into())), &[]);
        let a2 = ann("hw", Some(AnnotationValue::Str("b".into())), &[]);
        let d = ApparatusExtension.validate(&[a1, a2]);
        assert!(d.iter().any(|s| s.starts_with("HARDWARE_ALIAS_COLLISION")));
    }
}
