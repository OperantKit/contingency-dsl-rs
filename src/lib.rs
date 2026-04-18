//! Rust reference parser for contingency-dsl.
//!
//! See `../contingency-dsl/` for the language specification. The Python
//! reference implementation lives in `../contingency-dsl-py`.

pub mod enums;
pub mod errors;
pub mod lexer;
pub mod tokens;

pub use errors::{DslError, DslErrorKind, Result, SourceSpan};
pub use lexer::tokenize;
pub use tokens::{Token, TokenType};
