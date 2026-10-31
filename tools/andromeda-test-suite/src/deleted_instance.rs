//! A matcher on the instance a broker reports deleted (clause 4.5.0): its payload is the NGSI-LD Null of its kind and
//! it carries the `deletedAt` of the deletion, whichever kind of attribute it is.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use ngsi_ld::model::{Attribute, Output};

/// Matches an instance holding the NGSI-LD Null beside a `deletedAt`.
#[derive(Debug, Clone, Copy, MatcherBase)]
pub struct DeletedInstance;

/// An instance holding the NGSI-LD Null of its kind and a `deletedAt`.
#[must_use]
pub const fn deleted_instance() -> DeletedInstance {
    DeletedInstance
}

impl Matcher<&Attribute<Output>> for DeletedInstance {
    fn matches(&self, actual: &Attribute<Output>) -> MatcherResult {
        (actual.is_deleted() && actual.meta().output.deleted_at.is_some()).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => "is an instance holding the NGSI-LD Null and a deletedAt".into(),
            MatcherResult::NoMatch => "isn't an instance holding the NGSI-LD Null and a deletedAt".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::deleted_instance::deleted_instance;
    use googletest::prelude::*;
    use ngsi_ld::model::{Attribute, Lossless, Output};

    fn parsed(text: &str) -> Result<Attribute<Output>> {
        Ok(Attribute::<Output>::parse(text, Lossless::Normalized)?)
    }

    #[gtest]
    fn a_null_property_with_a_deleted_at_matches() -> Result<()> {
        verify_that!(
            &parsed(r#"{"type":"Property","value":"urn:ngsi-ld:null","deletedAt":"2020-01-01T00:00:00Z"}"#)?,
            deleted_instance()
        )
    }

    #[gtest]
    fn a_null_without_a_deleted_at_and_a_live_property_do_not_match() -> Result<()> {
        expect_that!(&parsed(r#"{"type":"Property","value":"urn:ngsi-ld:null"}"#)?, not(deleted_instance()));
        verify_that!(
            &parsed(r#"{"type":"Property","value":1,"deletedAt":"2020-01-01T00:00:00Z"}"#)?,
            not(deleted_instance())
        )
    }
}
