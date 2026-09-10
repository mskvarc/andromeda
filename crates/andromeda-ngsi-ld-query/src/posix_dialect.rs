//! Which regular expressions PostgreSQL's POSIX engine runs the same way the
//! `regex` crate does (clause 4.9 `RegExp`).
//!
//! Clause 4.9 states that "`RegExp` shall be a regular expression as mandated by
//! IEEE 1003.2 [11]", and Table 5.2.8-1 says the same of `idPattern`. IEEE
//! 1003.2 is POSIX.2, whose extended regular expressions both engines the
//! broker uses implement faithfully. Neither engine stops there: PostgreSQL's
//! `~` operator is an Advanced Regular Expression and the `regex` crate is a
//! Perl-flavoured superset, and the two supersets do not agree.
//!
//! Validating in one dialect and executing in the other has two failure modes,
//! and this module closes both:
//!
//! * A construct the `regex` crate compiles and PostgreSQL refuses reached SQL
//!   execution, where the refusal is a database error rather than a rejected
//!   request. `idPattern=\p{L}` answered `500 InternalError`; a malformed
//!   pattern is client input and owes the client a `400`.
//! * A construct both engines compile but read differently returns different
//!   entities depending on which engine answered, with nothing to signal it.
//!   `\b` is a word boundary to the `regex` crate and a literal backspace to
//!   PostgreSQL, so one filter silently means two things.
//!
//! Every construct rejected here is outside POSIX.2, so nothing the clause
//! mandates is refused: what is turned away is an extension of one engine that
//! the other cannot run, or cannot run the same way. The rejections were
//! established by executing each construct against both engines rather than
//! from either engine's documentation.

use crate::error::QueryError;
use regex_syntax::ast::{
    Assertion,
    AssertionKind,
    Ast,
    ClassSet,
    ClassSetBinaryOp,
    ClassSetItem,
    ClassSetRange,
    ClassSetUnion,
    Flag,
    FlagsItemKind,
    Group,
    GroupKind,
    Literal,
    LiteralKind,
    SetFlags,
};

/// Reject a pattern PostgreSQL's POSIX engine cannot run, or would run
/// differently from the `regex` crate.
///
/// # Errors
///
/// Returns [`QueryError::PatternRepeatedQuantifier`] when a repetition operator
/// is applied directly to another repetition, and
/// [`QueryError::PatternNonPosixConstruct`] for a construct only one of the two
/// engines supports.
pub fn check_posix_dialect(ast: &Ast) -> Result<(), QueryError> {
    match ast {
        Ast::Repetition(repetition) => {
            // PostgreSQL answers `a**` with "quantifier operand invalid";
            // the `regex` crate compiles it.
            if matches!(repetition.ast.as_ref(), Ast::Repetition(_)) {
                return Err(QueryError::PatternRepeatedQuantifier);
            }
            check_posix_dialect(&repetition.ast)
        }
        Ast::Group(group) => check_group(group),
        Ast::Alternation(alternation) => alternation.asts.iter().try_for_each(check_posix_dialect),
        Ast::Concat(concatenation) => concatenation.asts.iter().try_for_each(check_posix_dialect),
        Ast::ClassBracketed(bracketed) => check_class_set(&bracketed.kind),
        Ast::ClassUnicode(_) => Err(unsupported(UNICODE_PROPERTY_CLASS)),
        Ast::Assertion(assertion) => check_assertion(assertion),
        Ast::Literal(literal) => check_literal(literal),
        Ast::Flags(flags) => check_set_flags(flags),
        // A perl class (`\d`, `\w`, `\s` and their negations) is an ARE
        // shorthand PostgreSQL supports; the empty expression and `.` are
        // POSIX.2 itself.
        Ast::Empty(_) | Ast::Dot(_) | Ast::ClassPerl(_) => Ok(()),
    }
}

/// Reject a group whose delimiter only one engine parses.
///
/// A plain capture and `(?:…)` are common to both. A named capture is spelled
/// `(?P<name>…)` or `(?<name>…)`, and an inline flag group `(?i:…)`; PostgreSQL
/// answers all three with "invalid embedded option" or "quantifier operand
/// invalid".
fn check_group(group: &Group) -> Result<(), QueryError> {
    match &group.kind {
        GroupKind::CaptureIndex(_) => {}
        GroupKind::CaptureName { .. } => return Err(unsupported(NAMED_CAPTURE_GROUP)),
        GroupKind::NonCapturing(flags) => {
            if !flags.items.is_empty() {
                return Err(unsupported(INLINE_FLAG_GROUP));
            }
        }
    }
    check_posix_dialect(&group.ast)
}

/// Reject a standalone flag directive PostgreSQL does not define.
///
/// `(?i)`, `(?m)`, `(?s)` and `(?x)` are embedded options both engines accept.
/// `(?u)`, `(?U)` and `(?R)` are `regex`-crate extensions PostgreSQL answers
/// with "invalid embedded option".
fn check_set_flags(flags: &SetFlags) -> Result<(), QueryError> {
    for item in &flags.flags.items {
        match &item.kind {
            FlagsItemKind::Negation => {}
            FlagsItemKind::Flag(flag) => check_flag(*flag)?,
        }
    }
    Ok(())
}

