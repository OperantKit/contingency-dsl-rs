//! Token types and Token struct for the contingency-dsl lexer.
//!
//! Mirrors `contingency_dsl/tokens.py` in the Python reference
//! implementation. Each variant corresponds to a single terminal in the
//! DSL grammar (`schema/operant/grammar.ebnf`).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TokenType {
    ScheduleType,
    Number,
    TimeUnit,
    LParen,
    RParen,
    Comma,
    Equals,
    Arrow,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Dot,
    Minus,
    Let,
    Lh,
    Rd,
    Ext,
    Crf,
    Combinator,
    KwArgName,
    DrMod,
    Pr,
    Repeat,
    DistributionName,
    PrParam,
    Identifier,
    At,
    StringLiteral,
    Colon,
    Newline,
    Indent,
    Dedent,
    Gte,
    Percent,
    Phase,
    Sessions,
    Stable,
    Use,
    NoSchedule,
    Shaping,
    Progressive,
    Steps,
    Interleave,
    NoTrailing,
    Advance,
    When,
    Experiment,
    Placeholder,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub ty: TokenType,
    pub value: String,
    pub line: usize,
    pub col: usize,
}

impl Token {
    pub fn new(ty: TokenType, value: impl Into<String>, line: usize, col: usize) -> Self {
        Self { ty, value: value.into(), line, col }
    }
}
