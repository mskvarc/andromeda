//! The alphanumeric common code token itself.

use derive_more::Display;
use thiserror::Error;

/// Shortest common code Recommendation 20 defines.
const MINIMUM_LENGTH: usize = 2;

/// Longest common code Recommendation 20 defines.
const MAXIMUM_LENGTH: usize = 3;

/// A UN/CEFACT common code as it is written on the wire, for example `KGM`.
///
/// Recommendation 20 restricts a common code to two or three upper-case
/// alphanumeric characters, which is the same shape the catalog table's check
/// constraint enforces. The token is the only representation a withdrawn code
/// still has: once a revision removes a code the generating artifact stops
/// offering a typed variant for it, while the identifier already written into
/// stored rows must stay readable, so the registry keeps the token itself.
///
/// The borrow is `'static` because every token in the process originates either
/// in the committed registry snapshot or in the generated code list, both of
/// which are compiled in. That keeps decoding a stored identifier allocation-
/// free, which matters on a read path that decodes one token per attribute.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[display("{_0}")]
pub struct UnitCodeToken(&'static str);

impl UnitCodeToken {
    /// Return the token as written on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl TryFrom<&'static str> for UnitCodeToken {
    type Error = UnitCodeTokenMalformed;

    fn try_from(token: &'static str) -> Result<UnitCodeToken, UnitCodeTokenMalformed> {
        let length = token.len();
        let well_formed = (MINIMUM_LENGTH..=MAXIMUM_LENGTH).contains(&length) && token.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit());
        if well_formed { Ok(UnitCodeToken(token)) } else { Err(UnitCodeTokenMalformed) }
    }
}

/// A token was not two or three upper-case alphanumeric characters.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
#[error("a UN/CEFACT common code is two or three upper-case alphanumeric characters")]
pub struct UnitCodeTokenMalformed;

#[cfg(test)]
mod tests {
    use crate::unit_code_token::{UnitCodeToken, UnitCodeTokenMalformed};

    #[test]
    fn a_three_character_code_is_accepted() {
        assert_eq!(UnitCodeToken::try_from("KGM").map(UnitCodeToken::as_str), Ok("KGM"));
    }

    #[test]
    fn a_two_character_code_is_accepted() {
        assert_eq!(UnitCodeToken::try_from("CEL").map(UnitCodeToken::as_str), Ok("CEL"));
        assert_eq!(UnitCodeToken::try_from("A9").map(UnitCodeToken::as_str), Ok("A9"));
    }

    #[test]
    fn a_code_of_the_wrong_length_is_refused() {
        assert_eq!(UnitCodeToken::try_from("K"), Err(UnitCodeTokenMalformed));
        assert_eq!(UnitCodeToken::try_from("KGMX"), Err(UnitCodeTokenMalformed));
        assert_eq!(UnitCodeToken::try_from(""), Err(UnitCodeTokenMalformed));
    }

    #[test]
    fn a_lower_case_or_punctuated_code_is_refused() {
        assert_eq!(UnitCodeToken::try_from("kgm"), Err(UnitCodeTokenMalformed));
        assert_eq!(UnitCodeToken::try_from("K-M"), Err(UnitCodeTokenMalformed));
    }
}
