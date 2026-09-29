//! A pattern restated so that PostgreSQL's regular-expression engine runs it
//! the way the `regex` crate does (clause 4.9 `RegExp`).
//!
//! A [`crate::pattern::Pattern`] is evaluated by two engines: the `regex` crate
//! in memory, and PostgreSQL's Advanced Regular Expression engine in the store.
//! Only the first ever reads the text the client wrote. The store reads this
//! restatement, rendered from the `regex` crate's own intermediate
//! representation of the pattern, in which every embedded option has already
//! been applied:
//!
//! * `(?i)`, `(?i:…)` and `(?-i)` leave no option behind. Each literal and each
//!   bracket expression the option governs becomes the explicit set of its
//!   Unicode simple case folds (`CaseFolding.txt` statuses C and S), taken from
//!   the tables the `regex` crate matches with, so `(?i)čer` is `[Čč][Ee][Rr]`
//!   and `(?i)k` is `[KkK]`, the last member being the Kelvin sign. PostgreSQL's
//!   own case-insensitive option folds only ASCII under the `COLLATE "C"` every
//!   stored value carries, and under a Unicode collation it pairs each character
//!   with its upper- and lower-case mapping rather than its fold set, which
//!   misses the Kelvin sign, `ſ`, `ẞ`, `ς` and the titlecase digraphs.
//! * `(?m)` turns `^` and `$` into line anchors, written here as the lookaround
//!   each one is. PostgreSQL's `m` option is a synonym for its `n` option, which
//!   also stops `.` and a negated bracket expression matching a newline; the
//!   `regex` crate's does not.
//! * `(?x)` has already dropped the whitespace and the comments it allows, and
//!   `.` has already been read with `dot_matches_new_line`.
//! * `\d`, `\w` and `\s` and their negations are the `regex` crate's Unicode
//!   classes -- `Nd`, the UTS #18 word characters and `White_Space` -- and are
//!   written as the ranges of those classes, some seven hundred of them for
//!   `\w`. PostgreSQL's own shorthand classifies by the operand's collation,
//!   which under `COLLATE "C"` is ASCII alone.
//!
//! Every character is written as itself. A numeric escape is never emitted,
//! because the two engines size them differently, and no class shorthand is
//! emitted either, because the two engines classify them differently: a class
//! is always its explicit ranges. PostgreSQL compares a bracket range's
//! endpoints by code point under every collation (established against
//! PostgreSQL 18.6 under `C`, a libc `en_US.utf8`, an ICU `sl-SI`, `pg_c_utf8`
//! and `pg_unicode_fast`), so the restatement means the same whatever collation
//! the operand it is applied to carries.

use crate::error::QueryError;
use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, Hir, HirKind, Look, Repetition};
use std::str;

/// A pattern restated in the constructs PostgreSQL's regular-expression engine
/// and the `regex` crate read alike, with no embedded option left in it.
///
/// This is what a store binds wherever it evaluates a pattern, and what it
/// persists beside a pattern it evaluates later.
#[derive(Debug, Clone, PartialEq, Eq, derive_more::Display)]
pub struct PortablePattern(String);

/// The revision of the rendering [`PortablePattern`] performs.
///
/// A store that persists a restatement stores this beside it and renders again
/// every restatement an older revision produced. It is bumped by hand whenever
/// the restatement of some admitted pattern would change: a change to this
/// module or to [`crate::posix_dialect`] that alters an output, or an upgrade of
/// `regex-syntax`, whose Unicode tables the case-fold sets and the shorthand
/// classes are taken from. A unit test
/// pins the current revision to the `regex-syntax` version `Cargo.lock` holds,
/// so an upgrade cannot land without a decision about the revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, derive_more::Display)]
pub struct RenderingRevision(i16);

impl RenderingRevision {
    /// The revision as the store records it.
    #[must_use]
    pub const fn get(self) -> i16 {
        self.0
    }
}

