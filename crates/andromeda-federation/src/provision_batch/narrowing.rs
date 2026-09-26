//! Narrowing the client's array to what one registration receives (the `IN`
//! of clauses 5.6.7.4 and following).

use crate::{
    error::FederationError,
    provision::is_proxied,
    provision_batch::BatchEntity,
    registration_selection::{covered_attribute_members, csr_has_matching_information, is_entity_control_member, retain_covered_attributes},
    term_resolver::TermResolver,
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, uri::Uri};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use iri_rs::IriBuf;
use serde_json::Value;
use std::{collections::HashMap, slice::from_ref};

/// One entity of the array narrowed for one registration.
pub(super) struct NarrowedEntity {
    /// The entity identifier.
    pub(super) id: Uri,
    /// The narrowed entity; none for a deletion.
    pub(super) body: Option<Value>,
    /// The attributes a proxied registration claims from it.
    pub(super) proxied_attrs: Vec<AttributeName>,
}

/// Narrow the client's array to what `csr` receives (the `IN` of clauses
/// 5.6.7.4 and following): the entities it matches, each reduced to the
/// attributes it covers minus those another exclusive registration -- or,
/// unless `csr` is itself a redirect, another redirect registration -- owns,
/// and without the entities that keeps no attribute.
///
/// # Errors
///
/// Returns [`FederationError::JsonLd`] when a body attribute cannot be expanded
/// against the request context, and [`FederationError::ForwardedBody`] when an
/// entity the registration matches cannot be serialised.
pub(super) async fn narrow_input(
    csr: &ContextSourceRegistration,
    candidates: &[ContextSourceRegistration],
    entities: &[BatchEntity<'_>],
    expanded_types: &HashMap<&str, IriBuf>,
    resolver: &TermResolver<'_>,
) -> Result<Vec<NarrowedEntity>, FederationError> {
    let csr_is_redirect = csr.access.mode == Some(ForwardingMode::Redirect);
    let mut narrowed = Vec::new();
    for entity in entities {
        // Clause 5.12 matches on any of the entity's types; a batch entity names
        // at most one, so its resolved type (when known) is the whole set.
        let entity_type = entity.entity_type.and_then(|name| expanded_types.get(name.as_str()));
        let entity_types: &[IriBuf] = entity_type.map_or(&[], from_ref);
        if !csr_has_matching_information(csr, entity_types, Some(entity.id), None) {
            continue;
        }
        let Some(body) = entity.body else {
            narrowed.push(NarrowedEntity {
                id: entity.id.clone(), // clone: the narrowed entity outlives the borrowed array
                body: None,
                proxied_attrs: Vec::new(),
            });
            continue;
        };
        // Each registration narrows its own copy of the entity.
        let Value::Object(mut object) = body.to_json_value().map_err(FederationError::ForwardedBody)? else {
            continue;
        };
        if !retain_covered_attributes(&mut object, csr, entity_types, Some(entity.id), resolver).await? {
            continue;
        }
        for other in candidates.iter().filter(|other| other.id != csr.id) {
            let owns = match other.access.mode {
                Some(ForwardingMode::Exclusive) => true,
                Some(ForwardingMode::Redirect) => !csr_is_redirect,
                Some(ForwardingMode::Inclusive | ForwardingMode::Auxiliary) | None => false,
            };
            if !owns {
                continue;
            }
            for member in covered_attribute_members(&object, other, entity_types, Some(entity.id), resolver).await? {
                object.remove(&member);
            }
        }
        let remaining = object
            .keys()
            .filter(|name| !is_entity_control_member(name))
            .filter_map(|name| name.parse::<AttributeName>().ok())
            .collect::<Vec<_>>();
        if remaining.is_empty() {
            continue;
        }
        narrowed.push(NarrowedEntity {
            id: entity.id.clone(), // clone: the narrowed entity outlives the borrowed array
            body: Some(Value::Object(object)),
            proxied_attrs: if is_proxied(csr) { remaining } else { Vec::new() },
        });
    }
    Ok(narrowed)
}

#[cfg(test)]
mod tests {
    use crate::{
        counted_body::CountedBody,
        provision_batch::{
            BatchEntity,
            narrowing::narrow_input,
            registration_fixture::{offline_engine, registration, uri, vehicle},
        },
        term_resolver::TermResolver,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
    use iri_rs::IriBuf;
    use serde_json::json;
    use std::{collections::HashMap, slice::from_ref};

    #[tokio::test]
    async fn each_registration_receives_its_own_narrowed_copy_and_loses_what_other_proxied_sources_own() {
        // Clause 5.6.7.4: per registration, unmatched entities are dropped,
        // non-matching attributes are removed, attributes another exclusive or
        // redirect registration owns are removed, and an entity left without
        // attributes is dropped.
        let speed = registration(
            "urn:ngsi-ld:csr:speed",
            "redirect",
            Some("https://example.org/Vehicle"),
            None,
            &["https://example.org/speed"],
            &["createBatch"],
        );
        let location = registration(
            "urn:ngsi-ld:csr:location",
            "redirect",
            Some("https://example.org/Vehicle"),
            None,
            &["https://example.org/place"],
            &["createBatch"],
        );
        let everything = registration(
            "urn:ngsi-ld:csr:all",
            "inclusive",
            Some("https://example.org/Vehicle"),
            None,
            &[],
            &["createBatch"],
        );
        let candidates = vec![speed.clone(), location, everything.clone()]; // clone: the candidate list and the subjects both own the registrations
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let d1 = uri("urn:ngsi-ld:Device:1");
        let vehicle_type = "Vehicle".parse::<EntityTypeName>().expect("valid type");
        let device_type = "Device".parse::<EntityTypeName>().expect("valid type");
        let vehicle_body = vehicle("urn:ngsi-ld:Vehicle:1");
        let device_body = json!({"id": "urn:ngsi-ld:Device:1", "type": "Device", "brandName": {"type": "Property", "value": "sensor"}});
        let entities = [
            BatchEntity {
                id: &v1,
                entity_type: Some(&vehicle_type),
                body: Some(&vehicle_body),
            },
            BatchEntity {
                id: &d1,
                entity_type: Some(&device_type),
                body: Some(&device_body),
            },
        ];
        let expanded_types = HashMap::from([
            ("Vehicle", IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI")),
            ("Device", IriBuf::new("https://example.org/Device".to_string()).expect("valid IRI")),
        ]);
        // The bodies name their attributes by the terms this @context maps to
        // the declared IRIs, so coverage is decided by what each key means.
        let engine = offline_engine();
        // The bodies name their attributes by the terms this @context maps to
        // the declared IRIs, so coverage is decided by what each key means.
        // `place` stands in for a plain vocabulary term; the core context
        // protects `location`, so it could not be aliased to a peer IRI.
        let context = ContextSpec::none().with_body_context(Some(json!({
            "speed": "https://example.org/speed",
            "place": "https://example.org/place",
            "brandName": "https://example.org/brandName",
            "colour": "https://example.org/colour"
        })));
        let resolver = TermResolver::new(&engine, &context);

        let for_speed = narrow_input(&speed, &candidates, &entities, &expanded_types, &resolver)
            .await
            .expect("narrowing resolves");
        pretty_assertions::assert_eq!(for_speed.len(), 1, "the Device is not matched");
        let body = for_speed[0].body.as_ref().expect("a narrowed body");
        assert!(
            body.get("speed").is_some() && body.get("place").is_none() && body.get("brandName").is_none(),
            "{body}"
        );
        pretty_assertions::assert_eq!(for_speed[0].proxied_attrs.iter().map(ToString::to_string).collect::<Vec<_>>(), vec!["speed"]);

        let for_everything = narrow_input(&everything, &candidates, &entities, &expanded_types, &resolver)
            .await
            .expect("narrowing resolves");
        let body = for_everything[0].body.as_ref().expect("a narrowed body");
        assert!(body.get("brandName").is_some(), "the unowned attribute stays: {body}");
        assert!(
            body.get("speed").is_none() && body.get("place").is_none(),
            "the redirect-owned attributes leave the inclusive copy: {body}"
        );
        assert!(for_everything[0].proxied_attrs.is_empty(), "an inclusive registration proxies nothing");

        let colour = registration(
            "urn:ngsi-ld:csr:colour",
            "redirect",
            Some("https://example.org/Vehicle"),
            None,
            &["https://example.org/colour"],
            &["createBatch"],
        );
        assert!(
            narrow_input(&colour, &candidates, &entities, &expanded_types, &resolver)
                .await
                .expect("narrowing resolves")
                .is_empty(),
            "a source owning nothing present is skipped"
        );
    }

    #[tokio::test]
    async fn a_deletion_narrows_by_identifier_alone() {
        // A deletion names no type, so the registration's type selector is
        // not consulted and only its identifier narrows the array.
        let owner = registration(
            "urn:ngsi-ld:csr:v1",
            "redirect",
            Some("https://example.org/Vehicle"),
            Some("urn:ngsi-ld:Vehicle:1"),
            &[],
            &["deleteBatch"],
        );
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let v2 = uri("urn:ngsi-ld:Vehicle:2");
        let entities = [
            BatchEntity {
                id: &v1,
                entity_type: None,
                body: None,
            },
            BatchEntity {
                id: &v2,
                entity_type: None,
                body: None,
            },
        ];
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);
        let narrowed = narrow_input(&owner, from_ref(&owner), &entities, &HashMap::new(), &resolver)
            .await
            .expect("narrowing resolves");
        pretty_assertions::assert_eq!(
            narrowed.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1"]
        );
    }

    /// An entity is serialised only for a registration that matches it: a
    /// batch no registration covers serialises nothing (clause 5.12).
    #[tokio::test]
    async fn an_entity_no_registration_matches_is_never_serialised() {
        let devices = registration(
            "urn:ngsi-ld:csr:devices",
            "inclusive",
            Some("https://example.org/Device"),
            None,
            &[],
            &["createBatch"],
        );
        let vehicle_id = uri("urn:ngsi-ld:Vehicle:1");
        let device_id = uri("urn:ngsi-ld:Device:1");
        let vehicle_type = "Vehicle".parse::<EntityTypeName>().expect("valid type");
        let device_type = "Device".parse::<EntityTypeName>().expect("valid type");
        let vehicle_body = CountedBody::new(vehicle("urn:ngsi-ld:Vehicle:1"));
        let device_body = CountedBody::new(json!({"id": "urn:ngsi-ld:Device:1", "type": "Device", "brandName": {"type": "Property", "value": "sensor"}}));
        let entities = [
            BatchEntity {
                id: &vehicle_id,
                entity_type: Some(&vehicle_type),
                body: Some(&vehicle_body),
            },
            BatchEntity {
                id: &device_id,
                entity_type: Some(&device_type),
                body: Some(&device_body),
            },
        ];
        let expanded_types = HashMap::from([
            ("Vehicle", IriBuf::new("https://example.org/Vehicle".to_string()).expect("valid IRI")),
            ("Device", IriBuf::new("https://example.org/Device".to_string()).expect("valid IRI")),
        ]);
        let engine = offline_engine();
        let context = ContextSpec::none();
        let resolver = TermResolver::new(&engine, &context);

        let narrowed = narrow_input(&devices, from_ref(&devices), &entities, &expanded_types, &resolver)
            .await
            .expect("the array narrows");

        pretty_assertions::assert_eq!(
            narrowed.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Device:1"]
        );
        pretty_assertions::assert_eq!(vehicle_body.serialisations(), 0, "the unmatched entity is never serialised");
        pretty_assertions::assert_eq!(device_body.serialisations(), 1, "the matched entity is serialised once for its registration");
    }
}
