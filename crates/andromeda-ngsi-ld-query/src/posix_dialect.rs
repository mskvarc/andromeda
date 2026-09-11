//! Which regular expressions PostgreSQL's POSIX engine runs the same way the
//! `regex` crate does (clause 4.9 `RegExp`).
//!
//! Clause 4.9 states that "`RegExp` shall be a regular expression as mandated by
//! IEEE 1003.2 [11]", and Table 5.2.8-1 says the same of `idPattern`. IEEE
//! 1003.2 is POSIX.2, whose extended regular expressions both engines the
//! broker uses largely implement alike. Neither engine stops there: PostgreSQL's
//! `~` operator is an Advanced Regular Expression and the `regex` crate is a
//! Perl-flavoured superset, and the two supersets do not agree. Neither engine
//! reaches POSIX.2 in full, either: the `regex` crate has no parse path at all
//! for a bracket expression's equivalence classes and collating symbols.
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
//! Most of what is rejected here is outside POSIX.2 — an extension of one engine
//! the other cannot run, or cannot run the same way. Three rejections are not,
//! and are stated plainly because they narrow what clause 4.9 mandates:
//! a bracket expression nested inside another, which is how the `regex` crate
//! parses POSIX.2's equivalence classes (`[[=a=]]`) and collating symbols
//! (`[[.a.]]`); a hyphen in a position where POSIX.2 makes it a range operator
//! and the `regex` crate makes it a literal; and a negated POSIX character
//! class, which POSIX.2 does not define but PostgreSQL is asked to run.
//! `docs/spec-conformance-sweep.md` records why each is refused rather than
//! translated.
//!
//! Every rejection was established by executing the construct against both
//! engines — PostgreSQL 18.3 over a `COLLATE "C"` operand, which is the
//! collation `docs/text-collation-invariant.md` pins on every column a pattern
//! is applied to — rather than from either engine's documentation.