impl PortablePattern {
    /// The revision of the rendering this build performs.
    pub const RENDERING_REVISION: RenderingRevision = RenderingRevision(1);

    /// Render the intermediate representation the `regex` crate compiled.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError::PatternNonPosixConstruct`] for a node no portable
    /// construct expresses. [`crate::posix_dialect::check_posix_dialect`]
    /// refuses every pattern that could produce one before it is translated, so
    /// reaching the error means the two checks disagree.
    pub(crate) fn render(hir: &Hir) -> Result<PortablePattern, QueryError> {
        let mut text = String::new();
        write_node(hir, &mut text)?;
        Ok(PortablePattern(text))
    }

    /// The restatement, as PostgreSQL compiles it.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Append one node.
fn write_node(hir: &Hir, out: &mut String) -> Result<(), QueryError> {
    match hir.kind() {
        HirKind::Empty => Ok(()),
        HirKind::Literal(literal) => {
            let text = str::from_utf8(&literal.0).map_err(|_| unrenderable(NON_UTF8_LITERAL))?;
            text.chars().for_each(|character| push_outside_bracket(character, out));
            Ok(())
        }
        HirKind::Class(Class::Unicode(class)) => write_class(class, out),
        HirKind::Class(Class::Bytes(_)) => Err(unrenderable(BYTE_CLASS)),
        HirKind::Look(look) => write_look(*look, out),
        HirKind::Repetition(repetition) => write_repetition(repetition, out),
        // An alternation groups itself, so a capture around one adds nothing.
        HirKind::Capture(capture) if matches!(capture.sub.kind(), HirKind::Alternation(_)) => write_node(&capture.sub, out),
        HirKind::Capture(capture) => write_group(&capture.sub, out),
        HirKind::Concat(nodes) => nodes.iter().try_for_each(|node| write_node(node, out)),
        HirKind::Alternation(branches) => {
            // Always grouped: the translation drops a non-capturing group, so an
            // alternation written bare inside a concatenation would swallow its
            // neighbours as branches of its own.
            out.push_str("(?:");
            for (position, branch) in branches.iter().enumerate() {
                if position != 0 {
                    out.push('|');
                }
                write_node(branch, out)?;
            }
            out.push(')');
            Ok(())
        }
    }
}

/// Append a non-capturing group around one node.
///
/// Captures are written without capturing too: a pattern is only ever asked
/// whether it matches, never for what a group captured.
fn write_group(hir: &Hir, out: &mut String) -> Result<(), QueryError> {
    out.push_str("(?:");
    write_node(hir, out)?;
    out.push(')');
    Ok(())
}

/// Append a repetition, grouping its operand unless it is one atom.
///
/// PostgreSQL answers a quantifier applied directly to another quantifier with
/// "quantifier operand invalid", and a quantifier after a multi-character
/// literal would repeat only its last character.
fn write_repetition(repetition: &Repetition, out: &mut String) -> Result<(), QueryError> {
    if is_atom(&repetition.sub) {
        write_node(&repetition.sub, out)?;
    } else {
        write_group(&repetition.sub, out)?;
    }
    match (repetition.min, repetition.max) {
        (0, None) => out.push('*'),
        (1, None) => out.push('+'),
        (0, Some(1)) => out.push('?'),
        (min, None) => write_bound(out, min, UpperBound::Unbounded),
        (min, Some(max)) if min == max => write_bound(out, min, UpperBound::Exact),
        (min, Some(max)) => write_bound(out, min, UpperBound::At(max)),
    }
    if !repetition.greedy {
        out.push('?');
    }
    Ok(())
}

/// The upper end of a counted repetition.
#[derive(Debug, Clone, Copy)]
enum UpperBound {
    /// The count is exact: `{min}`.
    Exact,
    /// No upper end: `{min,}`.
    Unbounded,
    /// An upper end above the lower one: `{min,max}`.
    At(u32),
}

/// Append a counted bound.
fn write_bound(out: &mut String, min: u32, upper: UpperBound) {
    out.push('{');
    out.push_str(&min.to_string());
    match upper {
        UpperBound::Exact => {}
        UpperBound::Unbounded => out.push(','),
        UpperBound::At(max) => {
            out.push(',');
            out.push_str(&max.to_string());
        }
    }
    out.push('}');
}

/// Whether a node is written as one atom a quantifier may follow directly.
fn is_atom(hir: &Hir) -> bool {
    match hir.kind() {
        HirKind::Literal(literal) => str::from_utf8(&literal.0).is_ok_and(|text| text.chars().count() == 1),
        HirKind::Class(_) | HirKind::Alternation(_) | HirKind::Capture(_) => true,
        HirKind::Empty | HirKind::Look(_) | HirKind::Repetition(_) | HirKind::Concat(_) => false,
    }
}

/// Append an assertion.
///
/// Without an embedded option PostgreSQL anchors `^` and `$` to the whole
/// subject, which is what [`Look::Start`] and [`Look::End`] mean. The line
/// anchors `(?m)` asks for are written as the lookaround each one is, since
/// PostgreSQL's own `m` option changes what `.` matches as well.
fn write_look(look: Look, out: &mut String) -> Result<(), QueryError> {
    match look {
        Look::Start => out.push('^'),
        Look::End => out.push('$'),
        Look::StartLF => out.push_str("(?:^|(?<=\n))"),
        Look::EndLF => out.push_str("(?:$|(?=\n))"),
        Look::StartCRLF
        | Look::EndCRLF
        | Look::WordAscii
        | Look::WordAsciiNegate
        | Look::WordUnicode
        | Look::WordUnicodeNegate
        | Look::WordStartAscii
        | Look::WordEndAscii
        | Look::WordStartUnicode
        | Look::WordEndUnicode
        | Look::WordStartHalfAscii
        | Look::WordEndHalfAscii
        | Look::WordStartHalfUnicode
        | Look::WordEndHalfUnicode => return Err(unrenderable(UNRENDERABLE_ASSERTION)),
    }
    Ok(())
}

/// Append a class as a bracket expression over its ranges.
///
/// PostgreSQL text cannot carry U+0000, so a class holding it -- the
/// translation of a negated bracket expression, or of `.` -- is written as the
/// negation of its complement, and a class holding every character as `.`,
/// which PostgreSQL's default newline-insensitive mode lets match anything.
fn write_class(class: &ClassUnicode, out: &mut String) -> Result<(), QueryError> {
    let Some(first) = class.ranges().first() else {
        return Err(unrenderable(EMPTY_CLASS));
    };
    if first.start() != '\0' {
        out.push('[');
        write_ranges(class.ranges(), out);
        out.push(']');
        return Ok(());
    }
    // clone: negation works in place, and the class belongs to the translation.
    let mut complement = class.clone();
    complement.negate();
    if complement.ranges().is_empty() {
        out.push('.');
    } else {
        out.push_str("[^");
        write_ranges(complement.ranges(), out);
        out.push(']');
    }
    Ok(())
}

/// Append the members of a bracket expression: a range of three or more
/// characters as a range, a shorter one as its characters.
fn write_ranges(ranges: &[ClassUnicodeRange], out: &mut String) {
    for range in ranges {
        let (start, end) = (range.start(), range.end());
        push_inside_bracket(start, out);
        match u32::from(end) - u32::from(start) {
            0 => {}
            1 => push_inside_bracket(end, out),
            _ => {
                out.push('-');
                push_inside_bracket(end, out);
            }
        }
    }
}

/// Append a character outside a bracket expression, escaping the characters
/// PostgreSQL's engine gives a meaning there.
///
/// A backslash before a character that is not alphanumeric makes it literal in
/// both engines.
fn push_outside_bracket(character: char, out: &mut String) {
    if matches!(character, '.' | '[' | ']' | '{' | '}' | '(' | ')' | '*' | '+' | '?' | '^' | '$' | '|' | '\\') {
        out.push('\\');
    }
    out.push(character);
}

/// Append a character inside a bracket expression, escaping the characters
/// that would close it, negate it, form a range or open a POSIX class.
fn push_inside_bracket(character: char, out: &mut String) {
    if matches!(character, ']' | '\\' | '^' | '-' | '[') {
        out.push('\\');
    }
    out.push(character);
}

/// Name the node in the rejection a client receives.
const fn unrenderable(construct: &'static str) -> QueryError {
    QueryError::PatternNonPosixConstruct { construct }
}

/// How the rejection names a literal that is not UTF-8.
const NON_UTF8_LITERAL: &str = "a literal that is not UTF-8";

/// How the rejection names a class over bytes.
const BYTE_CLASS: &str = "a class over bytes rather than characters";

/// How the rejection names an assertion with no portable spelling.
const UNRENDERABLE_ASSERTION: &str = "a word-boundary or CRLF-aware assertion";

/// How the rejection names a class that admits no character.
const EMPTY_CLASS: &str = "a class that matches no character";

#[cfg(test)]
mod tests {
    use crate::{
        pattern::Pattern,
        portable_pattern::{PortablePattern, RenderingRevision},
    };
    use pretty_assertions::assert_eq;
    use regex_syntax::hir::{Class, ClassUnicode, ClassUnicodeRange, Hir};

