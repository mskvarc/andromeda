//! The character class of an HTTP `token` (RFC 9110 section 5.6.2, the grammar
//! of RFC 7230 section 3.2.6): the characters a field value carries outside a
//! quoted string and a comment.

/// Whether `character` is a `tchar`: an ASCII letter or digit, or one of
/// ``!#$%&'*+-.^_`|~``.
#[must_use]
pub const fn is_tchar(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(
            character,
            '!' | '#' | '$' | '%' | '&' | '\'' | '*' | '+' | '-' | '.' | '^' | '_' | '`' | '|' | '~'
        )
}

#[cfg(test)]
mod tests {
    use crate::http_token::is_tchar;

    #[test]
    fn letters_digits_and_the_fifteen_symbols_are_token_characters() {
        for character in "azAZ09!#$%&'*+-.^_`|~".chars() {
            assert!(is_tchar(character), "{character:?} is a tchar");
        }
    }

    /// RFC 9110 section 5.6.2: a delimiter, whitespace, a control character or
    /// anything beyond ASCII ends a token.
    #[test]
    fn delimiters_whitespace_controls_and_non_ascii_are_not() {
        for character in "\"(),/:;<=>?@[\\]{} \t\u{7}\u{7f}é".chars() {
            assert!(!is_tchar(character), "{character:?} is no tchar");
        }
    }
}
