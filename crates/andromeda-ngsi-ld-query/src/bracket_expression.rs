//! IEEE 1003.2 bracket expressions restated in the form the `regex` crate reads the same way (clause 4.9 `RegExp`).
//!
//! A POSIX.2 bracket expression may hold a collating symbol `[.c.]` and an equivalence class `[=c=]`, and reads every other `[` inside it as an ordinary member. The `regex` crate parses neither term and reads a `[` inside a bracket expression as the start of a nested one, so it would read `M[[=e=]]rc` as a class over `=` and `e` followed by a literal `]`. Before a pattern is compiled, each term is therefore replaced by the character it names, escaped, and every other `[` inside a bracket expression by `\[`. A character class `[:name:]` keeps its spelling, which both read alike.
//!
//! A term is restated only when it names exactly one character. Every column a pattern is applied to carries `COLLATE "C"`, where a collating element is one character and an equivalence class holds only the character that names it, so `[[=e=]]` is `e` and never `[eéèê]`. A term naming more than one character (`[[.hyphen.]]`, `[[=ch=]]`) names a multi-character collating element of some locale's table and is refused.
//!
//! The scan follows the `regex` crate's tokenizer everywhere else, escapes and the comments and ignored whitespace of `(?x)` included, so that it and the parser reading the restatement agree on where every bracket expression opens and closes.

use crate::{error::QueryError, non_posix_construct::NonPosixConstruct};
use regex_syntax::{ast::ClassAsciiKind, is_escapeable_character};

/// Restates every bracket expression of `pattern` in the form the `regex` crate reads as IEEE 1003.2 does.
///
/// # Errors
///
/// Returns [`QueryError::PatternNonPosixConstruct`] for a collating symbol or equivalence class that does not name exactly one character, for a `[:` that does not open a character class, and for a range whose end is a character class.
pub fn restate_bracket_expressions(pattern: &str) -> Result<String, QueryError> {
    let mut scan = BracketScan {
        rest: pattern,
        restated: String::with_capacity(pattern.len()),
        whitespace: Whitespace::Significant,
        enclosing: Vec::new(),
    };
    scan.pattern()?;
    Ok(scan.restated)
}

/// How the `regex` crate reads whitespace and `#` at the scan's position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Whitespace {
    /// Both are ordinary characters.
    Significant,
    /// `(?x)` is in effect: whitespace is skipped and `#` opens a comment running to the end of its line.
    Ignored,
}

/// One pattern being restated.
struct BracketScan<'pattern> {
    /// The text not yet scanned.
    rest: &'pattern str,
    /// The restatement of the text already scanned.
    restated: String,
    /// How whitespace reads at the scan's position.
    whitespace: Whitespace,
    /// How whitespace read outside each open group, innermost last, restored when the group closes.
    enclosing: Vec<Whitespace>,
}