    /// The restatement a pattern binds into the store.
    fn portable(raw: &str) -> String {
        Pattern::parse(raw)
            .unwrap_or_else(|error| panic!("{raw} is admitted: {error:?}"))
            .portable()
            .as_str()
            .to_owned()
    }

    /// A pattern with no embedded option and no class is its own restatement,
    /// so the anchored identifier prefix PostgreSQL turns into a B-tree range
    /// is still there to be found.
    #[test]
    fn a_pattern_without_options_or_classes_is_restated_as_written() {
        for raw in ["^urn:ngsi-ld:Vehicle:.*", "Merc", "^A", "a$", "urn:ngsi-ld:Vehicle:A4567"] {
            assert_eq!(portable(raw), raw);
        }
    }

    /// Clause 4.9 values in European languages fold outside ASCII: each literal
    /// under `(?i)` becomes the set of its simple case folds.
    #[test]
    fn a_case_insensitive_literal_becomes_the_set_of_its_case_folds() {
        assert_eq!(portable("(?i)čer"), "[Čč][Ee][Rr]");
        assert_eq!(portable("(?i)^ŽALEC$"), "^[Žž][Aa][Ll][Ee][Cc]$");
        assert_eq!(portable("(?i)ő"), "[Őő]");
        assert_eq!(portable("(?i)ł"), "[Łł]");
        assert_eq!(portable("(?i)ĳ"), "[Ĳĳ]");
    }

