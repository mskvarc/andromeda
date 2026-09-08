//! Registration-information matching and federated-fragment partitioning.

use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, scope::grammar::ScopeBuf, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, RegistrationInfo};
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use iri_rs::IriBuf;
use serde_json::{Map, Value};

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
    if !retain_covered_attributes(object, csr, entity_type, entity_id) {
        return None;
    }
    serde_json::to_vec(&value).ok()
}

/// The attributes a registration covers for one entity, read off the
/// information entries that match it (clause 5.12).
enum AttributeCoverage {
    /// No information entry matches the entity: nothing is covered.
    NotMatching,
    /// A matching entry declares no names, so every attribute is covered.
    Unrestricted,
    /// Only these declared names, in their expanded and compacted spellings.
    Names(Vec<AttributeName>),
}

/// What `csr` covers of an entity's attributes.
fn attribute_coverage(csr: &ContextSourceRegistration, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> AttributeCoverage {
    let information = csr
        .coverage
        .information
        .iter()
        .filter(|information| information_matches_entity(information, entity_type, entity_id))
        .collect::<Vec<_>>();
    if information.is_empty() {
        return AttributeCoverage::NotMatching;
    }
    let unrestricted = information
        .iter()
        .any(|entry| entry.property_names.as_ref().is_none_or(Vec::is_empty) && entry.relationship_names.as_ref().is_none_or(Vec::is_empty));
    if unrestricted {
        return AttributeCoverage::Unrestricted;
    }
    AttributeCoverage::Names(
        information
            .iter()
            .flat_map(|entry| entry.property_names.iter().chain(&entry.relationship_names).flatten())
            .flat_map(|name| [Some(name.clone()), compact_attribute_name(name)]) // clone: the coverage owns names borrowed from the registration
            .flatten()
            .collect(),
    )
}

/// Narrow an entity object to the attributes `csr` covers for it, keeping the
/// entity control members (clauses 5.6.7.4 and following: "remove
/// non-matching Attributes from the remaining Entities"). Returns whether any
/// attribute remains.
pub(crate) fn retain_covered_attributes(
    object: &mut Map<String, Value>,
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
) -> bool {
    match attribute_coverage(csr, entity_type, entity_id) {
        AttributeCoverage::NotMatching => object.retain(|name, _value| is_entity_control_member(name)),
        AttributeCoverage::Unrestricted => {}
        AttributeCoverage::Names(covered) => {
            object.retain(|name, _value| is_entity_control_member(name) || covered.iter().any(|covered_name| covered_name.as_str() == name));
        }
    }
    object.keys().any(|name| !is_entity_control_member(name))
}

/// The attribute members of `object` that `csr` covers for the entity: what
/// another registration's copy loses when `csr` owns them exclusively
/// (clauses 5.6.7.4 and following).
pub(crate) fn covered_attribute_members(
    object: &Map<String, Value>,
    csr: &ContextSourceRegistration,
    entity_type: Option<&IriBuf>,
    entity_id: Option<&Uri>,
) -> Vec<String> {
    let attributes = object.keys().filter(|name| !is_entity_control_member(name));
    match attribute_coverage(csr, entity_type, entity_id) {
        AttributeCoverage::NotMatching => Vec::new(),
        AttributeCoverage::Unrestricted => attributes.cloned().collect(), // clone: the result owns the member names it reports
        AttributeCoverage::Names(covered) => attributes
            .filter(|name| covered.iter().any(|covered_name| covered_name.as_str() == name.as_str()))
            .cloned() // clone: same
            .collect(),
    }
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

/// Whether a registration's declared scope can answer a scoped read.
pub(crate) fn registration_scope_admits(registration: &ContextSourceRegistration, scope_q: Option<&ScopeQuery>) -> bool {
    let Some(scope_q) = scope_q else {
        return true;
    };
    registration
        .coverage
        .scope
        .as_ref()
        .is_none_or(|scopes| scope_q.matches(&scopes.iter().map(ScopeBuf::as_str).collect::<Vec<_>>()))
}

/// Whether the entity selector axis of one information entry matches.
pub(crate) fn information_matches_entity(information: &RegistrationInfo, entity_type: Option<&IriBuf>, entity_id: Option<&Uri>) -> bool {
    let Some(selectors) = information.entities.as_deref().filter(|selectors| !selectors.is_empty()) else {
        return true;
    };
    selectors.iter().any(|selector| {
        let type_matches = entity_type.is_none_or(|requested| selector.type_.admits(requested.as_str()));
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

#[cfg(test)]
mod tests {
    use crate::registration_selection::{covered_attribute_members, retain_covered_attributes};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use iri_rs::IriBuf;
    use serde_json::{Map, Value, json};

    fn registration(information: &Value) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:csr:1",
            "type": "ContextSourceRegistration",
            "endpoint": "https://peer.example.org",
            "information": [information],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("valid registration")
    }

    fn vehicle() -> Map<String, Value> {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "https://example.org/brandName": {"type": "Property", "value": "x"}
        })
        .as_object()
        .expect("object")
        .clone() // clone: each case mutates its own copy
    }

    fn vehicle_type() -> IriBuf {
        IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI")
    }

    fn vehicle_id() -> Uri {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")
    }

    #[test]
    fn a_registration_naming_attributes_keeps_them_in_either_spelling_and_the_control_members() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/speed", "https://example.org/brandName"]
        }));
        let mut object = vehicle();
        assert!(retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id())));
        let mut kept = object.keys().cloned().collect::<Vec<_>>();
        kept.sort();
        pretty_assertions::assert_eq!(kept, vec!["https://example.org/brandName", "id", "speed", "type"]);
        let mut covered = covered_attribute_members(&vehicle(), &csr, Some(&vehicle_type()), Some(&vehicle_id()));
        covered.sort();
        pretty_assertions::assert_eq!(covered, vec!["https://example.org/brandName", "speed"]);
    }

    #[test]
    fn a_registration_declaring_no_attribute_names_covers_every_attribute() {
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let mut object = vehicle();
        assert!(retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id())));
        pretty_assertions::assert_eq!(object.len(), 4);
        pretty_assertions::assert_eq!(covered_attribute_members(&vehicle(), &csr, Some(&vehicle_type()), Some(&vehicle_id())).len(), 2);
    }

    #[test]
    fn a_registration_not_matching_the_entity_keeps_nothing() {
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Device"}]}));
        let mut object = vehicle();
        assert!(!retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id())));
        let mut kept = object.keys().cloned().collect::<Vec<_>>();
        kept.sort();
        pretty_assertions::assert_eq!(kept, vec!["id", "type"]);
        assert_eq!(
            covered_attribute_members(&vehicle(), &csr, Some(&vehicle_type()), Some(&vehicle_id())),
            [] as [String; 0]
        );
    }

    #[test]
    fn a_registration_naming_only_absent_attributes_leaves_no_attribute() {
        let csr = registration(&json!({
            "entities": [{"type": "https://example.org/Vehicle"}],
            "propertyNames": ["https://example.org/colour"]
        }));
        let mut object = vehicle();
        assert!(!retain_covered_attributes(&mut object, &csr, Some(&vehicle_type()), Some(&vehicle_id())));
    }
}
