//! The placeholders a drawn `q` writes for what only the namespace of a run decides.
//!
//! A filter is drawn before the namespace its case runs under exists, so its `q` cannot spell the identifier of a
//! drawn Entity or the entity type of the case. It writes `@N@` for the identifier of the Entity at position N and
//! `@type@` for the type, and is resolved against the namespace before it is parsed. No pool value holds an `@`.

use googletest::prelude::*;
use ngsi_ld::model::{EntityId, EntityTypeName};

/// The placeholder of the entity type of the case.
pub const ENTITY_TYPE: &str = "@type@";

/// The placeholder of the identifier of the Entity at `position`.
#[must_use]
pub fn entity(position: usize) -> String {
    format!("@{position}@")
}

/// `text` with each placeholder replaced: the type's with `entity_type`, and the one of the Entity at each position
/// with the identifier at that position of `ids`.
///
/// # Errors
///
/// Fails when an `@` is unpaired, or a placeholder names neither the type nor a position of `ids`.
pub fn resolved(text: &str, ids: &[EntityId], entity_type: &EntityTypeName) -> Result<String> {
    if text.matches('@').count() % 2 == 1 {
        fail!("{text} holds an unpaired @")?;
    }
    let type_name = ENTITY_TYPE.trim_matches('@');
    let mut resolved = String::with_capacity(text.len());
    for (index, piece) in text.split('@').enumerate() {
        if index % 2 == 0 {
            resolved.push_str(piece);
        } else if piece == type_name {
            resolved.push_str(entity_type.as_str());
        } else {
            resolved.push_str(&ids.get(piece.parse::<usize>()?).or_fail()?.to_string());
        }
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use crate::q_generation::placeholder::{ENTITY_TYPE, entity, resolved};
    use googletest::prelude::*;
    use ngsi_ld::model::{EntityId, EntityTypeName};

    #[gtest]
    fn every_placeholder_resolves_to_the_identifier_at_its_position_or_to_the_type() -> Result<()> {
        let ids: Vec<EntityId> = vec!["urn:x:zero".parse()?, "urn:x:one".parse()?];
        let entity_type: EntityTypeName = "Drawn_t".parse()?;
        let text = format!("r=={};r{{{ENTITY_TYPE}:p1}}==\"{}\"", entity(1), entity(0));
        verify_that!(resolved(&text, &ids, &entity_type)?, eq("r==urn:x:one;r{Drawn_t:p1}==\"urn:x:zero\""))
    }

    #[gtest]
    fn a_placeholder_past_the_identifiers_does_not_resolve() -> Result<()> {
        let ids: Vec<EntityId> = vec!["urn:x:zero".parse()?];
        verify_that!(resolved(&format!("r=={}", entity(1)), &ids, &"Drawn_t".parse()?), err(anything()))
    }
}