    /// Simple case folding is an equivalence over more than two characters for
    /// some letters, and the restatement carries every member of it.
    #[test]
    fn a_case_fold_set_with_more_than_two_members_is_written_whole() {
        assert_eq!(portable("(?i)k"), "[Kk\u{212A}]");
        assert_eq!(portable("(?i)s"), "[Ss\u{17F}]");
        assert_eq!(portable("(?i)σ"), "[Σςσ]");
        assert_eq!(portable("(?i)ǆ"), "[Ǆ-ǆ]");
    }

    /// Simple case folding maps one character to one character, so `ß` folds
    /// to `ẞ` and never to `SS`, and the Turkish dotted and dotless `i` fold to
    /// nothing but themselves: the `regex` crate reads them so, and so must the
    /// store.
    #[test]
    fn a_character_without_a_simple_fold_partner_keeps_only_its_own_forms() {
        assert_eq!(portable("(?i)ß"), "[ßẞ]");
        assert_eq!(portable("(?i)i"), "[Ii]");
        assert_eq!(portable("(?i)İ"), "İ");
        assert_eq!(portable("(?i)1"), "1");
    }

    /// A scoped group and a negation govern exactly the part of the pattern
    /// the `regex` crate applies them to.
    #[test]
    fn a_scoped_or_negated_case_insensitive_option_governs_only_its_span() {
        assert_eq!(portable("(?i:č)er"), "[Čč]er");
        assert_eq!(portable("a(?i)b(?-i)c"), "a[Bb]c");
        assert_eq!(portable("(?i)a(?-i:b)c"), "[Aa]b[Cc]");
        assert_eq!(portable("(?i)(ab|cd)(?-i)e"), "(?:[Aa][Bb]|[Cc][Dd])e");
    }

