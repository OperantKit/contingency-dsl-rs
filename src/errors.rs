//! Error types for contingency-dsl parser.
//!
//! Mirrors `contingency_dsl/errors.py`.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub line: usize,
    pub col: usize,
    pub length: usize,
}

impl SourceSpan {
    pub fn new(line: usize, col: usize) -> Self {
        Self { line, col, length: 1 }
    }
    pub fn with_length(line: usize, col: usize, length: usize) -> Self {
        Self { line, col, length }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DslErrorKind {
    Lex,
    Parse,
    Semantic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DslError {
    pub kind: DslErrorKind,
    pub message: String,
    pub span: Option<SourceSpan>,
}

impl DslError {
    pub fn lex(message: impl Into<String>, span: SourceSpan) -> Self {
        Self { kind: DslErrorKind::Lex, message: message.into(), span: Some(span) }
    }
    pub fn parse(message: impl Into<String>, span: SourceSpan) -> Self {
        Self { kind: DslErrorKind::Parse, message: message.into(), span: Some(span) }
    }
    pub fn semantic(message: impl Into<String>, span: SourceSpan) -> Self {
        Self { kind: DslErrorKind::Semantic, message: message.into(), span: Some(span) }
    }
}

impl fmt::Display for DslError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.span {
            Some(s) => write!(f, "line {}, col {}: {}", s.line, s.col, self.message),
            None => write!(f, "{}", self.message),
        }
    }
}

impl std::error::Error for DslError {}

pub type Result<T> = std::result::Result<T, DslError>;
