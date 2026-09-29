//! Which regular expressions the broker admits (clause 4.9 `RegExp`).
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
//! The store never compiles the client's text: it runs the
//! [`crate::portable_pattern::PortablePattern`] rendered from what the `regex`
//! crate compiled, so whatever is admitted here means the same in memory and in
//! the store. What this module decides is what a pattern may mean at all. It
//! admits IEEE 1003.2 as the `regex` crate reads it, plus the extensions the
//! two engines share, the case-insensitive option and the shorthand classes
//! `\d`, `\w` and `\s` with their negations, which both engines read as the
//! `regex` crate's Unicode classes. It refuses two kinds of construct:
//!
//! * An extension the broker does not support. `\p{L}`, `\x61` or a named group
//!   has no meaning in IEEE 1003.2, and a client relying on one is relying on
//!   whichever engine it last used; `\b` is a word boundary to the `regex` crate
//!   and a backspace to PostgreSQL, so no reading of it is the obvious one.
//! * An IEEE 1003.2 construct the `regex` crate reads differently from POSIX.2.
//!   Rendering follows the `regex` crate's reading, so admitting one would make
//!   both engines agree on something the client did not write.
//!
//! Three rejections of the second kind are stated plainly because they narrow
//! what clause 4.9 mandates: a bracket expression nested inside another, which
//! is how the `regex` crate parses POSIX.2's equivalence classes (`[[=a=]]`)
//! and collating symbols (`[[.a.]]`); a hyphen in a position where POSIX.2 makes
//! it a range operator and the `regex` crate makes it a literal; and a negated
//! POSIX character class, which POSIX.2 does not define.
//! `docs/spec-conformance-sweep.md` records why each is refused rather than
//! translated.
//!
//! Every reading recorded here was established by executing the construct
//! against both engines — PostgreSQL 18 over a `COLLATE "C"` operand, which is
//! the collation `docs/text-collation-invariant.md` pins on every column a
//! pattern is applied to — rather than from either engine's documentation.

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
    Flags,
    FlagsItemKind,
    Group,
    GroupKind,
    Literal,
    LiteralKind,
    RepetitionKind,
    RepetitionRange,
    SetFlags,
};

