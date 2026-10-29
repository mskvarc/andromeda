//! One place a compared document departs from the expectation, and what differs there.

use crate::comparison::document_path::DocumentPath;
use serde_json::{Number, Value};
use std::fmt;

/// One way the answer departs from the expectation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    /// Where, as a JSON path from the compared root.
    pub path: DocumentPath,
    /// What differs.
    pub discrepancy: Discrepancy,
}

impl Difference {
    /// The difference `discrepancy` at `path`.
    pub(crate) fn at(path: &DocumentPath, discrepancy: Discrepancy) -> Difference {
        Difference {
            // clone: each difference owns the path it names
            path: path.clone(),
            discrepancy,
        }
    }
}

// Hand-written: the path leads the discrepancy it locates.
impl fmt::Display for Difference {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.discrepancy)
    }
}

/// What differs at one place of a compared document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Discrepancy {
    /// The value is not the expected one.
    #[error("is {actual}, expected {expected}")]
    Unequal {
        /// The value the answer holds.
        actual: Value,
        /// The value expected.
        expected: Value,
    },
    /// An expected member is absent.
    #[error("is missing, expected {0}")]
    Missing(Value),
    /// An expected list item, paired by `id`, is absent.
    #[error("is missing")]
    MissingItem,
    /// A member is present that the expectation does not hold.
    #[error("is unexpected, holding {0}")]
    Unexpected(Value),
    /// A list holds another number of items.
    #[error("holds {actual} items, expected {expected}")]
    Length {
        /// The number of items the answer holds.
        actual: usize,
        /// The number of items expected.
        expected: usize,
    },
    /// No item of a list agrees with an expected one.
    #[error("holds no item agreeing with {0}")]
    Unpaired(Value),
    /// More items are left unpaired by their hash than DeepDiff pairs member by member.
    #[error("holds {0} items no expected item hash-pairs, more than DeepDiff pairs")]
    BeyondPairingCutoff(usize),
    /// An `observedAt` names another instant.
    #[error("is {actual}, expected the instant {expected}")]
    OtherInstant {
        /// The value the answer holds.
        actual: Value,
        /// The instant expected.
        expected: Value,
    },
    /// A system timestamp is not a well-formed date.
    #[error("is {0}, expected a well-formed date")]
    MalformedDate(Value),
    /// A number has the expected value in the other of the integer and float types.
    #[error("is {actual}, expected {expected} of the other number type")]
    OtherNumberType {
        /// The number the answer holds.
        actual: Number,
        /// The number expected.
        expected: Number,
    },
}
