//! A matcher on the body of an answer that holds no prefixed spelling of the proof seals: `entityIdSealed` and
//! `entityTypeSealed` are bare members of an `ngsildproof` (clause 4.5.2.2), never a term a context expands.

use bytes::Bytes;
use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};

/// The prefixed or expanded spellings no body may hold.
const PREFIXED_SPELLINGS: [&str; 6] = [
    "ngsi-ld:entityIdSealed",
    "ngsi-ld:entityTypeSealed",
    "https://uri.etsi.org/ngsi-ld/entityIdSealed",
    "https://uri.etsi.org/ngsi-ld/entityTypeSealed",
    "default-context/entityIdSealed",
    "default-context/entityTypeSealed",
];

/// Matches a body that holds none of the prefixed spellings of the seals.
#[derive(Debug, Clone, Copy, MatcherBase)]
pub struct BareSeals;

/// A body holding no `ngsi-ld:` prefixed, expanded or default-context spelling of `entityIdSealed` or
/// `entityTypeSealed`.
#[must_use]
pub const fn bare_seals() -> BareSeals {
    BareSeals
}

impl BareSeals {
    /// The first prefixed spelling `body` holds, if any.
    fn found_in(body: &Bytes) -> Option<&'static str> {
        let text = String::from_utf8_lossy(body);
        PREFIXED_SPELLINGS.into_iter().find(|spelling| text.contains(spelling))
    }
}

impl Matcher<&Bytes> for BareSeals {
    fn matches(&self, actual: &Bytes) -> MatcherResult {
        BareSeals::found_in(actual).is_none().into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => "holds the seals bare".into(),
            MatcherResult::NoMatch => "holds a prefixed spelling of a seal".into(),
        }
    }

    fn explain_match(&self, actual: &Bytes) -> Description {
        BareSeals::found_in(actual).map_or_else(|| "which holds the seals bare".into(), |spelling| format!("which holds {spelling}").into())
    }
}

#[cfg(test)]
mod tests {
    use crate::bare_seals::bare_seals;
    use bytes::Bytes;
    use googletest::prelude::*;

    #[gtest]
    fn a_body_with_bare_seals_matches() -> Result<()> {
        let body = Bytes::from_static(br#"{"ngsildproof":{"type":"Property","entityIdSealed":"urn:ngsi-ld:A:1"}}"#);
        verify_that!(&body, bare_seals())
    }

    #[gtest]
    fn each_prefixed_spelling_is_refused() -> Result<()> {
        let body = Bytes::from_static(br#"{"https://uri.etsi.org/ngsi-ld/entityTypeSealed":"A"}"#);
        expect_that!(&body, not(bare_seals()));
        let prefixed = Bytes::from_static(br#"{"ngsi-ld:entityIdSealed":"urn:ngsi-ld:A:1"}"#);
        verify_that!(&prefixed, not(bare_seals()))
    }
}
