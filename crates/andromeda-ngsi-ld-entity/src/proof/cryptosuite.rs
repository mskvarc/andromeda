//! The `cryptosuite` of a W3C Data Integrity proof (ETSI TS 104 179 V1.1.1
//! clause 5.1.2): the suite the proof was made with, such as
//! `eddsa-rdfc-2022`, which tells a verifier the canonicalization, digest and
//! signature algorithms to check it by.

use andromeda_ngsi_ld_lexicon::non_empty_text::{NonEmptyText, NonEmptyTextError};
use derive_more::{AsRef, Display};
use std::str::FromStr;

/// The identifier of a cryptographic suite, a non-empty string.
#[derive(AsRef, Clone, Debug, Display, Eq, Hash, PartialEq)]
#[as_ref(str)]
pub struct Cryptosuite(NonEmptyText);

impl Cryptosuite {
    /// The identifier as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Validating parser: the newtype forwards the non-empty check of its field,
// which `derive_more::FromStr` does not do for a field of another newtype.
impl FromStr for Cryptosuite {
    type Err = NonEmptyTextError;

    fn from_str(text: &str) -> Result<Cryptosuite, NonEmptyTextError> {
        NonEmptyText::new(text).map(Cryptosuite)
    }
}

#[cfg(test)]
mod tests {
    use crate::proof::cryptosuite::Cryptosuite;
    use andromeda_ngsi_ld_lexicon::non_empty_text::NonEmptyTextError;

    #[test]
    fn a_suite_identifier_reads_back_as_written() {
        pretty_assertions::assert_eq!(
            "eddsa-rdfc-2022".parse::<Cryptosuite>().map(|suite| suite.to_string()),
            Ok("eddsa-rdfc-2022".to_owned())
        );
    }

    #[test]
    fn an_empty_identifier_is_refused() {
        pretty_assertions::assert_eq!("".parse::<Cryptosuite>(), Err(NonEmptyTextError::Empty));
    }
}
