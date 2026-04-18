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

    /// Extract the conformance-style error code that prefixes the message
    /// (e.g. "SIDMAN_MISSING_SSI" from "SIDMAN_MISSING_SSI: details").
    /// Returns None when the message has no uppercase-code prefix.
    pub fn code(&self) -> Option<&str> {
        let is_code_char = |c: char| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_';
        if !self.message.chars().next().map_or(false, is_code_char) {
            return None;
        }
        let prefix_end = self
            .message
            .find(|c: char| !is_code_char(c))
            .unwrap_or(self.message.len());
        if prefix_end == 0 {
            return None;
        }
        let prefix = &self.message[..prefix_end];
        let rest = &self.message[prefix_end..];
        // The prefix is a code if followed by ':' (message detail), end of
        // string, or any non-code character.
        if rest.starts_with(':') || rest.is_empty() || !rest.chars().next().map_or(false, is_code_char) {
            Some(prefix)
        } else {
            None
        }
    }

    pub fn kind_str(&self) -> &'static str {
        match self.kind {
            DslErrorKind::Lex => "LexError",
            DslErrorKind::Parse => "ParseError",
            DslErrorKind::Semantic => "SemanticError",
        }
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
