//! Registration-information matching (clause 5.12).
//!
//! A registration stores the type and attribute names it covers expanded (as
//! IRIs), so every comparison here is one of IRIs: the entity axis of an
//! information entry ([`information_matches_entity`]) and the attribute axis
//! ([`information_covers_attribute`]). The attributes of a write body that a
//! registration covers are decided in [`crate::attribute_coverage`].

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, info::RegistrationInfo};
use andromeda_ngsi_ld_query::scope::query::ScopeQuery;
use iri_rs::IriBuf;

/// Whether one information entry covers the entity and requested attribute. The
/// attribute is expanded to its IRI by the caller so the comparison is exact.
pub(crate) fn csr_has_matching_information(
    csr: &ContextSourceRegistration,
    entity_types: &[IriBuf],
    entity_id: Option<&Identifier>,
    attribute: Option<&IriBuf>,
) -> bool {
    csr.coverage
        .information
        .iter()
        .any(|information| information_matches_entity(information, entity_types, entity_id) && information_covers_attribute(information, attribute))
}

/// Whether a registration's declared scope can answer a scoped read.
pub(crate) fn registration_scope_admits(registration: &ContextSourceRegistration, scope_q: Option<&ScopeQuery>) -> bool {
    let Some(scope_q) = scope_q else {
        return true;
    };
    registration.coverage.scope.as_ref().is_none_or(|scopes| scope_q.matches(scopes))
}

/// Whether the entity selector axis of one information entry matches.
pub(crate) fn information_matches_entity(information: &RegistrationInfo, entity_types: &[IriBuf], entity_id: Option<&Identifier>) -> bool {
    let Some(selectors) = information.entities.as_deref().filter(|selectors| !selectors.is_empty()) else {
        return true;
    };
    selectors.iter().any(|selector| {
        // Clause 5.12: an Entity specification matches an EntityInfo only if the
        // type selector matches the entity's types. An Entity may hold several
        // (clauses 4.5.5, 5.2.4), so the selector admitting ANY of them is a
        // match. An empty set places no type constraint -- an id-only read, or an
        // entity whose type this caller does not resolve -- and matches on id.
        let type_matches = entity_types.is_empty() || entity_types.iter().any(|requested| selector.type_.admits(requested.as_str()));
        let id_matches = entity_id.is_none_or(|requested| match (&selector.id, &selector.id_pattern) {
            (None, None) => true,
            (Some(id), None) => id.as_str() == requested.as_str(),
            (None, Some(pattern)) => pattern.is_match(requested.as_str()),
            (Some(id), Some(pattern)) => id.as_str() == requested.as_str() || pattern.is_match(requested.as_str()),
        });
        type_matches && id_matches
    })
}

/// Whether an attribute is declared by the same information entry. The declared
/// names are stored expanded, so the pre-expanded attribute IRI is compared
/// against them directly.
pub(crate) fn information_covers_attribute(information: &RegistrationInfo, attribute: Option<&IriBuf>) -> bool {
    let Some(attribute) = attribute else {
        return true;
    };
    let names = information.property_names.iter().chain(&information.relationship_names).flatten();
    let mut saw_name = false;
    for name in names {
        saw_name = true;
        if name.as_str() == attribute.as_str() {
            return true;
        }
    }
    !saw_name
}

#[cfg(test)]
mod tests {
    use crate::registration_information::information_matches_entity;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

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

    fn vehicle_id() -> Identifier {
        "urn:ngsi-ld:Vehicle:1".parse().expect("valid URI")
    }

    /// An absent entity type satisfies a type-scoped information entry: the
    /// read/purge-by-id paths pass no type on purpose, so a registration scoped
    /// to a type still covers an id-only read. The write path must therefore
    /// resolve the entity's actual type (see `crate::local_entity_type`), or this
    /// permissive match refuses writes to unrelated entities.
    #[test]
    fn an_absent_entity_type_satisfies_a_type_scoped_information_entry() {
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let info = &csr.coverage.information[0];
        assert!(information_matches_entity(info, &[], Some(&vehicle_id())));
    }

    /// A known entity type that differs from the one a registration covers does
    /// not match it. This is exactly what supplying the resolved local type buys
    /// the write path: a registration scoped to another type stops covering the
    /// write, so it is no longer wrongly refused.
    #[test]
    fn a_differing_known_entity_type_does_not_match_a_type_scoped_information_entry() {
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Device"}]}));
        let info = &csr.coverage.information[0];
        let building = IriBuf::new("https://example.org/Building".to_string()).expect("valid IRI");
        assert!(!information_matches_entity(info, &[building], Some(&vehicle_id())));
    }

    /// Clause 5.12: the type selector matches the entity's *types* (plural). A
    /// registration covering one of an entity's several types matches it, even
    /// when that type is not the first one the entity carries.
    #[test]
    fn a_registration_covering_one_of_several_entity_types_matches() {
        let device = IriBuf::new("https://example.org/Device".to_string()).expect("valid IRI");
        let vehicle = IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI");
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Vehicle"}]}));
        let info = &csr.coverage.information[0];
        // The entity is a Device first, a Vehicle second; the Vehicle registration
        // must still match it.
        assert!(information_matches_entity(info, &[device.clone(), vehicle.clone()], Some(&vehicle_id())));
        // Order must not matter.
        assert!(information_matches_entity(info, &[vehicle, device], Some(&vehicle_id())));
    }

    /// A registration covering none of an entity's types does not match it, no
    /// matter how many types the entity carries.
    #[test]
    fn a_registration_covering_none_of_several_entity_types_does_not_match() {
        let device = IriBuf::new("https://example.org/Device".to_string()).expect("valid IRI");
        let vehicle = IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI");
        let csr = registration(&json!({"entities": [{"type": "https://example.org/Building"}]}));
        let info = &csr.coverage.information[0];
        assert!(!information_matches_entity(info, &[device, vehicle], Some(&vehicle_id())));
    }

    /// A registration whose `EntityInfo` selects several types matches an entity
    /// that carries any one of them.
    #[test]
    fn a_multi_type_registration_selector_matches_an_entity_sharing_one_type() {
        let device = IriBuf::new("https://example.org/Device".to_string()).expect("valid IRI");
        let csr = registration(&json!({"entities": [{"type": ["https://example.org/Vehicle", "https://example.org/Device"]}]}));
        let info = &csr.coverage.information[0];
        assert!(information_matches_entity(info, &[device], Some(&vehicle_id())));
        let building = IriBuf::new("https://example.org/Building".to_string()).expect("valid IRI");
        assert!(!information_matches_entity(info, &[building], Some(&vehicle_id())));
    }
}
