//! The `proofValue` of a W3C Data Integrity proof (ETSI TS 104 179 V1.1.1
//! clause 5.1.2): the encoded signature.
//!
//! The value is held as written. A verifier decodes it by its multibase
//! header; the broker neither signs nor verifies, and the annex C.11 example
//! of ETSI GS CIM 009 v1.9.1 abridges its signature with `....`, which no
//! base decodes, so nothing beyond a non-empty string is required of it.

use andromeda_ngsi_ld_lexicon::non_empty_text::{NonEmptyText, NonEmptyTextError};
use derive_more::{AsRef, Display};
use std::str::FromStr;

/// An encoded signature, a non-empty string.
#[derive(AsRef, Clone, Debug, Display, Eq, Hash, PartialEq)]
#[as_ref(str)]
pub struct ProofValue(NonEmptyText);

impl ProofValue {
    /// The signature as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

// Validating parser: the newtype forwards the non-empty check of its field,
// which `derive_more::FromStr` does not do for a field of another newtype.
impl FromStr for ProofValue {
    type Err = NonEmptyTextError;

    fn from_str(text: &str) -> Result<ProofValue, NonEmptyTextError> {
        NonEmptyText::new(text).map(ProofValue)
    }
}

#[cfg(test)]
mod tests {
    use crate::proof::proof_value::ProofValue;
    use andromeda_ngsi_ld_lexicon::non_empty_text::NonEmptyTextError;

    /// Annex C.11 abridges its signature with `....`, which is kept as written.
    #[test]
    fn the_abridged_annex_c_11_signature_reads_back_as_written() {
        let written = "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS";
        pretty_assertions::assert_eq!(written.parse::<ProofValue>().map(|value| value.to_string()), Ok(written.to_owned()));
    }

    #[test]
    fn an_empty_signature_is_refused() {
        pretty_assertions::assert_eq!("".parse::<ProofValue>(), Err(NonEmptyTextError::Empty));
    }
}
