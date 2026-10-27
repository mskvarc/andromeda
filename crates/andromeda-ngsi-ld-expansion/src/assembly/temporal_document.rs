//! Turning an expanded tree into the Temporal Evolution of an Entity (clauses
//! 4.5.6 and 4.5.7).
//!
//! The same assembly as a write's, with the readings that differ: every
//! attribute holds the sequence of instances it took over time, the Scope is
//! one of those attributes, a Property whose value is the Scope paths in force
//! from the instant it names (clause 4.18) and not the entity's own `scope`
//! member, and the entity's system times are the peer's, carried apart.

use crate::{assembly::attribute_member, expanded_tree::ExpandedNode};
use andromeda_ngsi_ld_entity::{document::EntityDocument, member_role::MemberRole, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_ngsi_ld_model::{entity_system_times::EntitySystemTimes, temporal::temporal_document::TemporalDocument};
use andromeda_ngsi_ld_vocabulary::expanded::properties;
use indexmap::IndexMap;

/// Read the expanded entity node of a Temporal Evolution, `system_times`
/// being the entity's own clause 4.8 times it was written with.
///
/// # Errors
///
/// Returns the refusal of the first member that is not what its expanded IRI
/// names.
pub fn temporal_document(node: Option<ExpandedNode>, system_times: EntitySystemTimes) -> Result<TemporalDocument, EntityRefusal> {
    let Some(node) = node else {
        return Ok(TemporalDocument::new(EntityDocument::default(), system_times, IndexMap::new()));
    };
    let id = node
        .id
        .map(|id| id.parse::<Identifier>().map_err(|source| EntityRefusal::EntityId { source }))
        .transpose()?;
    let types = node
        .types
        .iter()
        .map(|type_name| type_name.parse::<EntityTypeName>().map_err(|source| EntityRefusal::EntityType { source }))
        .collect::<Result<Vec<_>, _>>()?;
    let mut attributes = IndexMap::with_capacity(node.members.len());
    for (name, items) in node.members {
        if is_entity_member(&name) {
            continue;
        }
        let name = name.parse::<AttributeName>().map_err(EntityRefusal::ExpandedAttributeName)?;
        if name.as_str() != properties::SCOPE.as_str() && MemberRole::is_reserved(name.as_str()) {
            return Err(EntityRefusal::ReservedAttributeName { attribute: name });
        }
        let member = attribute_member(&name, items)?;
        attributes.insert(name, member);
    }
    Ok(TemporalDocument::new(
        EntityDocument::new(id, types, None, None, attributes),
        system_times,
        IndexMap::new(),
    ))
}

/// Whether the expanded member `iri` is one of the entity's own that a
/// Temporal Evolution carries apart from its attributes: the expiry, which is
/// the entity's and not its history's, and the clause 4.8 system times, which
/// are read from the compact entity.
fn is_entity_member(iri: &str) -> bool {
    [properties::EXPIRES_AT, properties::CREATED_AT, properties::MODIFIED_AT, properties::DELETED_AT]
        .iter()
        .any(|member| member.as_str() == iri)
}