impl BracketScan<'_> {
    /// Scans the whole pattern outside any bracket expression.
    fn pattern(&mut self) -> Result<(), QueryError> {
        while let Some(next) = self.rest.chars().next() {
            match next {
                '\\' => self.escape(),
                '[' => {
                    self.copy(1);
                    self.bracket_expression()?;
                }
                '(' => self.group_open(),
                ')' => {
                    self.copy(1);
                    if let Some(enclosing) = self.enclosing.pop() {
                        self.whitespace = enclosing;
                    }
                }
                other => {
                    if !self.skip_ignored() {
                        self.copy(other.len_utf8());
                    }
                }
            }
        }
        Ok(())
    }

    /// Scans a bracket expression whose `[` is already copied, through its closing `]`.
    ///
    /// The opening follows the `regex` crate: an optional `^`, then any run of `-`, each an ordinary member, then, when no `-` came first, a `]` that is an ordinary member too.
    fn bracket_expression(&mut self) -> Result<(), QueryError> {
        self.skip_all_ignored();
        if self.rest.starts_with('^') {
            self.copy(1);
            self.skip_all_ignored();
        }
        let opens_with_hyphen = self.rest.starts_with('-');
        while self.rest.starts_with('-') {
            self.copy(1);
            self.skip_all_ignored();
        }
        if !opens_with_hyphen && self.rest.starts_with(']') {
            self.copy(1);
        }
        loop {
            self.skip_all_ignored();
            let Some(next) = self.rest.chars().next() else {
                return Ok(());
            };
            match next {
                ']' => {
                    self.copy(1);
                    return Ok(());
                }
                '\\' => self.escape(),
                '[' => self.bracketed_member()?,
                '-' => self.hyphen()?,
                other => self.copy(other.len_utf8()),
            }
        }
    }

    /// Restates a `[` inside a bracket expression.
    fn bracketed_member(&mut self) -> Result<(), QueryError> {
        let rest = self.rest;
        if let Some(after) = rest.strip_prefix("[:") {
            let length = character_class_length(after).ok_or(unsupported(NonPosixConstruct::UnknownCharacterClass))?;
            self.copy(2 + length);
            return Ok(());
        }
        if let Some(after) = rest.strip_prefix("[=") {
            return self.collating_element(after, "=]");
        }
        if let Some(after) = rest.strip_prefix("[.") {
            return self.collating_element(after, ".]");
        }
        self.restated.push_str("\\[");
        self.rest = &rest[1..];
        Ok(())
    }

    /// Restates an equivalence class or collating symbol whose opening is already stripped from `after`, as the one character it names.
    fn collating_element(&mut self, after: &str, close: &str) -> Result<(), QueryError> {
        let refused = || unsupported(NonPosixConstruct::CollatingElement);
        let end = after.find(close).ok_or_else(refused)?;
        let mut characters = after[..end].chars();
        let (Some(named), None) = (characters.next(), characters.next()) else {
            return Err(refused());
        };
        if is_escapeable_character(named) {
            self.restated.push('\\');
        } else if named.is_whitespace() && self.whitespace == Whitespace::Ignored {
            // `(?x)` skips whitespace inside a bracket expression too, and only ASCII whitespace can be escaped.
            return Err(refused());
        }
        self.restated.push(named);
        let consumed = self.rest.len() - after.len() + end + close.len();
        self.rest = &self.rest[consumed..];
        Ok(())
    }

    /// Copies a `-` inside a bracket expression, after its opening run.
    ///
    /// A range whose end is a character class (`[a-[:digit:]]`) is malformed in POSIX.2, and the `regex` crate reads the `[` as the range's end, so the bracket expression closes at another `]` than the one POSIX.2 closes it at.
    fn hyphen(&mut self) -> Result<(), QueryError> {
        if after_ignored(&self.rest[1..], self.whitespace).starts_with("[:") {
            return Err(unsupported(NonPosixConstruct::MedialHyphen));
        }
        self.copy(1);
        Ok(())
    }

    /// Copies an escape: the backslash, the character after it and, for an escape the `regex` crate braces (`\x{61}`, `\p{L}`, `\b{start}`), the braces.
    fn escape(&mut self) {
        let mut characters = self.rest.char_indices().skip(1);
        let Some((at, escaped)) = characters.next() else {
            self.copy(1);
            return;
        };
        let after = at + escaped.len_utf8();
        let braced = matches!(escaped, 'x' | 'u' | 'U' | 'p' | 'P' | 'b') && self.rest[after..].starts_with('{');
        let length = if braced {
            self.rest[after..].find('}').map_or(self.rest.len(), |close| after + close + 1)
        } else {
            after
        };
        self.copy(length);
    }

    /// Copies a `(` and whatever opens the group with it, tracking how the group reads whitespace.
    ///
    /// `(?flags)` sets options until the enclosing group closes and opens no group of its own; `(?flags:` opens a group with the options set inside it.
    fn group_open(&mut self) {
        self.copy(1);
        self.skip_all_ignored();
        let Some(after) = self.rest.strip_prefix('?') else {
            self.enclosing.push(self.whitespace);
            return;
        };
        let flags = after.find([')', ':']).filter(|&end| after[..end].chars().all(|flag| "imsUuxR-".contains(flag)));
        let Some(end) = flags else {
            self.enclosing.push(self.whitespace);
            return;
        };
        let whitespace = flag_whitespace(&after[..end], self.whitespace);
        if after[end..].starts_with(':') {
            self.enclosing.push(self.whitespace);
        }
        self.whitespace = whitespace;
        self.copy(1 + end + 1);
    }

    /// Copies one run of whitespace or one comment `(?x)` skips, if the scan is at one.
    fn skip_ignored(&mut self) -> bool {
        let skipped = self.rest.len() - after_one_ignored(self.rest, self.whitespace).len();
        self.copy(skipped);
        skipped != 0
    }

    /// Copies everything `(?x)` skips at the scan's position.
    fn skip_all_ignored(&mut self) {
        while self.skip_ignored() {}
    }

    /// Copies the next `length` bytes unchanged.
    fn copy(&mut self, length: usize) {
        let (copied, rest) = self.rest.split_at(length);
        self.restated.push_str(copied);
        self.rest = rest;
    }
}

