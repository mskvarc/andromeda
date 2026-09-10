//! The short names a discovery answer renders its stored names as.
//!
//! Tables 5.2.25-1, 5.2.26-1 and 5.2.28-1 all word the rule the same way — a
//! name is rendered as the "short name if contained in @context" — so every
//! discovery surface takes the same compaction, and none of them may reach for
//! a compaction of its own. The identifiers beside those names are the
//! opposite: Table 5.2.28-1 fixes the Attribute `id` to the "Full URI of
//! attribute name" and Tables 5.2.25-1 and 5.2.26-1 fix the entity type `id`
//! to the "Fully Qualified Name (FQN) of the entity type being described", so
//! a discovery resource is serialized as it is built rather than compacted as
//! a JSON-LD document, which would shorten the identifiers too.

use crate::{
    discovery::{
        EntityDiscoveryService,
        stored_names::{parse_attribute_name, parse_type_name},
    },
    error::EntityError,
};
use andromeda_jsonld::{
    compaction::{compact_attr_iri, compact_type_iri},
    context_spec::ContextSpec,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};

impl EntityDiscoveryService {
    /// The short name `context` gives a stored attribute name, or the name
    /// itself when the context declares no term for it.
    pub(super) async fn compacted_attribute_name(&self, iri: &AttributeName, context: &ContextSpec) -> Result<AttributeName, EntityError> {
        parse_attribute_name(&compact_attr_iri(&self.engine, iri.as_str(), context).await)
    }

    /// The short name `context` gives a stored entity type name, or the name
    /// itself when the context declares no term for it.
    pub(super) async fn compacted_type_name(&self, iri: &EntityTypeName, context: &ContextSpec) -> Result<EntityTypeName, EntityError> {
        parse_type_name(&compact_type_iri(&self.engine, iri.as_str(), context).await)
    }

    /// The short names `context` gives a list of stored attribute names.
    pub(super) async fn compacted_attribute_names(
        &self,
        iris: impl IntoIterator<Item = AttributeName>,
        context: &ContextSpec,
    ) -> Result<Vec<AttributeName>, EntityError> {
        let mut names = Vec::new();
        for iri in iris {
            names.push(self.compacted_attribute_name(&iri, context).await?);
        }
        Ok(names)
    }

    /// The short names `context` gives a list of stored entity type names.
    pub(super) async fn compacted_type_names(
        &self,
        iris: impl IntoIterator<Item = EntityTypeName>,
        context: &ContextSpec,
    ) -> Result<Vec<EntityTypeName>, EntityError> {
        let mut names = Vec::new();
        for iri in iris {
            names.push(self.compacted_type_name(&iri, context).await?);
        }
        Ok(names)
    }
}