/// Reject one flag letter PostgreSQL does not define.
const fn check_flag(flag: Flag) -> Result<(), QueryError> {
    match flag {
        Flag::CaseInsensitive | Flag::MultiLine | Flag::DotMatchesNewLine | Flag::IgnoreWhitespace => Ok(()),
        Flag::Unicode | Flag::SwapGreed | Flag::CRLF => Err(unsupported(NON_POSIX_FLAG)),
    }
}

/// Reject an assertion the two engines do not agree on.
///
/// `^`, `$` and `\A` mean the same thing to both. `\z` is a `regex`-crate
/// spelling PostgreSQL answers with "invalid escape \ sequence" — it spells the
/// same anchor `\Z`. The word-boundary assertions are the dangerous case: they
/// compile in both, but PostgreSQL reads `\b` as a backspace character and
/// `\B` as a literal, so the filter matches different entities depending on
/// which engine evaluated it.
const fn check_assertion(assertion: &Assertion) -> Result<(), QueryError> {
    match assertion.kind {
        AssertionKind::StartLine | AssertionKind::EndLine | AssertionKind::StartText => Ok(()),
        AssertionKind::EndText => Err(unsupported(END_TEXT_ANCHOR)),
        AssertionKind::WordBoundary
        | AssertionKind::NotWordBoundary
        | AssertionKind::WordBoundaryStart
        | AssertionKind::WordBoundaryEnd
        | AssertionKind::WordBoundaryStartAngle
        | AssertionKind::WordBoundaryEndAngle
        | AssertionKind::WordBoundaryStartHalf
        | AssertionKind::WordBoundaryEndHalf => Err(unsupported(WORD_BOUNDARY_ASSERTION)),
    }
}

/// Reject a literal written with an escape only the `regex` crate defines.
///
/// A braced hexadecimal escape `\x{61}` is answered by PostgreSQL with "invalid
/// escape \ sequence"; the fixed-width `\x61`, the octal `\141` and the
/// character escapes `\n`, `\t`, `\r`, `\f`, `\v`, `\a` and `\e` are common to
/// both.
const fn check_literal(literal: &Literal) -> Result<(), QueryError> {
    match literal.kind {
        LiteralKind::Verbatim | LiteralKind::Meta | LiteralKind::Superfluous | LiteralKind::Octal | LiteralKind::HexFixed(_) | LiteralKind::Special(_) => {
            Ok(())
        }
        LiteralKind::HexBrace(_) => Err(unsupported(BRACED_HEX_ESCAPE)),
    }
}

/// Walk a bracket expression, which may nest the same constructs.
///
/// A bracketed Unicode property class is rejected for the reason a bare one is:
/// PostgreSQL answers `[\p{L}]` with "invalid escape \ sequence".
fn check_class_set(set: &ClassSet) -> Result<(), QueryError> {
    match set {
        ClassSet::Item(item) => check_class_set_item(item),
        ClassSet::BinaryOp(op) => check_class_set_binary_op(op),
    }
}

/// Walk one member of a bracket expression.
fn check_class_set_item(item: &ClassSetItem) -> Result<(), QueryError> {
    match item {
        ClassSetItem::Unicode(_) => Err(unsupported(UNICODE_PROPERTY_CLASS)),
        ClassSetItem::Literal(literal) => check_literal(literal),
        ClassSetItem::Range(range) => check_class_set_range(range),
        ClassSetItem::Bracketed(bracketed) => check_class_set(&bracketed.kind),
        ClassSetItem::Union(union) => check_class_set_union(union),
        // A POSIX named class (`[:alpha:]`), a perl class and the empty set are
        // all read the same way by both engines.
        ClassSetItem::Empty(_) | ClassSetItem::Ascii(_) | ClassSetItem::Perl(_) => Ok(()),
    }
}

/// Walk the endpoints of a bracketed range.
fn check_class_set_range(range: &ClassSetRange) -> Result<(), QueryError> {
    check_literal(&range.start)?;
    check_literal(&range.end)
}

/// Walk every member of a bracketed union.
fn check_class_set_union(union: &ClassSetUnion) -> Result<(), QueryError> {
    union.items.iter().try_for_each(check_class_set_item)
}

/// Walk both sides of a bracketed set operation.
fn check_class_set_binary_op(op: &ClassSetBinaryOp) -> Result<(), QueryError> {
    check_class_set(&op.lhs)?;
    check_class_set(&op.rhs)
}

/// Name the construct in the rejection a client receives.
const fn unsupported(construct: &'static str) -> QueryError {
    QueryError::PatternNonPosixConstruct { construct }
}

/// How the rejection names a Unicode property class.
const UNICODE_PROPERTY_CLASS: &str = "a Unicode property class (\\p{...})";

