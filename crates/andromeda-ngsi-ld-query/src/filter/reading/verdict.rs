//! What a value test makes of one target of one kind.

use crate::filter::reading::scalar_test::ScalarTest;

/// The answer a value test gives a target of one kind, derived and never
/// stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict<'t> {
    /// No target of the kind satisfies the term.
    Never,
    /// Every target of the kind satisfies the term.
    Always,
    /// A target of the kind satisfies the term when it passes the test.
    Holds(&'t ScalarTest),
    /// A target of the kind satisfies the term when it fails the test.
    HoldsNot(&'t ScalarTest),
}
