//! The clause 4.6.2 check of the attribute names and entity types a request
//! writes with a prefix, made before either expansion path reads them.
//!
//! Both the typed walk and the processor leave only the IRI a name expands to,
//! so the spelling is checked on the typed compact model, each name against the
//! context it is read under: the request's own, or, below a root term that
//! declares one, that term's property-scoped context (JSON-LD 1.1 expansion
//! algorithm step 8). A scoped context nested in another is not processed
//! here, so the outer one stays in force below it.

use crate::written_member::{WrittenMember, WrittenMemberRef};
use andromeda_jsonld::engine::{expansion_context::ExpansionContext, prefixed_name::check_prefixed_name};
use andromeda_ngsi_ld_entity::refusal::EntityRefusal;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::{
    entity::{Entity, EntityPatch},
    entity_type::EntityTypeValue,
};
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use iri_rs::IriBuf;
use jsonld::Context;
use rdfx::BlankIdBuf;

/// Check the types and attribute names of an entity.
///
/// # Errors
///
/// Returns [`EntityRefusal::PrefixedEntityType`] or
/// [`EntityRefusal::PrefixedAttributeName`] for the first name that breaks the
/// grammar.
pub(crate) fn entity<M: WrittenMember>(context: &ExpansionContext, entity: &Entity<M>) -> Result<(), EntityRefusal> {
    types(context, &entity.type_)?;
    let geo_members = [
        (compact_properties::LOCATION, entity.location.as_ref()),
        (compact_properties::OBSERVATION_SPACE, entity.observation_space.as_ref()),
        (compact_properties::OPERATION_SPACE, entity.operation_space.as_ref()),
    ];
    members(context, geo_members, &entity.attrs)
}

/// Check the types and attribute names of an entity fragment.
///
/// # Errors
///
/// As [`entity`].
pub(crate) fn patch<M: WrittenMember>(context: &ExpansionContext, patch: &EntityPatch<M>) -> Result<(), EntityRefusal> {
    if let Some(entity_types) = &patch.type_ {
        types(context, entity_types)?;
    }
    let geo_members = [
        (compact_properties::LOCATION, patch.location.as_ref()),
        (compact_properties::OBSERVATION_SPACE, patch.observation_space.as_ref()),
        (compact_properties::OPERATION_SPACE, patch.operation_space.as_ref()),
    ];
    members(context, geo_members, &patch.attrs)
}

/// Check the attribute a request path names and the sub-attributes of the
/// fragment written for it.
///
/// # Errors
///
/// Returns [`EntityRefusal::PrefixedAttributeName`] for the first name that
/// breaks the grammar.
pub(crate) fn attribute<M: WrittenMember>(context: &ExpansionContext, name: &AttributeName, member: &M) -> Result<(), EntityRefusal> {
    attribute_name(context.active(), name)?;
    sub_attributes(member_scope(context, name.as_str()), member.written())
}

/// Check the attribute name a request path writes, read apart from any
/// fragment (clauses 5.6.5, 5.6.13, 5.6.14 and 5.6.15).
///
/// # Errors
///
/// Returns [`EntityRefusal::PrefixedAttributeName`] for a name that breaks the
/// grammar.
pub fn path_attribute(context: &ExpansionContext, name: &AttributeName) -> Result<(), EntityRefusal> {
    attribute_name(context.active(), name)
}

/// Check every type of an entity.
fn types(context: &ExpansionContext, entity_types: &EntityTypeValue) -> Result<(), EntityRefusal> {
    entity_types.iter().try_for_each(|entity_type| {
        check_prefixed_name(context.active(), entity_type.as_str()).map_err(|_| EntityRefusal::PrefixedEntityType {
            entity_type: entity_type.clone(), // clone: the refusal outlives the borrowed type, so it owns a copy
        })
    })
}

/// Check every attribute member: the three the model holds in fields of their
/// own, whose names are core terms, then the rest.
fn members<'a, M: WrittenMember + 'a>(
    context: &ExpansionContext,
    geo_members: [(&str, Option<&M>); 3],
    others: impl IntoIterator<Item = (&'a AttributeName, &'a M)>,
) -> Result<(), EntityRefusal> {
    for (key, member) in geo_members {
        if let Some(member) = member {
            sub_attributes(member_scope(context, key), member.written())?;
        }
    }
    others.into_iter().try_for_each(|(name, member)| {
        attribute_name(context.active(), name)?;
        sub_attributes(member_scope(context, name.as_str()), member.written())
    })
}

/// The context the members of the attribute written under `key` are read
/// under.
fn member_scope<'c>(context: &'c ExpansionContext, key: &str) -> &'c Context<IriBuf, BlankIdBuf> {
    context.property_scoped(key).unwrap_or_else(|| context.active())
}

/// Check the sub-attribute names of every instance of `member`, at any depth.
fn sub_attributes(active: &Context<IriBuf, BlankIdBuf>, member: WrittenMemberRef<'_>) -> Result<(), EntityRefusal> {
    match member {
        WrittenMemberRef::Attribute(member) => {
            member
                .instances()
                .flat_map(|instance| &instance.common().sub_attributes)
                .try_for_each(|(name, sub_attribute)| {
                    attribute_name(active, name)?;
                    sub_attributes(active, sub_attribute.written())
                })
        }
        WrittenMemberRef::Merge(member) => member
            .instances()
            .flat_map(|instance| &instance.sub_attributes)
            .try_for_each(|(name, sub_attribute)| {
                attribute_name(active, name)?;
                sub_attributes(active, sub_attribute.written())
            }),
    }
}

/// Check one attribute name.
fn attribute_name(active: &Context<IriBuf, BlankIdBuf>, name: &AttributeName) -> Result<(), EntityRefusal> {
    check_prefixed_name(active, name.as_str()).map_err(|_| EntityRefusal::PrefixedAttributeName {
        attribute: name.clone(), // clone: the refusal outlives the borrowed name, so it owns a copy
    })
}
