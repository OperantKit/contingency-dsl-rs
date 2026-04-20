//! `composed-ext` annotation extension.
//!
//! Ported from `contingency_dsl/annotations/composed/`. Contributes
//! `@omission` and `@avoidance` plus a lightweight analyzer that produces
//! execution plans for US-bearing respondent primitives.

use crate::annotations::util::{param, param_str};
use crate::ast::{Annotation, AnnotationValue};
use crate::extensions::AnnotationExtension;

const OMISSION_DURING_VALUES: &[&str] = &["cs", "iti", "trial"];
const AVOIDANCE_MODES: &[&str] = &["cancel", "postpone"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmissionDuring {
    Cs,
    Iti,
    Trial,
}

impl OmissionDuring {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cs" => Some(Self::Cs),
            "iti" => Some(Self::Iti),
            "trial" => Some(Self::Trial),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AvoidanceMode {
    Cancel,
    Postpone,
}

impl AvoidanceMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "cancel" => Some(Self::Cancel),
            "postpone" => Some(Self::Postpone),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OmissionAnnotation {
    pub response: String,
    pub during: OmissionDuring,
}

impl Default for OmissionAnnotation {
    fn default() -> Self {
        Self {
            response: String::new(),
            during: OmissionDuring::Cs,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvoidanceAnnotation {
    pub response: String,
    pub mode: AvoidanceMode,
}

impl Default for AvoidanceAnnotation {
    fn default() -> Self {
        Self {
            response: String::new(),
            mode: AvoidanceMode::Cancel,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ComposedExtension;

impl ComposedExtension {
    pub const NAME: &'static str = "composed-ext";
    pub const VERSION: &'static str = "0.1.0";
    pub const KEYWORDS: &'static [&'static str] = &["omission", "avoidance"];
}

impl AnnotationExtension for ComposedExtension {
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
        let mut diag: Vec<String> = Vec::new();
        for ann in annotations {
            match ann.keyword.as_str() {
                "omission" => diag.extend(validate_omission(ann)),
                "avoidance" => diag.extend(validate_avoidance(ann)),
                _ => {}
            }
        }
        diag
    }
}

fn validate_omission(ann: &Annotation) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    match param_str(ann, "response") {
        Some(s) if !s.is_empty() => {}
        _ => d.push("@omission requires a string 'response' parameter".into()),
    }
    if let Some(v) = param(ann, "during") {
        if let AnnotationValue::Str(s) = v {
            if !OMISSION_DURING_VALUES.contains(&s.as_str()) {
                d.push(format!(
                    "@omission 'during' must be one of {:?} (got '{}')",
                    OMISSION_DURING_VALUES, s
                ));
            }
        } else {
            d.push(format!(
                "@omission 'during' must be one of {:?} (got {:?})",
                OMISSION_DURING_VALUES, v
            ));
        }
    }
    if let Some(params) = ann.params.as_ref() {
        let unknown: Vec<String> = params
            .keys()
            .filter(|k| k.as_str() != "response" && k.as_str() != "during")
            .cloned()
            .collect();
        if !unknown.is_empty() {
            d.push(format!("@omission has unknown parameter(s): {:?}", unknown));
        }
    }
    d
}

fn validate_avoidance(ann: &Annotation) -> Vec<String> {
    let mut d: Vec<String> = Vec::new();
    match param_str(ann, "response") {
        Some(s) if !s.is_empty() => {}
        _ => d.push("@avoidance requires a string 'response' parameter".into()),
    }
    if let Some(v) = param(ann, "mode") {
        if let AnnotationValue::Str(s) = v {
            if !AVOIDANCE_MODES.contains(&s.as_str()) {
                d.push(format!(
                    "@avoidance 'mode' must be one of {:?} (got '{}')",
                    AVOIDANCE_MODES, s
                ));
            }
        } else {
            d.push(format!(
                "@avoidance 'mode' must be one of {:?} (got {:?})",
                AVOIDANCE_MODES, v
            ));
        }
    }
    if let Some(params) = ann.params.as_ref() {
        let unknown: Vec<String> = params
            .keys()
            .filter(|k| k.as_str() != "response" && k.as_str() != "mode")
            .cloned()
            .collect();
        if !unknown.is_empty() {
            d.push(format!(
                "@avoidance has unknown parameter(s): {:?}",
                unknown
            ));
        }
    }
    d
}

pub fn parse_omission(ann: &Annotation) -> Option<OmissionAnnotation> {
    let response = param_str(ann, "response")?.to_string();
    if response.is_empty() {
        return None;
    }
    let during = match param_str(ann, "during") {
        Some(s) => OmissionDuring::parse(s)?,
        None => OmissionDuring::Cs,
    };
    Some(OmissionAnnotation { response, during })
}

pub fn parse_avoidance(ann: &Annotation) -> Option<AvoidanceAnnotation> {
    let response = param_str(ann, "response")?.to_string();
    if response.is_empty() {
        return None;
    }
    let mode = match param_str(ann, "mode") {
        Some(s) => AvoidanceMode::parse(s)?,
        None => AvoidanceMode::Cancel,
    };
    Some(AvoidanceAnnotation { response, mode })
}

// -------------------------------------------------------------------------
// Analyzer helpers (structural validation on AnnotatedSchedule nodes).
// Narrowed port of the Python analyzer: flags @omission / @avoidance
// attached to non-US-bearing respondent primitives and multiple rules on
// the same primitive. Operates only on operant-side Annotated nodes for
// now; callers that want respondent coverage should apply the same check.
// -------------------------------------------------------------------------

const US_BEARING_PAIRS: &[&str] = &[
    "PairForwardDelay",
    "PairForwardTrace",
    "PairSimultaneous",
    "PairBackward",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposedRule {
    Omission(OmissionAnnotation),
    Avoidance(AvoidanceAnnotation),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionPlan {
    pub rule: ComposedRule,
    pub pair_type: String,
    pub cs: String,
    pub us: String,
    pub phase_label: Option<String>,
}

#[derive(Debug, Default)]
pub struct AnalysisResult {
    pub plans: Vec<ExecutionPlan>,
    pub diagnostics: Vec<String>,
}

impl AnalysisResult {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

/// Validate a set of composed-ext annotations attached to a single
/// primitive node (identified by its type name) and produce either an
/// `ExecutionPlan` or diagnostics.
pub fn analyze_pair_attachment(
    annotations: &[Annotation],
    inner_type: &str,
    cs: Option<&str>,
    us: Option<&str>,
    phase_label: Option<&str>,
) -> AnalysisResult {
    let mut result = AnalysisResult::default();

    let composed: Vec<&Annotation> = annotations
        .iter()
        .filter(|a| a.keyword == "omission" || a.keyword == "avoidance")
        .collect();
    if composed.is_empty() {
        return result;
    }

    if !US_BEARING_PAIRS.contains(&inner_type) {
        for a in &composed {
            result.diagnostics.push(format!(
                "@{} attached to '{}', but composed-ext cancellation / postponement rules require a US-bearing respondent primitive (one of {:?})",
                a.keyword, inner_type, US_BEARING_PAIRS
            ));
        }
        return result;
    }

    if composed.len() > 1 {
        let mut kinds: std::collections::BTreeSet<&str> =
            composed.iter().map(|a| a.keyword.as_str()).collect();
        if kinds.len() > 1 {
            result.diagnostics.push(format!(
                "conflicting composed-ext annotations on the same primitive: {:?} — at most one of @omission / @avoidance is admissible",
                kinds.iter().collect::<Vec<_>>()
            ));
            return result;
        }
        let only = kinds.pop_first().unwrap_or("");
        result.diagnostics.push(format!(
            "multiple @{} annotations on the same primitive — at most one is admissible",
            only
        ));
        return result;
    }

    let ann = composed[0];
    let rule = match ann.keyword.as_str() {
        "omission" => match parse_omission(ann) {
            Some(r) => ComposedRule::Omission(r),
            None => {
                let resp = param_str(ann, "response");
                if resp.map(str::is_empty).unwrap_or(true) {
                    result.diagnostics.push(format!(
                        "@omission: 'response' parameter is required (got {:?})",
                        resp
                    ));
                } else {
                    let during = param_str(ann, "during").unwrap_or("cs");
                    result.diagnostics.push(format!(
                        "@omission: 'during' must be one of cs/iti/trial (got '{}')",
                        during
                    ));
                }
                return result;
            }
        },
        "avoidance" => match parse_avoidance(ann) {
            Some(r) => ComposedRule::Avoidance(r),
            None => {
                let resp = param_str(ann, "response");
                if resp.map(str::is_empty).unwrap_or(true) {
                    result.diagnostics.push(format!(
                        "@avoidance: 'response' parameter is required (got {:?})",
                        resp
                    ));
                } else {
                    let mode = param_str(ann, "mode").unwrap_or("cancel");
                    result.diagnostics.push(format!(
                        "@avoidance: 'mode' must be one of cancel/postpone (got '{}')",
                        mode
                    ));
                }
                return result;
            }
        },
        _ => return result,
    };

    let (cs_s, us_s) = match (cs, us) {
        (Some(c), Some(u)) => (c.to_string(), u.to_string()),
        _ => {
            let kw = match &rule {
                ComposedRule::Omission(_) => "omission",
                ComposedRule::Avoidance(_) => "avoidance",
            };
            result.diagnostics.push(format!(
                "@{}: enclosing {} has non-string cs/us references (cs={:?}, us={:?})",
                kw, inner_type, cs, us
            ));
            return result;
        }
    };

    result.plans.push(ExecutionPlan {
        rule,
        pair_type: inner_type.to_string(),
        cs: cs_s,
        us: us_s,
        phase_label: phase_label.map(String::from),
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Annotation, AnnotationValue};
    use std::collections::BTreeMap;

    fn ann(kw: &str, params: &[(&str, AnnotationValue)]) -> Annotation {
        let mut m = BTreeMap::new();
        for (k, v) in params {
            m.insert((*k).to_string(), v.clone());
        }
        Annotation {
            keyword: kw.into(),
            positional: None,
            params: if m.is_empty() { None } else { Some(m) },
            label: None,
        }
    }

    #[test]
    fn omission_requires_response() {
        let a = ann("omission", &[]);
        let d = ComposedExtension.validate(&[a]);
        assert!(d[0].contains("requires a string 'response'"));
    }

    #[test]
    fn avoidance_mode_enum() {
        let a = ann(
            "avoidance",
            &[
                ("response", AnnotationValue::Str("lever".into())),
                ("mode", AnnotationValue::Str("escape".into())),
            ],
        );
        let d = ComposedExtension.validate(&[a]);
        assert!(d[0].contains("'mode' must be one of"));
    }

    #[test]
    fn omission_unknown_param() {
        let a = ann(
            "omission",
            &[
                ("response", AnnotationValue::Str("peck".into())),
                ("foo", AnnotationValue::Int(1)),
            ],
        );
        let d = ComposedExtension.validate(&[a]);
        assert!(d.iter().any(|s| s.contains("unknown parameter")));
    }

    #[test]
    fn analyzer_flags_non_pair_attachment() {
        let a = ann(
            "omission",
            &[("response", AnnotationValue::Str("peck".into()))],
        );
        let r = analyze_pair_attachment(&[a], "Extinction", Some("cs"), Some("us"), None);
        assert_eq!(r.plans.len(), 0);
        assert!(r.diagnostics[0].contains("US-bearing"));
    }

    #[test]
    fn analyzer_emits_plan_for_pair() {
        let a = ann(
            "omission",
            &[("response", AnnotationValue::Str("peck".into()))],
        );
        let r = analyze_pair_attachment(&[a], "PairForwardDelay", Some("cs1"), Some("food"), Some("P1"));
        assert_eq!(r.plans.len(), 1);
        assert_eq!(r.plans[0].cs, "cs1");
        assert_eq!(r.plans[0].us, "food");
        assert_eq!(r.plans[0].phase_label.as_deref(), Some("P1"));
    }
}
