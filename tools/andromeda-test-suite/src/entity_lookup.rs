//! The resource of an answer with a given identifier, for a test whose answer does not fix the order of a page.

use crate::identity_matcher::Identified;

/// The item of `items` whose id is `id`; a projected entity that has no id is never found.
#[must_use]
pub fn with_id<'a, R: Identified>(items: &'a [R], id: &R::Id) -> Option<&'a R> {
    items.iter().find(|item| item.identifier() == Some(id))
}

#[cfg(test)]
mod tests {
    use crate::entity_lookup::with_id;
    use googletest::prelude::*;
    use ngsi_ld::model::{Entity, EntityId, Lossless, Output, Projected};

    #[gtest]
    fn the_item_with_the_id_is_found_and_an_absent_one_is_not() -> Result<()> {
        let first: EntityId = "urn:ngsi-ld:Building:1".parse()?;
        let second: EntityId = "urn:ngsi-ld:Building:2".parse()?;
        let items = [
            Entity::<Output, Projected>::parse(r#"{"id":"urn:ngsi-ld:Building:1"}"#, Lossless::Normalized)?,
            Entity::<Output, Projected>::parse(r#"{"id":"urn:ngsi-ld:Building:2"}"#, Lossless::Normalized)?,
        ];
        expect_that!(with_id(&items, &second).and_then(|item| item.id.as_ref()), some(eq(&second)));
        let missing: EntityId = "urn:ngsi-ld:Building:3".parse()?;
        verify_that!(with_id(&items, &missing).is_none(), eq(true))?;
        verify_that!(with_id(&items, &first).is_some(), eq(true))
    }
}
