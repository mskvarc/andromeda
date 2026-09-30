//! The regular-expression constructs the broker refuses in a pattern.
//!
//! Clause 4.9 mandates IEEE 1003.2 for a `q` pattern and clause 5.7.2 for an
//! `idPattern`. Each variant is either an extension the broker does not
//! support, one of the few IEEE 1003.2 constructs the `regex` crate reads
//! differently from POSIX.2, or a node with no spelling both of the broker's
//! engines read alike. The rendering is what the client is told, so it names
//! the construct the way a client writes it and, where one exists, the
//! spelling to use instead.

use derive_more::Display;

/// A pattern construct outside the dialect the broker admits.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub enum NonPosixConstruct {
    /// A Unicode property class.
    #[display("a Unicode property class (\\p{{...}})")]
    UnicodePropertyClass,
    /// The `\z` anchor.
    #[display("the \\z anchor (write \\Z)")]
    EndTextAnchor,
    /// A word-boundary assertion.
    #[display("a word-boundary assertion (\\b, \\B)")]
    WordBoundaryAssertion,
    /// A numeric character escape.
    #[display("a numeric character escape (\\x61, \\x{{61}}, \\141)")]
    NumericCharacterEscape,
    /// A named capture group.
    #[display("a named capture group")]
    NamedCaptureGroup,
    /// An inline flag group other than case folding.
    #[display("an inline flag group other than (?i:...) and (?-i:...) ((?m:...), (?s:...), (?x:...))")]
    InlineFlagGroup,
    /// A flag letter IEEE 1003.2 has no counterpart for.
    #[display("an embedded option other than i, m, s and x")]
    NonPosixFlag,
    /// A negated embedded option other than case folding.
    #[display("a negated embedded option other than (?-i) ((?-m), (?-s), (?-x))")]
    NegatedEmbeddedOption,
    /// A repetition count above `RE_DUP_MAX`.
    #[display("a repetition count above 255")]
    RepetitionCount,
    /// A bracket expression nested inside another.
    #[display("a bracket expression nested inside another ([[=a=]], [[.a.]], [[abc]])")]
    NestedBracketExpression,
    /// A class set operation.
    #[display("a class set operation ([a&&b], [a--b], [a~~b])")]
    ClassSetOperation,
    /// A negated POSIX character class.
    #[display("a negated character class ([[:^alpha:]]; write [^[:alpha:]])")]
    NegatedPosixClass,
    /// A hyphen neither opening nor closing its bracket expression.
    #[display("an unescaped - inside a bracket expression that neither opens nor closes it (write \\-)")]
    MedialHyphen,
    /// A literal that is not UTF-8.
    #[display("a literal that is not UTF-8")]
    NonUtf8Literal,
    /// A class over bytes.
    #[display("a class over bytes rather than characters")]
    ByteClass,
    /// An assertion with no portable spelling.
    #[display("a word-boundary or CRLF-aware assertion")]
    UnrenderableAssertion,
    /// A class that admits no character.
    #[display("a class that matches no character")]
    EmptyClass,
}

#[cfg(test)]
mod tests {
    use crate::non_posix_construct::NonPosixConstruct;

    #[test]
    fn a_refused_construct_is_named_as_a_client_writes_it() {
        pretty_assertions::assert_eq!(NonPosixConstruct::EndTextAnchor.to_string(), "the \\z anchor (write \\Z)");
        pretty_assertions::assert_eq!(NonPosixConstruct::RepetitionCount.to_string(), "a repetition count above 255");
        pretty_assertions::assert_eq!(NonPosixConstruct::EmptyClass.to_string(), "a class that matches no character");
        pretty_assertions::assert_eq!(NonPosixConstruct::UnicodePropertyClass.to_string(), "a Unicode property class (\\p{...})");
        pretty_assertions::assert_eq!(
            NonPosixConstruct::NumericCharacterEscape.to_string(),
            "a numeric character escape (\\x61, \\x{61}, \\141)"
        );
    }
}
