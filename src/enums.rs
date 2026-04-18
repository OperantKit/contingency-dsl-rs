//! Enumerations for the contingency-dsl type system.
//!
//! Mirrors `contingency_dsl/enums.py`. The 3x3 atomic schedule grid
//! (Distribution x Domain) is realized via [`SCHEDULE_TYPE_KEYWORDS`].
//!
//! References:
//!     Ferster, C. B., & Skinner, B. F. (1957). Schedules of reinforcement.
//!     Catania, A. C. (2013). Learning (5th ed.).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Distribution {
    Fixed,
    Variable,
    Random,
}

impl Distribution {
    pub fn code(self) -> char {
        match self {
            Self::Fixed => 'F',
            Self::Variable => 'V',
            Self::Random => 'R',
        }
    }
    pub const ALL: [Self; 3] = [Self::Fixed, Self::Variable, Self::Random];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Domain {
    Ratio,
    Interval,
    Time,
}

impl Domain {
    pub fn code(self) -> char {
        match self {
            Self::Ratio => 'R',
            Self::Interval => 'I',
            Self::Time => 'T',
        }
    }
    pub const ALL: [Self; 3] = [Self::Ratio, Self::Interval, Self::Time];
}

pub fn is_schedule_type_keyword(word: &str) -> bool {
    if word.len() != 2 {
        return false;
    }
    let bytes = word.as_bytes();
    matches!(bytes[0], b'F' | b'V' | b'R') && matches!(bytes[1], b'R' | b'I' | b'T')
}

pub const COMBINATOR_KEYWORDS: &[&str] = &[
    "Conc", "Alt", "Conj", "Chain", "Tand", "Mult", "Mix",
    "Overlay", "Interpolate", "Interp",
];

pub const DR_KEYWORDS: &[&str] = &["DRL", "DRH", "DRO"];

/// Kw-arg keywords and their canonical forms.
/// Order matters only for longest-match when both a long and short form exist.
pub const KW_ARG_KEYWORDS: &[(&str, &str)] = &[
    ("COD", "COD"),
    ("ChangeoverDelay", "COD"),
    ("FRCO", "FRCO"),
    ("FixedRatioChangeover", "FRCO"),
    ("BO", "BO"),
    ("Blackout", "BO"),
    ("PUNISH", "PUNISH"),
];

pub fn is_kw_arg_keyword(word: &str) -> bool {
    KW_ARG_KEYWORDS.iter().any(|(k, _)| *k == word)
}

pub const SPECIAL_KEYWORDS: &[&str] = &["EXT", "CRF"];

pub const LOWERCASE_RESERVED: &[&str] = &[
    "let", "def", "hodos", "exponential", "linear", "start", "increment",
];

pub const EXPERIMENT_BLOCK_TRIGGERS: &[&str] =
    &["phase", "progressive", "shaping", "experiment"];

pub const DISTRIBUTION_NAMES: &[&str] =
    &["hodos", "linear", "exponential", "geometric", "uniform", "fixed"];

pub const PR_PARAMS: &[&str] = &["start", "increment"];
