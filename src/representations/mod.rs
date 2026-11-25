//! Alternative representations of schedules.
//!
//! A representation is another parameterization of the behavioral space of
//! the 3×3 lattice (Distribution × Domain), with a conversion in each
//! direction. The exact conversions keep annotations.

pub mod t_tau;

pub use t_tau::{
    approximate_ttau, from_ttau, to_ttau, IrtAssumption, TTauApprox, TTauError, TTauParams,
};
