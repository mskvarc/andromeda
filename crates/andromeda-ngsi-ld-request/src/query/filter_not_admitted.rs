//! The refusal of a derived query that carries a `q` filter no admission covered.

use thiserror::Error;

/// A query built without request validation carried a `q` filter.
///
/// Only request validation admits a `q` (clause 5.5.6, clauses 5.7.2.4 and
/// 5.7.4.4, clause 4.9), so a query constructed from parts that skipped it is
/// admitted only while it names no filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("the query carries a q filter that no request validation admitted")]
pub struct FilterNotAdmitted;
