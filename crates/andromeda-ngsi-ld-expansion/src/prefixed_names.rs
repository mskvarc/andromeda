//! The clause 4.6.2 check of the attribute names, entity types and
//! `objectType`s a request writes with a prefix, and of the IRI each attribute
//! name expands to, made before either expansion path reads them.
//!
//! Both the typed walk and the processor leave only the IRI a name expands to,
//! and both drop a member whose name expands to no IRI (JSON-LD 1.1 expansion
//! algorithm step 13.3), so the spelling and the expansion are checked on the
//! typed compact model, each name against the context it is read under: the
//! request's own, or, below a root term that declares one, that term's
//! property-scoped context (step 8). A scoped context nested in another is not
//! processed here, so the outer one stays in force below it.

use crate::written_member::{WrittenMember, WrittenMemberRef};
use andromeda_jsonld::engine::{
    expansion_context::ExpansionContext,
    key_expansion::{KeyExpansion, expand_key},
    prefixed_name::check_prefixed_name,
};
use andromeda_ngsi_ld_entity::{name_refusal::NameRefusal, refusal::EntityRefusal};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, coerced_term::CoercedTerm};
use andromeda_ngsi_ld_model::{
    attribute::{instance::Attribute, one_or_many::OneOrMany, patched::Patched},
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
/// Returns [`NameRefusal::PrefixedEntityType`] or
/// [`NameRefusal::PrefixedAttributeName`] for the first name that breaks the
/// grammar, and [`NameRefusal::UnexpandedAttributeName`] for the first
/// attribute name that expands to no IRI.
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
/// Returns [`NameRefusal::PrefixedAttributeName`] for the first name that
/// breaks the grammar, and [`NameRefusal::UnexpandedAttributeName`] for the
/// first that expands to no IRI.
pub(crate) fn attribute<M: WrittenMember>(context: &ExpansionContext, name: &AttributeName, member: &M) -> Result<(), EntityRefusal> {
    attribute_name(context.active(), name)?;
    let active = member_scope(context, name.as_str());
    object_types(active, name, member.written())?;
    sub_attributes(active, member.written()).map_err(|refusal| EntityRefusal::from(*refusal))
}

/// Check the attribute name a request path writes, read apart from any
/// fragment (clauses 5.6.5, 5.6.13, 5.6.14 and 5.6.15).
///
/// # Errors
///
/// Returns [`NameRefusal::PrefixedAttributeName`] for a name that breaks the
/// grammar, and [`NameRefusal::UnexpandedAttributeName`] for one that expands
/// to no IRI.
pub fn path_attribute(context: &ExpansionContext, name: &AttributeName) -> Result<(), EntityRefusal> {
    Ok(attribute_name(context.active(), name)?)
}

/// Check every type of an entity.
fn types(context: &ExpansionContext, entity_types: &EntityTypeValue) -> Result<(), NameRefusal> {
    entity_types.iter().try_for_each(|entity_type| {
        check_prefixed_name(context.active(), entity_type.as_str()).map_err(|_| NameRefusal::PrefixedEntityType {
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
            sub_attributes(member_scope(context, key), member.written()).map_err(|refusal| EntityRefusal::from(*refusal))?;
        }
    }
    others.into_iter().try_for_each(|(name, member)| {
        attribute_name(context.active(), name)?;
        let active = member_scope(context, name.as_str());
        object_types(active, name, member.written())?;
        sub_attributes(active, member.written()).map_err(|refusal| EntityRefusal::from(*refusal))
    })
}

/// Check every `objectType` the instances of `member` write, each an entity
/// type name (clause 4.5.3.2), as an entity type is checked.
fn object_types(active: &Context<IriBuf, BlankIdBuf>, name: &AttributeName, member: WrittenMemberRef<'_>) -> Result<(), NameRefusal> {
    let check = |object_type: &CoercedTerm| {
        check_prefixed_name(active, object_type.as_str()).map_err(|_| NameRefusal::PrefixedObjectType {
            attribute: name.clone(),          // clone: the refusal outlives the borrowed name, so it owns a copy
            object_type: object_type.clone(), // clone: the refusal outlives the borrowed term, so it owns a copy
        })
    };
    match member {
        WrittenMemberRef::Attribute(member) => member
            .instances()
            .filter_map(written_object_types)
            .flat_map(OneOrMany::iter)
            .try_for_each(check),
        WrittenMemberRef::Merge(member) => member
            .instances()
            .filter_map(|instance| match &instance.object_type {
                Some(Patched::Set(object_types)) => Some(object_types),
                Some(Patched::Null) | None => None,
            })
            .flat_map(OneOrMany::iter)
            .try_for_each(check),
    }
}

/// The `objectType`s one instance writes: a Relationship's or a
/// `ListRelationship`'s, the only families that carry one.
const fn written_object_types(instance: &Attribute) -> Option<&OneOrMany<CoercedTerm>> {
    match instance {
        Attribute::Relationship(relationship) => relationship.object_type.as_ref(),
        Attribute::ListRelationship(list) => list.object_type.as_ref(),
        Attribute::Property(_)
        | Attribute::VocabProperty(_)
        | Attribute::GeoProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::ListProperty(_) => None,
    }
}

/// The context the members of the attribute written under `key` are read
/// under.
fn member_scope<'c>(context: &'c ExpansionContext, key: &str) -> &'c Context<IriBuf, BlankIdBuf> {
    context.property_scoped(key).unwrap_or_else(|| context.active())
}

/// Check the sub-attributes of every instance of `member`, at any depth.
///
/// The refusal travels boxed along the recursion, and each level's own checks
/// run in [`sub_attribute_level`], which returns before the next level is
/// entered, so a level costs a few words of stack and a chain of
/// sub-attributes as long as the reader admits is checked on the stack the
/// expansion runs on.
fn sub_attributes(active: &Context<IriBuf, BlankIdBuf>, member: WrittenMemberRef<'_>) -> Result<(), Box<NameRefusal>> {
    match member {
        WrittenMemberRef::Attribute(member) => member
            .instances()
            .flat_map(|instance| &instance.common().sub_attributes)
            .try_for_each(|(name, sub_attribute)| sub_attribute_tree(active, name, sub_attribute.written())),
        WrittenMemberRef::Merge(member) => member
            .instances()
            .flat_map(|instance| &instance.sub_attributes)
            .try_for_each(|(name, sub_attribute)| sub_attribute_tree(active, name, sub_attribute.written())),
    }
}

/// Check the sub-attribute `name` writing `member`, then its own
/// sub-attributes.
fn sub_attribute_tree(active: &Context<IriBuf, BlankIdBuf>, name: &AttributeName, member: WrittenMemberRef<'_>) -> Result<(), Box<NameRefusal>> {
    sub_attribute_level(active, name, member).map_err(Box::new)?;
    sub_attributes(active, member)
}

/// Check the name of one sub-attribute and the `objectType`s it writes.
fn sub_attribute_level(active: &Context<IriBuf, BlankIdBuf>, name: &AttributeName, member: WrittenMemberRef<'_>) -> Result<(), NameRefusal> {
    attribute_name(active, name)?;
    object_types(active, name, member)
}

/// Check one attribute name: its spelling after a prefix, then the IRI it
/// expands to.
fn attribute_name(active: &Context<IriBuf, BlankIdBuf>, name: &AttributeName) -> Result<(), NameRefusal> {
    check_prefixed_name(active, name.as_str()).map_err(|_| NameRefusal::PrefixedAttributeName {
        attribute: name.clone(), // clone: the refusal outlives the borrowed name, so it owns a copy
    })?;
    match expand_key(active, name.as_str()) {
        KeyExpansion::Iri(_) | KeyExpansion::Keyword => Ok(()),
        KeyExpansion::NoIri => Err(NameRefusal::UnexpandedAttributeName {
            attribute: name.clone(), // clone: the refusal outlives the borrowed name, so it owns a copy
        }),
    }
}
