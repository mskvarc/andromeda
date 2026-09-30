//! The name a `@context` gives one of the terms it declares.

use derive_more::Display;

/// A term a `@context` declares, spelled exactly as the context writes it.
///
/// JSON-LD 1.1 lets a term be any non-empty key that is not a keyword, so the
/// spelling carries no further structure. The type keeps that key apart from the
/// IRIs and values it sits beside, and lets an error name the term a client's
/// context declared.
#[derive(Clone, Debug, Display, Eq, Hash, PartialEq)]
pub struct ContextTerm(Box<str>);

impl ContextTerm {
    /// Keep the key a `@context` wrote for one term.
    #[must_use]
    pub fn new(term: &str) -> ContextTerm {
        ContextTerm(Box::<str>::from(term))
    }

    /// The term exactly as the context wrote it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::context_term::ContextTerm;

    #[test]
    fn a_context_term_keeps_and_renders_its_spelling() {
        let term = ContextTerm::new("speed");
        pretty_assertions::assert_eq!(term.as_str(), "speed");
        pretty_assertions::assert_eq!(term.to_string(), "speed");
    }
}