/// How whitespace reads once `flags`, the letters of an embedded option, take effect over `current`.
fn flag_whitespace(flags: &str, current: Whitespace) -> Whitespace {
    let mut negated = false;
    let mut whitespace = current;
    for flag in flags.chars() {
        match flag {
            '-' => negated = true,
            'x' if negated => whitespace = Whitespace::Significant,
            'x' => whitespace = Whitespace::Ignored,
            _ => {}
        }
    }
    whitespace
}

/// The byte length of a character class whose `[:` is already stripped from `after`, through its `:]`, when the `regex` crate reads it as one.
///
/// The `regex` crate takes the name up to the first `:` and reads anything but a name it knows followed by `:]` as a nested bracket expression.
fn character_class_length(after: &str) -> Option<usize> {
    let name_start = usize::from(after.starts_with('^'));
    let name_end = name_start + after[name_start..].find(':')?;
    if !after[name_end..].starts_with(":]") {
        return None;
    }
    ClassAsciiKind::from_name(&after[name_start..name_end])?;
    Some(name_end + 2)
}

/// `text` past everything `(?x)` skips at its start.
fn after_ignored(text: &str, whitespace: Whitespace) -> &str {
    let mut rest = text;
    loop {
        let after = after_one_ignored(rest, whitespace);
        if after.len() == rest.len() {
            return rest;
        }
        rest = after;
    }
}

/// `text` past the whitespace character or the comment `(?x)` skips at its start, or all of `text` when there is none.
fn after_one_ignored(text: &str, whitespace: Whitespace) -> &str {
    match (whitespace, text.chars().next()) {
        (Whitespace::Ignored, Some('#')) => text.find('\n').map_or("", |end| &text[end + 1..]),
        (Whitespace::Ignored, Some(next)) if next.is_whitespace() => &text[next.len_utf8()..],
        (Whitespace::Ignored | Whitespace::Significant, _) => text,
    }
}

/// Name the construct in the rejection a client receives.
const fn unsupported(construct: NonPosixConstruct) -> QueryError {
    QueryError::PatternNonPosixConstruct { construct }
}

#[cfg(test)]
mod tests {
    use crate::{bracket_expression::restate_bracket_expressions, error::QueryError, non_posix_construct::NonPosixConstruct};
    use pretty_assertions::assert_eq;

    fn restated(pattern: &str) -> Option<String> {
        restate_bracket_expressions(pattern).ok()
    }

    fn refusal(pattern: &str) -> Option<NonPosixConstruct> {
        let Err(QueryError::PatternNonPosixConstruct { construct }) = restate_bracket_expressions(pattern) else {
            return None;
        };
        Some(construct)
    }

