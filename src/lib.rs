//! Rust reference parser for contingency-dsl.
//!
//! See `../contingency-dsl/` for the language specification. The Python
//! reference implementation lives in `../contingency-dsl-py`.

pub mod ast;
pub mod enums;
pub mod errors;
pub mod experiment;
pub mod lexer;
pub mod parser;
pub mod respondent_parser;
pub mod serializer;
pub mod tokens;

pub use errors::{DslError, DslErrorKind, Result, SourceSpan};
pub use lexer::tokenize;
pub use parser::{parse, parse_top_level, TopLevel};
pub use serializer::{
    expr_to_json, json_to_string, paper_to_json, phase_sequence_to_json, program_to_json,
};
pub use tokens::{Token, TokenType};
