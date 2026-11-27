//! Where a name of a path stands, which decides how far the name runs written without quotes (clause 4.9).
//!
//! Wherever an unquoted colon after a name has no meaning of its own, an unquoted name is a letter followed by letters, digits, `_`, `-`, `:`, `/` and `#`, so a short name, a prefixed name (clause 4.6.2) and an absolute IRI holding no dot are all written as they are. At the head of a hop's braces and in its type list an unquoted colon ends the list, so an unquoted name there is exactly the clause 4.9 `AttrName` or `EntityType`, a letter followed by letters, digits and `_`. The text of `X:y` alone never tells a type list from a name, since RFC 3986 section 3.1 makes `X` the scheme of an absolute IRI, so the position decides. A name outside its position's set is written as the grammar's `quotedStr`, which every position reads.

use andromeda_ngsi_ld_lexicon::name::unprefixed::is_unprefixed_name;
use std::str;

/// Where a name of a path stands.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NamePosition {
    /// A position no unquoted colon after the name means anything at: the attribute of a Query Term, a sub-attribute or member after it, the Relationship of a hop before its brace, the attribute after a hop's type list, and every name of an `orderBy` term or an `aggrGroupBy` key.
    Path,
    /// The first name inside a hop's braces and every type of its list, where an unquoted colon after the name ends the type list.
    HopHead,
}

impl NamePosition {
    /// The byte length of the unquoted name that opens at the start of `text` at this position, zero when none opens there.
    #[must_use]
    pub(super) fn unquoted_extent(self, text: &str) -> usize {
        match self {
            NamePosition::Path => extent(text, |character| continues_attr_name(character) || matches!(character, '-' | ':' | '/' | '#')),
            NamePosition::HopHead => attr_name_extent(text),
        }
    }

    /// Whether `text`, written without quotes at this position, reads back as the one name it is.
    #[must_use]
    pub(super) fn reads_bare(self, text: &str) -> bool {
        !text.is_empty() && self.unquoted_extent(text) == text.len()
    }
}

/// The byte length of the clause 4.9 `AttrName` that opens at the start of `text`, zero when none opens there.
#[must_use]
pub(super) fn attr_name_extent(text: &str) -> usize {
    extent(text, continues_attr_name)
}

/// The byte length of the run that opens with a letter at the start of `text` and continues through every character `continues` admits.
fn extent(text: &str, continues: impl Fn(char) -> bool) -> usize {
    let mut characters = text.char_indices();
    match characters.next() {
        Some((_, first)) if opens(first) => characters.find(|(_, character)| !continues(*character)).map_or(text.len(), |(index, _)| index),
        Some(_) | None => 0,
    }
}

/// Whether `character` opens an unquoted name, as a Unicode letter does.
fn opens(character: char) -> bool {
    let mut buffer = [0; 4];
    is_unprefixed_name(character.encode_utf8(&mut buffer))
}

/// Whether `character` continues an `AttrName`, as a Unicode letter, a digit and `_` do.
fn continues_attr_name(character: char) -> bool {
    let mut buffer = [b'a'; 5];
    let length = character.encode_utf8(&mut buffer[1..]).len();
    str::from_utf8(&buffer[..=length]).is_ok_and(is_unprefixed_name)
}

#[cfg(test)]
mod tests {
    use crate::filter::name_position::{NamePosition, attr_name_extent};

    #[test]
    fn an_attr_name_is_a_letter_followed_by_letters_digits_and_underscores() {
        for (text, extent) in [
            ("speed>50", 5),
            ("x_1.a", 3),
            ("M\u{fc}nchen==1", 8),
            ("a-b", 1),
            ("a/b", 1),
            ("a#b", 1),
            ("a:b", 1),
            ("true==1", 4),
        ] {
            pretty_assertions::assert_eq!(attr_name_extent(text), extent, "{text}");
            pretty_assertions::assert_eq!(NamePosition::HopHead.unquoted_extent(text), extent, "{text}");
        }
    }

    #[test]
    fn a_name_of_a_path_runs_through_hyphens_colons_slashes_and_hashes() {
        for (text, extent) in [
            ("speed>50", 5),
            ("ex:speed>50", 8),
            ("urn:xq:weight>1000", 13),
            ("ex:speed.observedAt", 8),
            ("zip]", 3),
            ("ex:sensor{Device:humidity}", 9),
            ("urn:x:Vehicle/a#speed==1", 21),
            ("https://example.org/a", 15),
            ("urn:x:a.b", 7),
            ("speed-limit>5", 11),
            ("a;b", 1),
            ("a,b", 1),
            ("a|b", 1),
            ("a(b)", 1),
            ("a~=b", 1),
            ("a!=b", 1),
            ("a b", 1),
            ("a\"b", 1),
            ("ex:speed;desc", 8),
        ] {
            pretty_assertions::assert_eq!(NamePosition::Path.unquoted_extent(text), extent, "{text}");
        }
    }

    #[test]
    fn no_name_opens_at_a_character_other_than_a_letter() {
        for text in ["_a", ":a", "/a", "#a", "1a", "-a", "\"a\"", ""] {
            pretty_assertions::assert_eq!(NamePosition::Path.unquoted_extent(text), 0, "{text}");
            pretty_assertions::assert_eq!(NamePosition::HopHead.unquoted_extent(text), 0, "{text}");
        }
    }

    #[test]
    fn a_name_reads_bare_exactly_when_its_whole_text_is_the_unquoted_name_of_its_position() {
        for (text, path, hop_head) in [
            ("speed", true, true),
            ("M\u{fc}nchen", true, true),
            ("ex:speed", true, false),
            ("urn:xq:weight", true, false),
            ("urn:x:Vehicle/a#speed", true, false),
            ("ngsi-ld:speed_1", true, false),
            ("https://example.org/a", false, false),
            ("urn:x:a;b", false, false),
            ("urn:x:a=b", false, false),
            ("urn:x:a(b)", false, false),
            ("urn:x:a,b", false, false),
            ("http://[::1]/a", false, false),
            ("1x:y", false, false),
            ("", false, false),
        ] {
            pretty_assertions::assert_eq!(NamePosition::Path.reads_bare(text), path, "{text} at a path");
            pretty_assertions::assert_eq!(NamePosition::HopHead.reads_bare(text), hop_head, "{text} at the head of a hop");
        }
    }
}