    #[test]
    fn an_equivalence_class_or_collating_symbol_becomes_the_character_it_names() {
        assert_eq!(restated("M[[=e=]]rc"), Some("M[e]rc".to_owned()));
        assert_eq!(restated("[[.e.]]des"), Some("[e]des".to_owned()));
        assert_eq!(restated("^[[=a=]x[:digit:]]$"), Some("^[ax[:digit:]]$".to_owned()));
        assert_eq!(restated("[[.a.]-[.c.]]"), Some("[a-c]".to_owned()));
    }

    #[test]
    fn a_named_character_the_regex_crate_reads_as_an_operator_is_escaped() {
        assert_eq!(restated("[[.-.]]"), Some("[\\-]".to_owned()));
        assert_eq!(restated("[[.].]]"), Some("[\\]]".to_owned()));
        assert_eq!(restated("[[=^=]]"), Some("[\\^]".to_owned()));
        assert_eq!(restated("[[...]]"), Some("[\\.]".to_owned()));
        assert_eq!(restated("[[= =]]"), Some("[\\ ]".to_owned()));
    }

    #[test]
    fn any_other_bracket_inside_a_bracket_expression_is_an_ordinary_member() {
        assert_eq!(restated("[[abc]]"), Some("[\\[abc]]".to_owned()));
        assert_eq!(restated("[x[a-c]]"), Some("[x\\[a-c]]".to_owned()));
        assert_eq!(restated("[a-[]"), Some("[a-\\[]".to_owned()));
    }

    #[test]
    fn text_outside_a_bracket_expression_and_character_classes_keep_their_spelling() {
        for pattern in [
            "^A.*",
            "\\[[=a=]]",
            "[[:alpha:]]",
            "[^[:^alpha:]]",
            "[]a]",
            "[^]a]",
            "[-[:alpha:]]",
            "[\\[]",
            "a\\p{L}",
            "(?i)čer",
            "[",
        ] {
            assert_eq!(restated(pattern), Some(pattern.to_owned()), "{pattern}");
        }
    }

    #[test]
    fn a_term_that_does_not_name_exactly_one_character_is_refused() {
        for pattern in ["[[.hyphen.]]", "[[=ch=]]", "[[==]]", "[[.a]", "[[=a]", "(?x)[[=\u{A0}=]]"] {
            assert_eq!(refusal(pattern), Some(NonPosixConstruct::CollatingElement), "{pattern}");
        }
    }

    #[test]
    fn a_character_class_with_a_malformed_or_unknown_name_is_refused() {
        for pattern in ["[[:alpha]]", "[[:foo:]]", "[[:alpha:"] {
            assert_eq!(refusal(pattern), Some(NonPosixConstruct::UnknownCharacterClass), "{pattern}");
        }
    }

    #[test]
    fn a_range_ending_in_a_character_class_is_refused() {
        for pattern in ["[a-[:digit:]]", "[]-[:alpha:]]", "(?x)[a- [:digit:]]"] {
            assert_eq!(refusal(pattern), Some(NonPosixConstruct::MedialHyphen), "{pattern}");
        }
    }

    /// Under `(?x)` the `regex` crate skips a comment, inside a bracket expression as outside one, so a `[` in it opens nothing.
    #[test]
    fn a_bracket_in_an_extended_mode_comment_is_left_alone() {
        assert_eq!(restated("(?x)a # [[=e=]\n[[=b=]]"), Some("(?x)a # [[=e=]\n[b]".to_owned()));
        assert_eq!(restated("(?x)[a # ]\n[=b=]]"), Some("(?x)[a # ]\nb]".to_owned()));
        assert_eq!(restated("a # [[=e=]]"), Some("a # [e]".to_owned()));
    }

    /// An option set inside a group lasts until the group closes.
    #[test]
    fn extended_mode_ends_with_the_group_that_set_it() {
        assert_eq!(restated("((?x)a)# [[=e=]]"), Some("((?x)a)# [e]".to_owned()));
        assert_eq!(restated("(?x:a)# [[=e=]]"), Some("(?x:a)# [e]".to_owned()));
        assert_eq!(restated("(?x)(?-x)# [[=e=]]"), Some("(?x)(?-x)# [e]".to_owned()));
    }
}
