//! Expanding the typed compact model the transport decodes a request into.
//!
//! The result is the tree the JSON-LD processor's expansion of the request's
//! serialization (after its concise rewrite) yields, built without that
//! serialization: entity members are expanded as the JSON their typed values
//! serialize to, attribute instances member by member
//! ([`instance::attribute_member`]), and a member the typed model could not
//! read is rewritten and expanded as the JSON it is.

pub mod instance;

use crate::{
    compact_walk::{element::Expanded, object::NodeBuilder},
    decline::Decline,
    normalization::Normalization,
    term_scope::TermScope,
    typed_walk::instance::{add_json, add_optional, add_serialized, attribute_member},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    attribute::instance::AttributeValue,
    entity::{Entity, EntityPatch},
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use serde_json::Value;

/// The member naming an entity.
const ID_MEMBER: &str = "id";

/// The member naming an entity's types.
const TYPE_MEMBER: &str = "type";

/// Expand a typed entity written at the top of a request.
///
/// # Errors
///
/// Declines whatever the walk does not model.
pub fn entity_node(scope: &TermScope<'_>, entity: &Entity, normalization: Normalization) -> Result<Expanded, Decline> {
    let mut node = NodeBuilder::default();
    add_serialized(&mut node, scope, ID_MEMBER, &entity.id)?;
    add_serialized(&mut node, scope, TYPE_MEMBER, &entity.type_)?;
    add_optional(&mut node, scope, compact_properties::SCOPE, entity.scope.as_ref())?;
    add_optional(&mut node, scope, compact_properties::EXPIRES_AT, entity.expires_at.as_ref())?;
    add_optional(&mut node, scope, compact_properties::CREATED_AT, entity.created_at.as_ref())?;
    add_optional(&mut node, scope, compact_properties::MODIFIED_AT, entity.modified_at.as_ref())?;
    add_optional(&mut node, scope, compact_properties::DELETED_AT, entity.deleted_at.as_ref())?;
    let geo_members = [
        (compact_properties::LOCATION, entity.location.as_ref()),
        (compact_properties::OBSERVATION_SPACE, entity.observation_space.as_ref()),
        (compact_properties::OPERATION_SPACE, entity.operation_space.as_ref()),
    ];
    attributes(&mut node, scope, geo_members, &entity.attrs, normalization)?;
    Ok(node.finish(None))
}

/// Expand a typed entity fragment written at the top of a request.
///
/// # Errors
///
/// Declines whatever the walk does not model.
pub fn patch_node(scope: &TermScope<'_>, patch: &EntityPatch, normalization: Normalization) -> Result<Expanded, Decline> {
    let mut node = NodeBuilder::default();
    add_optional(&mut node, scope, ID_MEMBER, patch.id.as_ref())?;
    add_optional(&mut node, scope, TYPE_MEMBER, patch.type_.as_ref())?;
    add_optional(&mut node, scope, compact_properties::SCOPE, patch.scope.as_ref())?;
    add_optional(&mut node, scope, compact_properties::EXPIRES_AT, patch.expires_at.as_ref())?;
    let geo_members = [
        (compact_properties::LOCATION, patch.location.as_ref()),
        (compact_properties::OBSERVATION_SPACE, patch.observation_space.as_ref()),
        (compact_properties::OPERATION_SPACE, patch.operation_space.as_ref()),
    ];
    attributes(&mut node, scope, geo_members, &patch.attrs, normalization)?;
    Ok(node.finish(None))
}

/// Expand every attribute member: the three the model holds in fields of
/// their own (clause 5.2.4), then the rest.
fn attributes<'a>(
    node: &mut NodeBuilder,
    scope: &TermScope<'_>,
    geo_members: [(&str, Option<&AttributeValue>); 3],
    others: impl IntoIterator<Item = (&'a AttributeName, &'a AttributeValue)>,
    normalization: Normalization,
) -> Result<(), Decline> {
    let present = geo_members.into_iter().filter_map(|(key, member)| member.map(|member| (key, member)));
    for (key, member) in present.chain(others.into_iter().map(|(name, member)| (name.as_str(), member))) {
        match member {
            AttributeValue::Single(_) => attribute_member(node, scope, key, member)?,
            AttributeValue::Multiple(instances) if !instances.is_empty() => attribute_member(node, scope, key, member)?,
            // Not instances: the member goes through the concise rewrite the
            // write kind runs, and is expanded as the JSON it then is.
            AttributeValue::Multiple(_) => add_json(node, scope, key, &normalization.apply_to_member(key, Value::Array(Vec::new())))?,
            AttributeValue::Raw(raw) => add_json(node, scope, key, &normalization.apply_to_member(key, raw.clone()))?, // clone: the typed request is borrowed, and the rewrite works on its own copy
        }
    }
    Ok(())
}
