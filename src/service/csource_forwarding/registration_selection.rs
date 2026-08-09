//! Registration-information matching and forwarded-fragment partitioning.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, RegistrationInfo};
use iri_rs::IriBuf;
use serde_json::Value;

/// Build a minimal forwarding query string with only type and id params.
pub(crate) fn collect_exclusive_attrs(csr: &ContextSourceRegistration) -> Vec<AttributeName> {
    let mut attrs = Vec::new();
    for info in &csr.coverage.information {
        for name in info.property_names.iter().chain(info.relationship_names.iter()).flatten() {
            attrs.push(name.clone()); // clone: the result owns names borrowed from the registration.
            // Also add the compacted form (last path/fragment segment) for
            // matching against deserialized EntityPatch attr keys, which are
            // compacted.
            if let Some(short) = compact_attribute_name(name)
                && short != *name
            {
                attrs.push(short);
            }
        }
    }
    attrs
}

/// Attribute names from matching information entries that this request covers.
pub(crate) fn collect_matching_attrs(
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    explicit_attribute: Option<&AttributeName>,
    body: Option<&[u8]>,
) -> Vec<AttributeName> {
    if let Some(attribute) = explicit_attribute {
        return vec![attribute.clone()]; // clone: the result outlives the borrowed request.
    }
    let requested = body.and_then(fragment_attribute_names).unwrap_or_default();
    let mut covered = Vec::new();
    for information in csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_type, entity_id))
    {
        let declared = information.property_names.iter().chain(&information.relationship_names).flatten();
        let declared = declared.collect::<Vec<_>>();
        if declared.is_empty() {
            covered.extend(requested.iter().cloned());
        } else if requested.is_empty() {
            covered.extend(declared.iter().map(|name| (*name).clone()));
        } else {
            covered.extend(
                requested
                    .iter()
                    .filter(|attribute| {
                        declared
                            .iter()
                            .any(|name| *name == *attribute || compact_attribute_name(name).as_ref() == Some(attribute))
                    })
                    .cloned(),
            );
        }
    }
    covered.sort();
    covered.dedup();
    covered
}

/// Retain only attributes covered by matching information entries.
pub(crate) fn filter_fragment_for_registration(
    body: Option<&[u8]>,
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
) -> Option<Vec<u8>> {
    let body = body?;
    let Ok(mut value) = serde_json::from_slice::<Value>(body) else {
        return Some(body.to_vec());
    };
    let Some(object) = value.as_object_mut() else {
        return Some(body.to_vec());
    };
    let information = csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_type, entity_id))
        .collect::<Vec<_>>();
    let unrestricted = information
        .iter()
        .any(|entry| entry.property_names.as_ref().is_none_or(Vec::is_empty) && entry.relationship_names.as_ref().is_none_or(Vec::is_empty));
    if unrestricted {
        return Some(body.to_vec());
    }
    let covered = information
        .iter()
        .flat_map(|entry| entry.property_names.iter().chain(&entry.relationship_names).flatten())
        .flat_map(|name| [Some(name.clone()), compact_attribute_name(name)])
        .flatten()
        .collect::<Vec<_>>();
    object.retain(|name, _value| is_entity_control_member(name) || covered.iter().any(|covered_name| covered_name.as_str() == name));
    if object.keys().all(|name| is_entity_control_member(name)) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

/// Attribute names present in an entity or entity-fragment payload.
pub(crate) fn fragment_attribute_names(body: &[u8]) -> Option<Vec<AttributeName>> {
    let value = serde_json::from_slice::<Value>(body).ok()?;
    let object = value.as_object()?;
    Some(
        object
            .keys()
            .filter(|name| !is_entity_control_member(name))
            .filter_map(|name| name.parse().ok())
            .collect(),
    )
}

/// Whether a member is entity metadata rather than a forwarded attribute.
pub(crate) fn is_entity_control_member(name: &str) -> bool {
    matches!(name, "@context" | "id" | "type" | "scope" | "createdAt" | "modifiedAt" | "expiresAt")
}

/// Whether one information entry covers the entity and requested attribute.
pub(crate) fn csr_has_matching_information(
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
    attribute: Option<&AttributeName>,
) -> bool {
    csr.coverage
        .information
        .iter()
        .any(|information| information_matches_entity(information, entity_type, entity_id) && information_covers_attribute(information, attribute))
}

/// Whether the entity selector axis of one information entry matches.
pub(crate) fn information_matches_entity(information: &RegistrationInfo, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> bool {
    let Some(selectors) = information.entities.as_deref().filter(|selectors| !selectors.is_empty()) else {
        return true;
    };
    selectors.iter().any(|selector| {
        let type_matches = entity_type.is_none_or(|requested| selector.type_.matches(&[requested.as_str()]));
        let id_matches = entity_id.is_none_or(|requested| match (&selector.id, &selector.id_pattern) {
            (None, None) => true,
            (Some(id), None) => id.as_str() == requested.as_str(),
            (None, Some(pattern)) => pattern.is_match(requested.as_str()),
            (Some(id), Some(pattern)) => id.as_str() == requested.as_str() || pattern.is_match(requested.as_str()),
        });
        type_matches && id_matches
    })
}

/// Whether an attribute is declared by the same information entry.
pub(crate) fn information_covers_attribute(information: &RegistrationInfo, attribute: Option<&AttributeName>) -> bool {
    let Some(attribute) = attribute else {
        return true;
    };
    let names = information.property_names.iter().chain(&information.relationship_names).flatten();
    let mut saw_name = false;
    for name in names {
        saw_name = true;
        if name == attribute || compact_attribute_name(name).as_ref() == Some(attribute) {
            return true;
        }
    }
    !saw_name
}

/// Extract the short name from an expanded IRI by taking the fragment or last path segment.
pub(crate) fn compact_attribute_name(attribute: &AttributeName) -> Option<AttributeName> {
    let iri = attribute.as_str();
    let compact = if let Some(pos) = iri.rfind('#') {
        &iri[pos + 1..]
    } else {
        iri.rfind('/').map(|pos| &iri[pos + 1..])?
    };
    compact.parse().ok()
}
