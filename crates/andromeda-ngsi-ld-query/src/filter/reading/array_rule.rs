//! What a value test makes of a target holding several values.

/// The answer a value test gives an array target, derived and never stored.
///
/// An element is always tested with the asserted test, so
/// [`ArrayRule::NoElement`] reads "no element satisfies the asserted test".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrayRule {
    /// No array satisfies the term.
    Never,
    /// Every array satisfies the term.
    Always,
    /// An array satisfies the term when some element passes the test.
    SomeElement,
    /// An array satisfies the term when no element passes the test.
    NoElement,
}
