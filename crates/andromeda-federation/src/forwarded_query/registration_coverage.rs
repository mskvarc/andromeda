//! What one registration covers of what a client asked for (clause 5.2.10).
//!
//! Clause 4.3.6.1 makes the broker responsible for respecting every constraint
//! a registration declares, so a forwarded request names only the identifiers,
//! types and attributes the registration covers. Deciding *what* it covers is
//! a question about the registration alone, which is why it is answered here
//! rather than inside the query the answer narrows.

use crate::registration_selection::{compact_attribute_name, information_matches_entity};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::{
    entity_info::EntityTypeSelection,
    registration::{ContextSourceRegistration, info::RegistrationInfo},
};
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use chrono::{DateTime, SecondsFormat, Utc};

/// An instant in the RFC 3339 form the temporal parameters are written in.
pub(super) fn rfc3339(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// The identifiers among `ids` that some information entry of the registration
/// covers, or `None` when it covers none of them.
pub(super) fn covered_ids(ids: &[Uri], registration: &ContextSourceRegistration) -> Option<Vec<Uri>> {
    let covered = ids
        .iter()
        .filter(|id| {
            registration
                .coverage
                .information
                .iter()
                .any(|information| information_matches_entity(information, None, Some(id)))
        })
        .cloned() // clone: the forwarded query owns the identifiers it writes
        .collect::<Vec<_>>();
    (!covered.is_empty()).then_some(covered)
}

/// The part of the client's type selector the registration covers, or `None`
/// when it covers none of it.
///
/// Only a flat alternation of names can be narrowed; the wildcard and a
/// conjunction are forwarded whole, because removing a member would change
/// what they select rather than restrict it.
pub(super) fn covered_type(entity_type: &TypeExprRaw, registration: &ContextSourceRegistration) -> Option<TypeExprRaw> {
    let selectors = registration
        .coverage
        .information
        .iter()
        .filter_map(|information| information.entities.as_deref())
        .flatten()
        .map(|selector| &selector.type_)
        .collect::<Vec<_>>();
    let unrestricted = registration.coverage.information.iter().any(|information| information.entities.is_none());
    if unrestricted || selectors.is_empty() {
        return Some(entity_type.clone()); // clone: the forwarded query owns the selector it writes
    }
    let leaves = match entity_type {
        TypeExprRaw::Type(name) => vec![name],
        TypeExprRaw::Or(children) => children
            .iter()
            .map(|child| match child {
                TypeExprRaw::Type(name) => Some(name),
                TypeExprRaw::Wildcard | TypeExprRaw::And(_) | TypeExprRaw::Or(_) => None,
            })
            .collect::<Option<Vec<_>>>()?,
        TypeExprRaw::Wildcard | TypeExprRaw::And(_) => return Some(entity_type.clone()), // clone: same
    };
    let mut covered = leaves
        .into_iter()
        .filter(|name| selectors.iter().any(|selector| selector_covers_type(selector, name)))
        .cloned() // clone: the narrowed selector owns its names
        .map(TypeExprRaw::Type)
        .collect::<Vec<_>>();
    match covered.len() {
        0 => None,
        1 => covered.pop(),
        _ => Some(TypeExprRaw::Or(covered)),
    }
}

/// Whether a registered type selection (Table 5.2.8-1: one name or several)
/// admits the client's compact or expanded type name. Registered names are
/// stored expanded, so a compact client name is compared against the last
/// segment of the registered IRI as well.
fn selector_covers_type(selector: &EntityTypeSelection, name: &EntityTypeName) -> bool {
    selector
        .names()
        .iter()
        .any(|registered| registered == name || compact_type_name(registered).as_ref() == Some(name))
}

/// The last fragment or path segment of an expanded type IRI, as the compact
/// name a client most likely wrote for it.
fn compact_type_name(name: &EntityTypeName) -> Option<EntityTypeName> {
    let iri = name.as_ref();
    let compact = match iri.rfind('#') {
        Some(position) => &iri[position + 1..],
        None => iri.rfind('/').map(|position| &iri[position + 1..])?,
    };
    compact.parse().ok()
}

/// The attributes among `attrs` the registration declares, or every one of
/// them when it declares no attribute names at all (clause 5.2.10: a block
/// without names covers every attribute of its entities); `None` when it
/// declares names and none of the requested ones is among them.
pub(super) fn covered_attrs(attrs: &[AttributeName], registration: &ContextSourceRegistration) -> Option<Vec<AttributeName>> {
    let declared = registration
        .coverage
        .information
        .iter()
        .flat_map(RegistrationInfo::attribute_names)
        .collect::<Vec<_>>();
    if declared.is_empty() {
        return Some(attrs.to_vec()); // clone: the forwarded query owns the names it writes
    }
    let covered = attrs
        .iter()
        .filter(|attribute| {
            declared
                .iter()
                .any(|name| *name == *attribute || compact_attribute_name(name).as_ref() == Some(attribute))
        })
        .cloned() // clone: same
        .collect::<Vec<_>>();
    (!covered.is_empty()).then_some(covered)
}