    /// A bracket expression under `(?i)` is folded before it is negated, as
    /// the `regex` crate folds it: `[^č]` excludes both `č` and `Č`.
    #[test]
    fn a_case_insensitive_bracket_expression_is_folded_then_negated() {
        assert_eq!(portable("(?i)[^č]"), "[^Čč]");
        assert_eq!(portable("(?i)^[^čš]+$"), "^[^ČčŠš]+$");
        assert_eq!(portable("(?i)[č-ž]"), "[ÿČ-ž]");
    }

    /// A POSIX class is ASCII in the `regex` crate, and folding it reaches the
    /// non-ASCII characters whose fold set holds an ASCII letter.
    #[test]
    fn a_case_insensitive_posix_class_reaches_the_non_ascii_members_of_its_fold_sets() {
        assert_eq!(portable("(?i)[[:alpha:]]"), "[A-Za-z\u{17F}\u{212A}]");
        assert_eq!(portable("[[:alpha:]]"), "[A-Za-z]");
    }

    /// `(?m)` line anchors are written as lookarounds rather than as
    /// PostgreSQL's `m` option, which would also stop `.` matching a newline.
    #[test]
    fn a_multi_line_anchor_is_written_as_its_lookaround() {
        assert_eq!(portable("(?m)^a$"), "(?:^|(?<=\n))a(?:$|(?=\n))");
        assert_eq!(portable("(?m)a.b"), "a.b");
    }

    /// `(?x)` whitespace is gone from the restatement, so PostgreSQL never has
    /// to agree with the `regex` crate on what extended mode ignores.
    #[test]
    fn extended_mode_leaves_no_whitespace_behind() {
        assert_eq!(portable("(?x) a b # comment"), "ab");
    }

    /// Characters PostgreSQL gives a meaning are escaped outside and inside a
    /// bracket expression.
    #[test]
    fn a_metacharacter_is_escaped_where_postgresql_would_read_it() {
        assert_eq!(portable(r"a\.b\{1\}"), r"a\.b\{1\}");
        assert_eq!(portable(r"[\]x]"), r"[\]x]");
        assert_eq!(portable(r"[\-x]"), r"[\-x]");
        assert_eq!(portable(r"[x\^]"), r"[\^x]");
    }

    /// A quantifier after a multi-character literal or after another quantified
    /// node repeats the whole of it, which needs a group in PostgreSQL.
    #[test]
    fn a_quantified_node_that_is_not_one_atom_is_grouped() {
        assert_eq!(portable("(?:ab)+"), "(?:ab)+");
        assert_eq!(portable("(a*)*"), "(?:a*)*");
        assert_eq!(portable("a{2,3}"), "a{2,3}");
        assert_eq!(portable("a{2}"), "a{2}");
        assert_eq!(portable("a{2,}"), "a{2,}");
        assert_eq!(portable("a+?"), "a+?");
    }

    /// An alternation is grouped wherever it stands, since the translation
    /// drops the non-capturing group that bounded it.
    #[test]
    fn an_alternation_is_always_grouped() {
        assert_eq!(portable("x(?:ab|cd)y"), "x(?:ab|cd)y");
        assert_eq!(portable("ab|cd"), "(?:ab|cd)");
    }

