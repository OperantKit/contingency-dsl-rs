//! Rust reference parser for contingency-dsl.
//!
//! See `../contingency-dsl/` for the language specification. The Python
//! reference implementation lives in `../contingency-dsl-py`.

pub mod annotations;
pub mod ast;
pub mod enums;
pub mod errors;
pub mod experiment;
pub mod extensions;
pub mod lexer;
pub mod parser;
pub mod respondent_parser;
pub mod serializer;
pub mod tokens;

pub use annotations::{
    ApparatusExtension, ComposedExtension, MeasurementExtension, StimulusExtension,
    SubjectsExtension, TemporalExtension,
};
pub use errors::{DslError, DslErrorKind, Result, SourceSpan};
pub use extensions::{
    AnnotationExtension, CyclicDependencyError, ExtensionRegistry, KeywordConflictError,
    RegistryError,
};
pub use lexer::tokenize;
pub use parser::{collect_warnings, expand_bindings, parse, parse_top_level, TopLevel};
pub use serializer::{
    expr_to_json, json_to_string, paper_to_json, phase_sequence_to_json, program_to_json,
};
pub use tokens::{Token, TokenType};
