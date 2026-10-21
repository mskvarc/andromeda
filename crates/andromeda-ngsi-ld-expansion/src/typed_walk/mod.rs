//! Expanding the typed compact model the transport decodes a request into.
//!
//! The result is the tree the JSON-LD processor's expansion of the request's
//! normalized serialization yields, built without that serialization: entity
//! members are expanded as the JSON their typed values serialize to, attribute
//! instances member by member ([`crate::written_member::WrittenMember`]). The model
//! read every concise member into the family it abbreviates already; the one
//! member the write decides is the NGSI-LD Null written in place of a whole
//! attribute.

pub mod instance;
pub(crate) mod merge_instance;

use crate::{
    compact_walk::{element::Expanded, object::NodeBuilder},
    decline::Decline,
    normalization::Normalization,
    term_scope::TermScope,
    typed_walk::instance::{add_member, add_optional, add_serialized, is_ngsi_ld_null},
    written_member::WrittenMember,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, time::rendering::instant_text};
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use andromeda_ngsi_ld_vocabulary::{NULL_URI, compact::properties as compact_properties};

/// The member naming an entity.
const ID_MEMBER: &str = "id";

/// The member naming an entity's types.
const TYPE_MEMBER: &str = "type";

/// Expand a typed entity written at the top of a request.
///
/// # Errors
///
/// Declines whatever the walk does not model.
pub fn entity_node<M: WrittenMember>(scope: &TermScope<'_>, entity: &Entity<M>, normalization: Normalization) -> Result<Expanded, Decline> {
    let mut node = NodeBuilder::default();
    add_serialized(&mut node, scope, ID_MEMBER, &entity.id)?;
    add_serialized(&mut node, scope, TYPE_MEMBER, &entity.type_)?;
    add_optional(&mut node, scope, compact_properties::SCOPE, entity.scope.as_ref())?;
    add_optional(&mut node, scope, compact_properties::EXPIRES_AT, entity.expires_at.map(instant_text).as_ref())?;
    add_optional(&mut node, scope, compact_properties::CREATED_AT, entity.created_at.map(instant_text).as_ref())?;
    add_optional(&mut node, scope, compact_properties::MODIFIED_AT, entity.modified_at.map(instant_text).as_ref())?;
    add_optional(&mut node, scope, compact_properties::DELETED_AT, entity.deleted_at.map(instant_text).as_ref())?;
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
pub fn patch_node<M: WrittenMember>(scope: &TermScope<'_>, patch: &EntityPatch<M>, normalization: Normalization) -> Result<Expanded, Decline> {
    let mut node = NodeBuilder::default();
    add_optional(&mut node, scope, ID_MEMBER, patch.id.as_ref())?;
    add_optional(&mut node, scope, TYPE_MEMBER, patch.type_.as_ref())?;
    add_optional(&mut node, scope, compact_properties::SCOPE, patch.scope.as_ref())?;
    add_optional(&mut node, scope, compact_properties::EXPIRES_AT, patch.expires_at.map(instant_text).as_ref())?;
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
fn attributes<'a, M: WrittenMember + 'a>(
    node: &mut NodeBuilder,
    scope: &TermScope<'_>,
    geo_members: [(&str, Option<&M>); 3],
    others: impl IntoIterator<Item = (&'a AttributeName, &'a M)>,
    normalization: Normalization,
) -> Result<(), Decline> {
    let present = geo_members.into_iter().filter_map(|(key, member)| member.map(|member| (key, member)));
    for (key, member) in present.chain(others.into_iter().map(|(name, member)| (name.as_str(), member))) {
        match normalization {
            // A merge removes the whole member by the Null (clause 5.5.12), and
            // a notification reports it deleted by it (clause 5.8.6).
            Normalization::MemberDeletions if is_ngsi_ld_null(member) => add_serialized(node, scope, key, NULL_URI.as_str())?,
            Normalization::MemberDeletions | Normalization::Entity => add_member(node, scope, key, member)?,
        }
    }
    Ok(())
}