    /// `.` holds every character, U+0000 included, which PostgreSQL text cannot
    /// carry; it is written as `.`, which PostgreSQL's default mode lets match a
    /// newline too.
    #[test]
    fn a_class_holding_every_character_is_written_as_a_dot() {
        assert_eq!(portable("a.b"), "a.b");
        assert_eq!(portable(r"[\w\W]"), ".");
        assert_eq!(portable(r"[\s\S]"), ".");
    }

    /// `\s` is `White_Space`, written out whole; its negation holds U+0000 and
    /// is written as the negation of `\s`.
    #[test]
    fn a_white_space_shorthand_is_written_as_every_white_space_character() {
        let white_space = "\t-\r \u{85}\u{A0}\u{1680}\u{2000}-\u{200A}\u{2028}\u{2029}\u{202F}\u{205F}\u{3000}";
        assert_eq!(portable(r"\s"), format!("[{white_space}]"));
        assert_eq!(portable(r"\S"), format!("[^{white_space}]"));
        assert_eq!(portable(r"[^\s]"), format!("[^{white_space}]"));
    }

    /// `\d` is every decimal digit and `\w` every word character Unicode
    /// defines, not the ASCII ones PostgreSQL's own shorthand is under
    /// `COLLATE "C"`: the restatement opens with the ASCII members and goes on
    /// to the Arabic-Indic digits and the Latin-1 letters.
    #[test]
    fn a_digit_or_word_shorthand_is_written_as_its_unicode_ranges() {
        assert!(portable(r"\d").starts_with("[0-9\u{660}-\u{669}\u{6F0}-\u{6F9}"), "{}", portable(r"\d"));
        assert!(portable(r"\D").starts_with("[^0-9\u{660}-\u{669}"), "{}", portable(r"\D"));
        assert!(portable(r"\w").starts_with("[0-9A-Z_a-zªµºÀ-ÖØ-ö"), "{}", portable(r"\w"));
        assert!(portable(r"\W").starts_with("[^0-9A-Z_a-zªµºÀ-ÖØ-ö"), "{}", portable(r"\W"));
    }

    /// Every word character's simple case folds are word characters too, so
    /// `(?i)` leaves `\w` as it is.
    #[test]
    fn a_case_insensitive_word_shorthand_is_the_word_shorthand() {
        assert_eq!(portable(r"(?i)\w"), portable(r"\w"));
    }

    /// The workspace lock file, which fixes the `regex-syntax` the fold sets
    /// are rendered from.
    const CARGO_LOCK: &str = include_str!("../../../Cargo.lock");

    /// The version `Cargo.lock` holds for `package`.
    fn locked_version<'lock>(lock: &'lock str, package: &str) -> Option<&'lock str> {
        let name = format!("name = \"{package}\"");
        let mut lines = lock.lines();
        lines.find(|line| *line == name)?;
        lines.next()?.strip_prefix("version = \"")?.strip_suffix('"')
    }

    /// Revision 1 was rendered from `regex-syntax` 0.8.11 (Unicode 16.0 case
    /// folding and properties). Moving the lock to another version fails here
    /// until the revision is bumped with it, so stored restatements are rendered
    /// again.
    #[test]
    fn the_rendering_revision_is_pinned_to_the_locked_regex_syntax() {
        assert_eq!(
            (PortablePattern::RENDERING_REVISION, locked_version(CARGO_LOCK, "regex-syntax")),
            (RenderingRevision(1), Some("0.8.11")),
            "a regex-syntax upgrade can change the case-fold sets and the shorthand classes: bump RENDERING_REVISION and this pin together"
        );
    }

    /// A class that admits nothing has no bracket spelling; it is refused
    /// rather than rendered as something that matches.
    #[test]
    fn a_class_that_admits_nothing_is_refused() {
        let empty = Hir::class(Class::Unicode(ClassUnicode::new(Vec::<ClassUnicodeRange>::new())));
        assert!(PortablePattern::render(&empty).is_err());
    }
}