use crate::error::QueryError;
use regex_syntax::ast::{
    Assertion,
    AssertionKind,
    Ast,
    ClassAscii,
    ClassSet,
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
/// [`QueryError::PatternNonPosixConstruct`] for a construct the two engines do
/// not run identically.
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
        Ast::ClassPerl(_) => Err(unsupported(PERL_SHORTHAND_CLASS)),
        Ast::Assertion(assertion) => check_assertion(assertion),
        Ast::Literal(literal) => check_literal(literal),
        Ast::Flags(flags) => check_set_flags(flags),
        // The empty expression and `.` are POSIX.2 itself. `.` matches a
        // newline in both engines because [`crate::pattern::Pattern`] compiles
        // with `dot_matches_new_line`, which is PostgreSQL's default and the
        // POSIX.2 reading.
        Ast::Empty(_) | Ast::Dot(_) => Ok(()),
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

/// Reject a standalone flag directive PostgreSQL does not define or reads
/// differently.
///
/// PostgreSQL has no negated embedded option at all: `(?-s)`, `(?-i)` and
/// `(?-x)` are each answered with "quantifier operand invalid", because its
/// parser reads the `-` as a quantifier applied to nothing.
fn check_set_flags(flags: &SetFlags) -> Result<(), QueryError> {
    for item in &flags.flags.items {
        match &item.kind {
            FlagsItemKind::Negation => return Err(unsupported(NEGATED_EMBEDDED_OPTION)),
            FlagsItemKind::Flag(flag) => check_flag(*flag)?,
        }
    }
    Ok(())
}

/// Reject one flag letter PostgreSQL does not define or reads differently.
///
/// `(?m)`, `(?s)` and `(?x)` are embedded options both engines read the same
/// way. `(?i)` is not: the `regex` crate case-folds by Unicode simple case
/// folding, so `(?i)é` matches `É` in memory, while PostgreSQL over a
/// `COLLATE "C"` operand folds only ASCII and does not. `(?u)`, `(?U)` and
/// `(?R)` are `regex`-crate extensions PostgreSQL answers with "invalid
/// embedded option".
const fn check_flag(flag: Flag) -> Result<(), QueryError> {
    match flag {
        Flag::MultiLine | Flag::DotMatchesNewLine | Flag::IgnoreWhitespace => Ok(()),
        Flag::CaseInsensitive => Err(unsupported(CASE_INSENSITIVE_FLAG)),
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

/// Reject a literal written with a numeric character escape, which the two
/// engines spell to different widths.
///
/// PostgreSQL's `\xhhh` consumes every hexadecimal digit that follows it and
/// yields one character, while the `regex` crate's `\xhh` is exactly two
/// digits: `Merc\x33de` is `Merc` followed by U+33DE in the store and
/// `Merc3de` in memory, so it selects `Merc3des` from one engine and nothing
/// from the other. The braced form `\x{61}` PostgreSQL answers outright with
/// "invalid escape \ sequence", and the octal form has widths of its own.
/// None of the three is IEEE 1003.2, which gives a backslash no meaning beyond
/// escaping a metacharacter, so refusing them turns away nothing clause 4.9
/// mandates.
///
/// The character escapes `\n`, `\t`, `\r`, `\f`, `\v`, `\a` and `\e` are of
/// fixed width in both engines and were executed against both.
const fn check_literal(literal: &Literal) -> Result<(), QueryError> {
    match literal.kind {
        LiteralKind::Verbatim | LiteralKind::Meta | LiteralKind::Superfluous | LiteralKind::Special(_) => Ok(()),
        LiteralKind::Octal | LiteralKind::HexFixed(_) | LiteralKind::HexBrace(_) => Err(unsupported(NUMERIC_CHARACTER_ESCAPE)),
    }
}

/// Walk a bracket expression, which may nest the same constructs.
///
/// A set operation between two class sets — `[a-z&&b-d]`, `[[a-z]--[aeiou]]`,
/// `[[a-z]~~[a-c]]` — is a `regex`-crate extension with no POSIX.2 counterpart.
/// PostgreSQL reads `&`, `-` and `~` as ordinary members, so `[a-z&&b-d]` is
/// the intersection `[b-d]` in memory and the union of `a-z`, `&` and `b-d` in
/// the store.
fn check_class_set(set: &ClassSet) -> Result<(), QueryError> {
    match set {
        ClassSet::Item(item) => check_class_set_item(item),
        ClassSet::BinaryOp(_) => Err(unsupported(CLASS_SET_OPERATION)),
    }
}

/// Walk one member of a bracket expression.
///
/// A bracketed Unicode property class is rejected for the reason a bare one is:
/// PostgreSQL answers `[\p{L}]` with "invalid escape \ sequence".
fn check_class_set_item(item: &ClassSetItem) -> Result<(), QueryError> {
    match item {
        ClassSetItem::Unicode(_) => Err(unsupported(UNICODE_PROPERTY_CLASS)),
        ClassSetItem::Perl(_) => Err(unsupported(PERL_SHORTHAND_CLASS)),
        ClassSetItem::Bracketed(_) => Err(unsupported(NESTED_BRACKET_EXPRESSION)),
        ClassSetItem::Ascii(class) => check_ascii_class(class),
        ClassSetItem::Literal(literal) => check_literal(literal),
        ClassSetItem::Range(range) => check_class_set_range(range),
        ClassSetItem::Union(union) => check_class_set_union(union),
        // An empty bracket expression carries nothing either engine can read
        // differently.
        ClassSetItem::Empty(_) => Ok(()),
    }
}

/// Reject a negated POSIX character class.
///
/// `[[:alpha:]]` names the same set in both engines, for every one of the
/// fourteen class names the `regex` crate defines. `[[:^alpha:]]` is the
/// `regex`-crate spelling of its complement, which POSIX.2 does not define and
/// PostgreSQL answers with "invalid character class"; the portable spelling of
/// the same set is `[^[:alpha:]]`, which both engines run.
const fn check_ascii_class(class: &ClassAscii) -> Result<(), QueryError> {
    if class.negated { Err(unsupported(NEGATED_POSIX_CLASS)) } else { Ok(()) }
}

/// Walk the endpoints of a bracketed range.
fn check_class_set_range(range: &ClassSetRange) -> Result<(), QueryError> {
    check_literal(&range.start)?;
    check_literal(&range.end)
}

/// Walk every member of a bracketed union, rejecting a hyphen the two engines
/// place differently.
///
/// POSIX.2 makes an unescaped `-` a range operator wherever a member precedes
/// it and a member follows it, and a literal only when it opens or closes the
/// list. The `regex` crate stops earlier: after a range or a character class it
/// reads the next `-` as a literal. So `[a-b-c]` and `[[:alpha:]-z]` are a
/// malformed range to PostgreSQL — "invalid character range", reaching the
/// client as `500 InternalError` — and three ordinary members to the `regex`
/// crate, while `[]-a]` and `[--/]` are ranges to PostgreSQL and member lists
/// to the `regex` crate. A `-` that opens or closes the list, and an escaped
/// `\-` anywhere, are read the same way by both.
fn check_class_set_union(union: &ClassSetUnion) -> Result<(), QueryError> {
    let last = union.items.len().saturating_sub(1);
    for (position, item) in union.items.iter().enumerate() {
        if position != 0 && position != last && is_bare_hyphen(item) {
            return Err(unsupported(MEDIAL_HYPHEN));
        }
        check_class_set_item(item)?;
    }
    Ok(())
}

/// Whether a member of a bracket expression is an unescaped `-`.
const fn is_bare_hyphen(item: &ClassSetItem) -> bool {
    match item {
        ClassSetItem::Literal(literal) => matches!(literal.kind, LiteralKind::Verbatim) && literal.c == '-',
        ClassSetItem::Empty(_)
        | ClassSetItem::Range(_)
        | ClassSetItem::Ascii(_)
        | ClassSetItem::Unicode(_)
        | ClassSetItem::Perl(_)
        | ClassSetItem::Bracketed(_)
        | ClassSetItem::Union(_) => false,
    }
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

/// How the rejection names a numeric character escape.
const NUMERIC_CHARACTER_ESCAPE: &str = "a numeric character escape (\\x61, \\x{61}, \\141)";

/// How the rejection names a named capture group.
const NAMED_CAPTURE_GROUP: &str = "a named capture group";

/// How the rejection names an inline flag group.
const INLINE_FLAG_GROUP: &str = "an inline flag group ((?i:...))";

/// How the rejection names a flag letter PostgreSQL does not define.
const NON_POSIX_FLAG: &str = "an embedded option other than m, s and x";

/// How the rejection names a negated embedded option.
const NEGATED_EMBEDDED_OPTION: &str = "a negated embedded option ((?-i), (?-s), (?-x))";

/// How the rejection names the case-insensitive flag.
const CASE_INSENSITIVE_FLAG: &str = "the case-insensitive option ((?i)), which folds only ASCII in the store";

/// How the rejection names a Perl shorthand class.
const PERL_SHORTHAND_CLASS: &str = "a Perl shorthand class (\\d, \\w, \\s and their negations), which matches only ASCII in the store";

/// How the rejection names a bracket expression nested inside another.
const NESTED_BRACKET_EXPRESSION: &str = "a bracket expression nested inside another ([[=a=]], [[.a.]], [[abc]])";

/// How the rejection names a class set operation.
const CLASS_SET_OPERATION: &str = "a class set operation ([a&&b], [a--b], [a~~b])";

/// How the rejection names a negated POSIX character class.
const NEGATED_POSIX_CLASS: &str = "a negated character class ([[:^alpha:]]; write [^[:alpha:]])";

/// How the rejection names a hyphen neither opening nor closing its list.
const MEDIAL_HYPHEN: &str = "an unescaped - inside a bracket expression that neither opens nor closes it (write \\-)";

#[cfg(test)]
mod tests {
    use crate::{error::QueryError, pattern::Pattern};

    /// Assert that every pattern is refused as a construct the two engines do
    /// not share.
    fn each_is_refused(patterns: &[&str]) {
        for pattern in patterns {
            let error = Pattern::parse(pattern).expect_err("must be refused");
            assert!(matches!(error, QueryError::PatternNonPosixConstruct { .. }), "{pattern}: got {error:?}");
        }
    }

    /// Every one of these was executed against PostgreSQL 18 and refused with
    /// "invalid escape \ sequence", "invalid embedded option", "invalid
    /// character class", "invalid character range" or "quantifier operand
    /// invalid". Accepting them made the pattern reach SQL, where the refusal
    /// surfaced as `500 InternalError` rather than a rejected request.
    #[test]
    fn a_construct_postgresql_cannot_run_is_refused_before_it_reaches_sql() {
        each_is_refused(&[
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
            "(?-i)a",
            "(?-s)a.b",
            "(?-x)a b",
            "\\x{61}",
            "[[:^alpha:]]",
            "[a-b-c]",
            "[[:alpha:]-z]",
        ]);
    }

    /// `\b` compiles in both engines and means two different things: a word
    /// boundary to the `regex` crate, a backspace character to PostgreSQL.
    /// A filter that silently selects different entities depending on which
    /// engine answered it is worse than one that is refused.
    #[test]
    fn a_word_boundary_assertion_the_two_engines_read_differently_is_refused() {
        each_is_refused(&["\\bword", "a\\Bb"]);
    }

    /// The `regex` crate has no parse path for POSIX.2's equivalence classes or
    /// collating symbols, so it reads `[[=a=]]` as a nested class over
    /// `{'=', 'a'}` and `[[.hyphen.]]` as one over `{'.','h','y','p','e','n'}`,
    /// while PostgreSQL reads them as `a` and `-`. Nesting a plain bracket
    /// expression is the `regex` crate's own extension and diverges the same
    /// way: `[[abc]]` is `[abc]` in memory and `[abc]` followed by a literal
    /// `]` in the store. A mistyped or unknown class name — `[[:alpha]]`,
    /// `[[:foo:]]` — takes the same parse and is refused outright by
    /// PostgreSQL.
    ///
    /// Refusing these narrows what clause 4.9 mandates. It is deliberate: no
    /// reading of `[[=a=]]` exists that both engines share, and translating one
    /// engine's reading into the other's would depend on the collating-element
    /// table of whichever collation the store happens to hold.
    #[test]
    fn a_bracket_expression_nested_inside_another_is_refused() {
        each_is_refused(&[
            "[[=a=]]",
            "^[[=a=]]$",
            "[[=a=]x]",
            "[[=a=][:digit:]]",
            "[[.a.]]",
            "[[.hyphen.]]",
            "[[abc]]",
            "[[a-c]]",
            "[x[abc]]",
            "[[:alpha]]",
            "[[:foo:]]",
        ]);
    }

    /// A set operation between two class sets is a `regex`-crate extension with
    /// no POSIX.2 counterpart. PostgreSQL reads its operator characters as
    /// ordinary members: `[a-z&&b-d]` selects `b`, `c` and `d` in memory and
    /// also `a`, `e`…`z` and `&` in the store.
    #[test]
    fn a_class_set_operation_is_refused() {
        each_is_refused(&["[a-z&&b-d]", "[[a-z]--[aeiou]]", "[[a-z]~~[a-c]]", "[+--]", "[a-b--d]"]);
    }

    /// POSIX.2 makes an unescaped `-` a range operator wherever a member
    /// precedes it and a member follows it; the `regex` crate reads it as a
    /// literal once a range or a character class precedes it. `[]-a]` is the
    /// range `]`…`a` in the store and the three members `]`, `-`, `a` in
    /// memory, and `[--/]` is the range `-`…`/` in the store and two members in
    /// memory.
    #[test]
    fn a_hyphen_the_two_engines_place_differently_is_refused() {
        each_is_refused(&["[]-a]", "[--/]", "[a-b-c]", "[[:alpha:]-z]"]);
    }

    /// PostgreSQL's `\xhhh` consumes every hexadecimal digit that follows it,
    /// so `Merc\x33de` is `Merc` followed by U+33DE in the store; the `regex`
    /// crate reads exactly two digits, so the same pattern is `Merc3de` in
    /// memory and selects the stored `Merc3des` the store does not. A lone
    /// `\x61` happens to agree, but the escape has one width or the other and
    /// the parse cannot see what follows it, so the form is refused.
    #[test]
    fn a_numeric_character_escape_the_two_engines_size_differently_is_refused() {
        each_is_refused(&["\\x61", "Merc\\x33de", "[\\x61]", "\\x{61}", "a[\\x41-\\x5A]"]);
    }

    /// The Perl shorthand classes compile in both engines and classify
    /// differently: the `regex` crate applies Unicode properties, so `\w`
    /// matches `é` and `ß`, `\d` matches `٣` (U+0663) and `\s` matches U+00A0,
    /// while PostgreSQL over the `COLLATE "C"` operand every pattern is applied
    /// to classifies only ASCII and matches none of them. `(?i)` diverges the
    /// same way: `(?i)é` matches `É` in memory and nothing but `é` in the store.
    #[test]
    fn a_unicode_aware_shorthand_the_two_engines_read_differently_is_refused() {
        each_is_refused(&["\\d+", "\\w", "\\s", "\\D", "\\W", "\\S", "[\\d]", "[\\w]", "[\\s]", "(?i)ab"]);
    }

    /// Clause 4.9 mandates IEEE 1003.2, so every POSIX.2 extended regular
    /// expression that survives has to mean in memory what it means in the
    /// store. Each row states what the pattern selects: the two lists were
    /// produced by running the pattern against both engines, and the
    /// PostgreSQL 18.3 verdict over a `COLLATE "C"` operand is what the
    /// expectations record.
    #[test]
    fn every_posix_extended_regular_expression_is_accepted_and_means_what_postgresql_means() {
        for (raw, matching, rejecting) in [
            ("^A.*", &["A", "Abc"][..], &["bA", "abc"][..]),
            ("a{2,3}", &["aa", "aaa"], &["a", "b"]),
            ("[[:alpha:]]+", &["abc", "A"], &["123", "é", "ß"]),
            ("[]a]", &["a", "]"], &["b", "["]),
            ("[^]a]", &["b", "["], &["a", "]"]),
            ("[a-]", &["a", "-"], &["b"]),
            ("[-a]", &["a", "-"], &["b"]),
            ("[-a-]", &["a", "-"], &["b"]),
            ("[a-c-]", &["a", "c", "-"], &["d"]),
            ("[.-/]", &[".", "/"], &["a", "-"]),
            ("[[:alpha:]-]", &["a", "-"], &["1"]),
            ("[a\\-c]", &["a", "c", "-"], &["b"]),
            ("[\\-]", &["-"], &["a"]),
            ("[^[:alpha:]]", &["1", "="], &["a", "Z"]),
            ("a\\{b", &["a{b"], &["ab"]),
            ("(a*)*", &["", "aaa"], &[]),
            ("a|b", &["a", "b"], &["c"]),
            ("(ab)+", &["ab", "abab"], &["a"]),
            ("\\Aab", &["abc"], &["xab"]),
            ("urn:ngsi-ld:Vehicle:.*", &["urn:ngsi-ld:Vehicle:A4567"], &["urn:ngsi-ld:Device:1"]),
        ] {
            let pattern = Pattern::parse(raw).unwrap_or_else(|error| panic!("{raw} is a POSIX.2 extended regular expression: {error:?}"));
            for subject in matching {
                assert!(pattern.is_match(subject), "{raw} must match {subject:?}");
            }
            for subject in rejecting {
                assert!(!pattern.is_match(subject), "{raw} must not match {subject:?}");
            }
        }
    }

    /// The extensions PostgreSQL shares with the `regex` crate stay available:
    /// refusing them would turn away patterns both engines run identically.
    ///
    /// An octal escape (`\141`) is absent because the `regex` crate refuses one
    /// unless its parser is built with octal support, so it never reaches the
    /// dialect walk however readily PostgreSQL runs it.
    #[test]
    fn a_shorthand_both_engines_share_is_still_accepted() {
        for pattern in [
            "(?m)^a",
            "(?s)a",
            "(?x)a b",
            "a\\n",
            "a\\t",
            "(?:ab)+",
            "[[:word:]]",
            "[[:ascii:]]",
            "[a[:digit:]]",
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
        for pattern in [
            "(a|\\p{L})",
            "(ab)*\\p{L}",
            "^(x(y\\bz))$",
            "[a-z]|(?:\\x{61})",
            "(a|[[=a=]])",
            "^(x[[:^alpha:]])$",
            "(ab)*[]-a]",
        ] {
            assert!(Pattern::parse(pattern).is_err(), "{pattern} nests a construct only one engine runs");
        }
    }
}
