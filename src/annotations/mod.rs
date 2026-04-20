//! Annotation extension modules ported from
//! `contingency_dsl/annotations/` in the Python reference implementation.
//!
//! Each submodule provides a typed dataclass-equivalent representation of
//! parsed annotations, a `XxxExtension` struct implementing
//! [`crate::extensions::AnnotationExtension`], and where the Python module
//! carried an analyzer/validator helper, the same helper re-ported.

pub mod apparatus;
pub mod composed;
pub mod measurement;
pub mod stimulus;
pub mod subjects;
pub mod temporal;

pub(crate) mod util;

pub use apparatus::ApparatusExtension;
pub use composed::ComposedExtension;
pub use measurement::MeasurementExtension;
pub use stimulus::StimulusExtension;
pub use subjects::SubjectsExtension;
pub use temporal::TemporalExtension;
