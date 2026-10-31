//! The chain of entities a deep join test writes: each entity names the next one in a Relationship `linkedTo`, and the
//! last names none, so a join of `links` levels from the first reaches every other entity (clause 4.5.23).

use crate::namespace::{Namespace, NamespaceError};
use ngsi_ld::model::{BuildError, Entity, EntityTypeName, Input};

/// Why a chain could not be built.
#[derive(Debug, thiserror::Error)]
pub enum ChainError {
    /// An identifier of the chain is not a valid URN.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// The model refused an entity of the chain.
    #[error(transparent)]
    Build(#[from] BuildError),
}

/// The `links + 1` entities of type `building_type` with identifiers `urn:ngsi-ld:Building:<token>-<n>` for `n` from
/// 0 to `links`: the entity `n` below `links` holds the Relationship `linkedTo` to the entity `n + 1`.
///
/// # Errors
///
/// Returns [`ChainError`] when an identifier or an entity is refused.
pub fn building_chain(namespace: &Namespace, building_type: &EntityTypeName, links: usize) -> Result<Vec<Entity<Input>>, ChainError> {
    (0..=links)
        .map(|n| -> Result<Entity<Input>, ChainError> {
            let id = namespace.urn("Building", n)?;
            if n < links {
                Ok(Entity::builder()
                    .id(id)
                    .entity_type(building_type.clone())
                    .relationship("linkedTo", namespace.urn("Building", n + 1)?)
                    .build()?)
            } else {
                Ok(Entity::builder().id(id).entity_type(building_type.clone()).build()?)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{linked_chain::building_chain, namespace::Namespace};
    use googletest::prelude::*;

    #[gtest]
    fn a_chain_of_links_holds_one_more_entity_and_the_last_names_none() -> Result<()> {
        let namespace = Namespace::fresh();
        let building_type = namespace.entity_type("Building")?;
        let chain = building_chain(&namespace, &building_type, 3)?;
        expect_that!(chain, len(eq(4)));
        expect_that!(chain.first().map(|entity| entity.attributes.len()), some(eq(1)));
        verify_that!(chain.last().map(|entity| entity.attributes.len()), some(eq(0)))
    }
}