/// Reject a pattern outside the dialect the broker admits.
///
/// # Errors
///
/// Returns [`QueryError::PatternRepeatedQuantifier`] when a repetition operator
/// is applied directly to another repetition, and
/// [`QueryError::PatternNonPosixConstruct`] for a construct the broker does not
/// admit.
pub fn check_posix_dialect(ast: &Ast) -> Result<(), QueryError> {
    match ast {
        Ast::Repetition(repetition) => {
            // IEEE 1003.2 gives `a**` no meaning and PostgreSQL answers it with
            // "quantifier operand invalid"; the `regex` crate compiles it.
            if matches!(repetition.ast.as_ref(), Ast::Repetition(_)) {
                return Err(QueryError::PatternRepeatedQuantifier);
            }
            check_repetition_count(&repetition.op.kind)?;
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
        // The empty expression and `.` are POSIX.2 itself. `.` matches a
        // newline in both engines because [`crate::pattern::Pattern`] compiles
        // with `dot_matches_new_line`, which is PostgreSQL's default and the
        // POSIX.2 reading. PostgreSQL's own `\d`, `\w` and `\s` classify by
        // the operand's collation, which is ASCII alone under `COLLATE "C"`,
        // but the store never sees the shorthand: the restatement writes out
        // the Unicode class the `regex` crate compiled it to (`Nd`, the UTS #18
        // word characters, `White_Space`), see
        // [`crate::portable_pattern::PortablePattern`].
        Ast::Empty(_) | Ast::Dot(_) | Ast::ClassPerl(_) => Ok(()),
    }
}

/// Reject a repetition count above the POSIX.2 bound.
///
/// IEEE 1003.2 guarantees counts up to `RE_DUP_MAX`, at least 255, and
/// PostgreSQL stops there: `a{256}` is "invalid repetition count(s)". The
/// `regex` crate accepts far larger counts, and the restatement the store runs
/// carries the count unchanged.
const fn check_repetition_count(kind: &RepetitionKind) -> Result<(), QueryError> {
    match kind {
        RepetitionKind::ZeroOrOne | RepetitionKind::ZeroOrMore | RepetitionKind::OneOrMore => Ok(()),
        RepetitionKind::Range(RepetitionRange::Exactly(count) | RepetitionRange::AtLeast(count)) => bounded_count(*count),
        RepetitionKind::Range(RepetitionRange::Bounded(min, max)) => bounded_count(if *min > *max { *min } else { *max }),
    }
}

/// Whether one repetition count is within `RE_DUP_MAX`.
const fn bounded_count(count: u32) -> Result<(), QueryError> {
    if count > RE_DUP_MAX { Err(unsupported(REPETITION_COUNT)) } else { Ok(()) }
}

/// The largest repetition count IEEE 1003.2 guarantees and PostgreSQL accepts.
const RE_DUP_MAX: u32 = 255;

/// Reject a group the broker does not admit.
///
/// A plain capture and `(?:…)` are IEEE 1003.2 and its common extension. A
/// named capture, spelled `(?P<name>…)` or `(?<name>…)`, is an extension of the
/// `regex` crate alone. A flag group is admitted only for the case-insensitive
/// option, see [`check_group_flags`].
fn check_group(group: &Group) -> Result<(), QueryError> {
    match &group.kind {
        GroupKind::CaptureIndex(_) => {}
        GroupKind::CaptureName { .. } => return Err(unsupported(NAMED_CAPTURE_GROUP)),
        GroupKind::NonCapturing(flags) => check_group_flags(flags)?,
    }
    check_posix_dialect(&group.ast)
}

/// Reject a flag group other than `(?i:…)`, `(?-i:…)` or their combination.
///
/// Scoping the case-insensitive option to a group is how a client folds case
/// in one part of a pattern, such as a name, and not in another, such as an
/// identifier prefix. The store never sees the group:
/// [`crate::portable_pattern::PortablePattern`] writes out the fold sets it
/// governs. Scoping `m`, `s` or `x` would render as faithfully, but the
/// admitted set is kept to the option the both-engine integration suite
/// (`tests/unicode_pattern`) verifies.
fn check_group_flags(flags: &Flags) -> Result<(), QueryError> {
    flags.items.iter().try_for_each(|item| match &item.kind {
        FlagsItemKind::Negation | FlagsItemKind::Flag(Flag::CaseInsensitive) => Ok(()),
        FlagsItemKind::Flag(Flag::MultiLine | Flag::DotMatchesNewLine | Flag::IgnoreWhitespace) => Err(unsupported(INLINE_FLAG_GROUP)),
        FlagsItemKind::Flag(Flag::Unicode | Flag::SwapGreed | Flag::CRLF) => Err(unsupported(NON_POSIX_FLAG)),
    })
}

/// Reject a standalone flag directive the broker does not admit.
///
/// Every option may be set; only the case-insensitive one may be cleared,
/// `(?-i)`, which is how a pattern ends a case-insensitive span. Clearing `m`,
/// `s` or `x` is refused for the reason [`check_group_flags`] gives.
fn check_set_flags(flags: &SetFlags) -> Result<(), QueryError> {
    let mut negated = false;
    for item in &flags.flags.items {
        match &item.kind {
            FlagsItemKind::Negation => negated = true,
            FlagsItemKind::Flag(Flag::CaseInsensitive) => {}
            FlagsItemKind::Flag(flag) if negated => {
                check_flag(*flag)?;
                return Err(unsupported(NEGATED_EMBEDDED_OPTION));
            }
            FlagsItemKind::Flag(flag) => check_flag(*flag)?,
        }
    }
    Ok(())
}

/// Reject one flag letter the broker does not admit.
///
/// `(?i)`, `(?m)`, `(?s)` and `(?x)` are admitted: the store runs a
/// restatement with each already applied, so it reads them as the `regex` crate
/// does, including `(?i)`'s Unicode simple case folding and `(?m)`'s line
/// anchors, which PostgreSQL's own options read differently. `(?u)`, `(?U)` and
/// `(?R)` are `regex`-crate extensions with no counterpart in IEEE 1003.2.
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
/// `\B` as a literal, so the same text means two different filters depending
/// on the engine a client last used, and no portable construct restates the
/// `regex` crate's reading.
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
/// digits: `Merc\x33de` is `Merc` followed by U+33DE to PostgreSQL and
/// `Merc3de` to the `regex` crate, so the same text selects `Merc3des` from one
/// engine and nothing from the other. The braced form `\x{61}` PostgreSQL answers outright with
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
/// IEEE 1003.2 and PostgreSQL read `&`, `-` and `~` as ordinary members, so
/// `[a-z&&b-d]` is the intersection `[b-d]` to the `regex` crate and the union
/// of `a-z`, `&` and `b-d` to POSIX.2.
fn check_class_set(set: &ClassSet) -> Result<(), QueryError> {
    match set {
        ClassSet::Item(item) => check_class_set_item(item),
        ClassSet::BinaryOp(_) => Err(unsupported(CLASS_SET_OPERATION)),
    }
}

/// Walk one member of a bracket expression.
///
/// A bracketed Unicode property class is rejected for the reason a bare one is:
/// PostgreSQL answers `[\p{L}]` with "invalid escape \ sequence". A bracketed
/// shorthand class is admitted for the reason a bare one is, see
/// [`check_posix_dialect`].
fn check_class_set_item(item: &ClassSetItem) -> Result<(), QueryError> {
    match item {
        ClassSetItem::Unicode(_) => Err(unsupported(UNICODE_PROPERTY_CLASS)),
        ClassSetItem::Bracketed(_) => Err(unsupported(NESTED_BRACKET_EXPRESSION)),
        ClassSetItem::Ascii(class) => check_ascii_class(class),
        ClassSetItem::Literal(literal) => check_literal(literal),
        ClassSetItem::Range(range) => check_class_set_range(range),
        ClassSetItem::Union(union) => check_class_set_union(union),
        // An empty bracket expression carries nothing either engine can read
        // differently.
        ClassSetItem::Empty(_) | ClassSetItem::Perl(_) => Ok(()),
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
/// malformed range to POSIX.2 and PostgreSQL ("invalid character range") and
/// three ordinary members to the `regex` crate, while `[]-a]` and `[--/]` are
/// ranges to POSIX.2 and member lists to the `regex` crate. A `-` that opens or
/// closes the list, and an escaped `\-` anywhere, are read the same way by
/// both.
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
const INLINE_FLAG_GROUP: &str = "an inline flag group other than (?i:...) and (?-i:...) ((?m:...), (?s:...), (?x:...))";

/// How the rejection names a flag letter IEEE 1003.2 has no counterpart for.
const NON_POSIX_FLAG: &str = "an embedded option other than i, m, s and x";

/// How the rejection names a negated embedded option.
const NEGATED_EMBEDDED_OPTION: &str = "a negated embedded option other than (?-i) ((?-m), (?-s), (?-x))";

/// How the rejection names a repetition count above `RE_DUP_MAX`.
const REPETITION_COUNT: &str = "a repetition count above 255";

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

    /// Every one of these is outside IEEE 1003.2, and PostgreSQL 18 refuses
    /// each with "invalid escape \ sequence", "invalid embedded option",
    /// "invalid character class", "invalid character range" or "quantifier
    /// operand invalid": a client relying on one relies on the `regex` crate
    /// alone.
    #[test]
    fn a_construct_outside_posix_that_postgresql_cannot_run_is_refused() {
        each_is_refused(&[
            "\\p{L}",
            "\\P{L}",
            "[\\p{L}]",
            "a\\z",
            "\\x{61}",
            "(?P<name>ab)",
            "(?<name>ab)",
            "(?u)a",
            "(?U)a+",
            "(?R)a",
            "(?-u)a",
            "[[:^alpha:]]",
            "[a-b-c]",
            "[[:alpha:]-z]",
        ]);
    }

    /// Only the case-insensitive option may be scoped to a group or cleared;
    /// the other options may only be set for the rest of the pattern.
    #[test]
    fn a_scoped_or_cleared_option_other_than_case_insensitivity_is_refused() {
        each_is_refused(&[
            "(?m:^a)", "(?s:a.b)", "(?x:a b)", "(?im:a)", "(?-m)a", "(?-s)a.b", "(?-x)a b", "(?i-s)a", "(?-is)a",
        ]);
    }

    /// The case-insensitive option is admitted set, scoped and cleared, alone
    /// or beside the options that may be set.
    #[test]
    fn the_case_insensitive_option_is_admitted_set_scoped_and_cleared() {
        for pattern in ["(?i)čer", "(?i:č)er", "(?-i)a", "a(?i)b(?-i)c", "(?-i:a)", "(?im)^a", "(?is)a.b", "(?xi)a b"] {
            assert!(Pattern::parse(pattern).is_ok(), "{pattern} is admitted");
        }
    }

    /// IEEE 1003.2 guarantees repetition counts up to 255 and PostgreSQL
    /// refuses larger ones with "invalid repetition count(s)", which reached
    /// the client as `500 InternalError`.
    #[test]
    fn a_repetition_count_above_255_is_refused_and_255_is_admitted() {
        each_is_refused(&["a{256}", "a{256,}", "a{1,256}", "a{300,400}", "(ab){1000}"]);
        for pattern in ["a{255}", "a{0,255}", "a{255,}"] {
            assert!(Pattern::parse(pattern).is_ok(), "{pattern} is admitted");
        }
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
    /// while POSIX.2 and PostgreSQL read them as `a` and `-`. Nesting a plain
    /// bracket expression is the `regex` crate's own extension and diverges the
    /// same way: `[[abc]]` is `[abc]` to the `regex` crate and `[abc]` followed
    /// by a literal `]` to POSIX.2. A mistyped or unknown class name —
    /// `[[:alpha]]`, `[[:foo:]]` — takes the same parse and is refused outright
    /// by PostgreSQL.
    ///
    /// Refusing these narrows what clause 4.9 mandates. It is deliberate: the
    /// restatement the store runs follows the `regex` crate's reading, which is
    /// not the POSIX.2 one, and translating POSIX.2's reading instead would
    /// depend on the collating-element table of some collation.
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
    /// no POSIX.2 counterpart. POSIX.2 reads its operator characters as
    /// ordinary members: `[a-z&&b-d]` selects `b`, `c` and `d` to the `regex`
    /// crate and also `a`, `e`…`z` and `&` to POSIX.2.
    #[test]
    fn a_class_set_operation_is_refused() {
        each_is_refused(&["[a-z&&b-d]", "[[a-z]--[aeiou]]", "[[a-z]~~[a-c]]", "[+--]", "[a-b--d]"]);
    }

    /// POSIX.2 makes an unescaped `-` a range operator wherever a member
    /// precedes it and a member follows it; the `regex` crate reads it as a
    /// literal once a range or a character class precedes it. `[]-a]` is the
    /// range `]`…`a` to POSIX.2 and the three members `]`, `-`, `a` to the
    /// `regex` crate, and `[--/]` is the range `-`…`/` to POSIX.2 and two
    /// members to the `regex` crate.
    #[test]
    fn a_hyphen_the_two_engines_place_differently_is_refused() {
        each_is_refused(&["[]-a]", "[--/]", "[a-b-c]", "[[:alpha:]-z]"]);
    }

    /// PostgreSQL's `\xhhh` consumes every hexadecimal digit that follows it,
    /// so `Merc\x33de` is `Merc` followed by U+33DE to PostgreSQL; the `regex`
    /// crate reads exactly two digits, so the same pattern is `Merc3de` to it
    /// and selects `Merc3des`, which PostgreSQL's reading does not. A lone
    /// `\x61` happens to agree, but the escape has one width or the other and
    /// IEEE 1003.2 defines neither, so the form is refused.
    #[test]
    fn a_numeric_character_escape_the_two_engines_size_differently_is_refused() {
        each_is_refused(&["\\x61", "Merc\\x33de", "[\\x61]", "\\x{61}", "a[\\x41-\\x5A]"]);
    }

    /// The shorthand classes are admitted bare, bracketed and negated, and
    /// classify by Unicode: `\w` holds the letters of every European language,
    /// `\d` every decimal digit and `\s` every `White_Space` character. The
    /// store runs the same classes, written out as ranges; PostgreSQL's own
    /// shorthand over a `COLLATE "C"` operand would have matched none of the
    /// non-ASCII subjects here.
    #[test]
    fn a_shorthand_class_is_admitted_and_classifies_by_unicode() {
        for (raw, matching, rejecting) in [
            (
                "^\\w+$",
                &["čebela", "Škofja", "straße", "Ærø", "łódź", "Ελλάδα", "Ǆ", "e\u{301}", "a_1"][..],
                &["a-b", "a b", "a.b", ""][..],
            ),
            ("^\\d+$", &["0123456789", "\u{663}", "\u{967}", "\u{FF11}"], &["a", "\u{B2}", "\u{2167}", "½"]),
            (
                "^\\s$",
                &[" ", "\t", "\n", "\u{B}", "\u{C}", "\r", "\u{85}", "\u{A0}", "\u{2028}", "\u{3000}"],
                &["a", "\u{200B}", "\u{FEFF}", "_"],
            ),
            ("^\\W$", &["-", " ", "\u{B2}"], &["č", "_", "\u{663}"]),
            ("^\\D$", &["a", "\u{B2}"], &["7", "\u{663}"]),
            ("^\\S$", &["a", "\u{200B}"], &[" ", "\u{A0}"]),
            ("^[\\w:\\-]+$", &["urn:ngsi-ld:čebela-1", "Žalec-12"][..], &["a b"][..]),
            ("^[\\d.]+$", &["1.5", "\u{663}.\u{664}"], &["1,5"]),
            ("^[^\\s]+$", &["čebela"], &["č ebela"]),
        ] {
            let pattern = Pattern::parse(raw).unwrap_or_else(|error| panic!("{raw} is admitted: {error:?}"));
            for subject in matching {
                assert!(pattern.is_match(subject), "{raw} must match {subject:?}");
            }
            for subject in rejecting {
                assert!(!pattern.is_match(subject), "{raw} must not match {subject:?}");
            }
        }
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