/// How the rejection names the `\z` anchor.
const END_TEXT_ANCHOR: &str = "the \\z anchor (write \\Z)";

/// How the rejection names a word-boundary assertion.
const WORD_BOUNDARY_ASSERTION: &str = "a word-boundary assertion (\\b, \\B)";

/// How the rejection names a braced hexadecimal escape.
const BRACED_HEX_ESCAPE: &str = "a braced hexadecimal escape (\\x{...})";

/// How the rejection names a named capture group.
const NAMED_CAPTURE_GROUP: &str = "a named capture group";

/// How the rejection names an inline flag group.
const INLINE_FLAG_GROUP: &str = "an inline flag group ((?i:...))";

/// How the rejection names a flag letter PostgreSQL does not define.
const NON_POSIX_FLAG: &str = "an embedded option other than i, m, s and x";

#[cfg(test)]
mod tests {
    use crate::{error::QueryError, pattern::Pattern};

    /// Every one of these was executed against PostgreSQL 18 and refused with
    /// "invalid escape \ sequence", "invalid embedded option" or "quantifier
    /// operand invalid". Accepting them made the pattern reach SQL, where the
    /// refusal surfaced as `500 InternalError` rather than a rejected request.
    #[test]
    fn a_construct_postgresql_cannot_run_is_refused_before_it_reaches_sql() {
        for pattern in [
            "\\p{L}",
            "\\P{L}",
            "[\\p{L}]",
            "a\\z",
            "\\x{61}",
            "(?P<name>ab)",
            "(?<name>ab)",
            "(?i:ab)",
            "(?u)a",
            "(?U)a+",
            "(?R)a",
        ] {
            let error = Pattern::parse(pattern).expect_err("must be refused");
            assert!(matches!(error, QueryError::PatternNonPosixConstruct { .. }), "{pattern}: got {error:?}");
        }
    }

    /// `\b` compiles in both engines and means two different things: a word
    /// boundary to the `regex` crate, a backspace character to PostgreSQL.
    /// A filter that silently selects different entities depending on which
    /// engine answered it is worse than one that is refused.
    #[test]
    fn a_word_boundary_assertion_the_two_engines_read_differently_is_refused() {
        for pattern in ["\\bword", "a\\Bb"] {
            let error = Pattern::parse(pattern).expect_err("must be refused");
            assert!(matches!(error, QueryError::PatternNonPosixConstruct { .. }), "{pattern}: got {error:?}");
        }
    }

    /// Clause 4.9 mandates IEEE 1003.2, so every POSIX.2 extended regular
    /// expression has to survive validation. Each of these was also executed
    /// against PostgreSQL, which accepted all of them.
    #[test]
    fn every_posix_extended_regular_expression_is_accepted() {
        for pattern in [
            "^A.*",
            "a{2,3}",
            "[[:alpha:]]+",
            "[[=a=]]",
            "[]a]",
            "[^]a]",
            "[a-]",
            "a\\{b",
            "(a*)*",
            "a|b",
            "(ab)+",
            "\\Aab",
            "urn:ngsi-ld:Vehicle:.*",
        ] {
            assert!(Pattern::parse(pattern).is_ok(), "{pattern} is a POSIX.2 extended regular expression");
        }
    }

    /// The ARE shorthands and embedded options PostgreSQL shares with the
    /// `regex` crate stay available: refusing them would turn away patterns
    /// both engines run identically.
    ///
    /// An octal escape (`\141`) is absent because the `regex` crate refuses one
    /// unless its parser is built with octal support, so it never reaches the
    /// dialect walk however readily PostgreSQL runs it.
    #[test]
    fn a_shorthand_both_engines_share_is_still_accepted() {
        for pattern in [
            "\\d+", "\\w", "\\s", "[\\d]", "[\\w]", "(?i)ab", "(?m)^a", "(?s)a", "(?x)a b", "\\x61", "a\\n", "(?:ab)+",
        ] {
            assert!(Pattern::parse(pattern).is_ok(), "{pattern} is common to both engines");
        }
    }

    /// The guard that predates this module: PostgreSQL answers a repetition
    /// applied to a repetition with "quantifier operand invalid".
    #[test]
    fn a_repetition_applied_to_another_repetition_keeps_its_own_rejection() {
        for pattern in ["a**", "invalid_entity_id_pattern**", "(a+)*+"] {
            let error = Pattern::parse(pattern).expect_err("must be refused");
            assert!(matches!(error, QueryError::PatternRepeatedQuantifier), "{pattern}: got {error:?}");
        }
    }

    /// A rejected construct is nested as readily as it is written at the top
    /// level, so the walk has to reach every arm of the tree.
    #[test]
    fn a_rejected_construct_is_found_however_deeply_it_is_nested() {
        for pattern in ["(a|\\p{L})", "(ab)*\\p{L}", "^(x(y\\bz))$", "[a-z]|(?:\\x{61})"] {
            assert!(Pattern::parse(pattern).is_err(), "{pattern} nests a construct only one engine runs");
        }
    }
}

