//! The unprefixed form of the clause 4.6.2 name grammar, which the part after
//! the colon of a prefixed name follows (`prefix %x3A unicodeLetter *nameChar`).

use crate::name::grammar::Name;

/// Whether `text` is a name without a prefix: a Unicode letter, then Unicode
/// letters, digits and `_`.
#[must_use]
pub fn is_unprefixed_name(text: &str) -> bool {
    !text.contains(':') && Name::new(text).is_ok()
}

#[cfg(test)]
mod tests {
    use crate::name::unprefixed::is_unprefixed_name;

    #[test]
    fn a_letter_then_letters_digits_and_underscores_is_an_unprefixed_name() {
        assert!(is_unprefixed_name("speed_limit42"));
        assert!(is_unprefixed_name("M\u{00fc}nchen"));
    }

    #[test]
    fn a_hyphen_a_leading_digit_or_an_empty_text_is_no_unprefixed_name() {
        assert!(!is_unprefixed_name("bad-name"));
        assert!(!is_unprefixed_name("42x"));
        assert!(!is_unprefixed_name(""));
    }

    #[test]
    fn a_prefixed_name_is_no_unprefixed_name() {
        assert!(!is_unprefixed_name("ex:name"));
    }
}
