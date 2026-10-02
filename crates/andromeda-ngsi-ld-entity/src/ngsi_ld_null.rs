//! Recognising the NGSI-LD Null (clause 4.5.0).
//!
//! The Null is the string `urn:ngsi-ld:null`; expansion under the core context
//! may also spell it as the IRI `https://uri.etsi.org/ngsi-ld/null`, so both
//! spellings name it.

use andromeda_ngsi_ld_vocabulary::{NULL_IRI, NULL_URI};

/// Whether `text` is one of the two spellings of the NGSI-LD Null.
#[must_use]
pub fn is_ngsi_ld_null(text: &str) -> bool {
    text == NULL_URI.as_str() || text == NULL_IRI.as_str()
}

#[cfg(test)]
mod tests {
    use crate::ngsi_ld_null::is_ngsi_ld_null;

    #[test]
    fn both_spellings_are_the_null_and_nothing_else_is() {
        assert!(is_ngsi_ld_null("urn:ngsi-ld:null"));
        assert!(is_ngsi_ld_null("https://uri.etsi.org/ngsi-ld/null"));
        assert!(!is_ngsi_ld_null("urn:ngsi-ld:nil"));
    }
}
