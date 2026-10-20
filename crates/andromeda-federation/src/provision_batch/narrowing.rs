//! Narrowing the client's array to what one registration receives (the `IN`
//! of clauses 5.6.7.4 and following).

use crate::{
    attribute_coverage::{covered_attributes, narrowed_document},
    provision::is_proxied,
    provision_batch::BatchEntity,
    registration_information::csr_has_matching_information,
};
use andromeda_jsonld::batch_contexts::BatchContext;
use andromeda_ngsi_ld_entity::document::EntityDocument;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_model::registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode};
use iri_rs::IriBuf;
use std::borrow::Cow;

/// One entity of the array narrowed for one registration.
pub(super) struct NarrowedEntity<'a> {
    /// The entity identifier.
    pub(super) id: Identifier,
    /// The narrowed document, borrowed as admitted when nothing was taken out
    /// of it; none for a deletion.
    pub(super) body: Option<Cow<'a, EntityDocument>>,
    /// The attributes a proxied registration claims from it, by their expanded
    /// names.
    pub(super) proxied_attrs: Vec<AttributeName>,
    /// The context the entity was submitted with, which it is forwarded under.
    pub(super) context: &'a BatchContext,
}

/// The types of an admitted document, as the IRIs registrations are matched
/// on (clause 5.12 matches on any of them).
fn entity_types(entity: &BatchEntity<'_>) -> Vec<IriBuf> {
    entity
        .document
        .into_iter()
        .flat_map(EntityDocument::types)
        .filter_map(|name| IriBuf::new(name.to_string()).ok())
        .collect()
}

/// Narrow the client's array to what `csr` receives (the `IN` of clauses
/// 5.6.7.4 and following): the entities it matches, each reduced to the
/// attributes it covers minus those another exclusive registration -- or,
/// unless `csr` is itself a redirect, another redirect registration -- owns,
/// and without the entities that keeps no attribute. Every entity is read
/// under the expanded names its own context gave it.
pub(super) fn narrow_input<'a>(
    csr: &ContextSourceRegistration,
    candidates: &[ContextSourceRegistration],
    entities: &[BatchEntity<'a>],
) -> Vec<NarrowedEntity<'a>> {
    let csr_is_redirect = csr.access.mode == Some(ForwardingMode::Redirect);
    let mut narrowed = Vec::new();
    for entity in entities {
        let types = entity_types(entity);
        if !csr_has_matching_information(csr, &types, Some(entity.id), None) {
            continue;
        }
        let Some(document) = entity.document else {
            narrowed.push(NarrowedEntity {
                id: entity.id.clone(), // clone: the narrowed entity outlives the borrowed array
                body: None,
                proxied_attrs: Vec::new(),
                context: entity.context,
            });
            continue;
        };
        let Some(mut body) = narrowed_document(document, csr, &types, Some(entity.id)) else {
            continue;
        };
        for other in candidates.iter().filter(|other| other.id != csr.id) {
            let owns = match other.access.mode {
                Some(ForwardingMode::Exclusive) => true,
                Some(ForwardingMode::Redirect) => !csr_is_redirect,
                Some(ForwardingMode::Inclusive | ForwardingMode::Auxiliary) | None => false,
            };
            if !owns {
                continue;
            }
            let taken = covered_attributes(&body, other, &types, Some(entity.id));
            if !taken.is_empty() {
                body.to_mut().attributes_mut().retain(|name, _| !taken.contains(name));
            }
        }
        if body.attributes().is_empty() {
            continue;
        }
        let proxied_attrs = if is_proxied(csr) {
            body.attributes().keys().cloned().collect() // clone: the result owns the names the registration claims
        } else {
            Vec::new()
        };
        narrowed.push(NarrowedEntity {
            id: entity.id.clone(), // clone: the narrowed entity outlives the borrowed array
            body: Some(body),
            proxied_attrs,
            context: entity.context,
        });
    }
    narrowed
}

#[cfg(test)]
mod tests {
    use crate::provision_batch::{
        BatchEntity,
        narrowing::{NarrowedEntity, narrow_input},
        registration_fixture::{document, registration, uri},
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{
        batch_contexts::{BatchContext, BatchContexts},
        context_spec::ContextSpec,
    };
    use serde_json::json;
    use std::{borrow::Cow, slice::from_ref};

    const VEHICLE_TYPE: &str = "https://example.org/Vehicle";
    const SPEED: &str = "https://example.org/speed";
    const PLACE: &str = "https://example.org/place";
    const BRAND: &str = "https://example.org/brandName";

    /// Names of the attributes a narrowed entity keeps, in written order.
    fn kept(entity: &NarrowedEntity<'_>) -> Vec<String> {
        entity
            .body
            .as_ref()
            .map(|body| body.attributes().keys().map(ToString::to_string).collect())
            .unwrap_or_default()
    }

    #[test]
    fn each_registration_receives_its_own_narrowed_copy_and_loses_what_other_proxied_sources_own() {
        // Clause 5.6.7.4: per registration, unmatched entities are dropped,
        // non-matching attributes are removed, attributes another exclusive or
        // redirect registration owns are removed, and an entity left without
        // attributes is dropped.
        let speed = registration("urn:ngsi-ld:csr:speed", "redirect", Some(VEHICLE_TYPE), None, &[SPEED], &["createBatch"]);
        let location = registration("urn:ngsi-ld:csr:location", "redirect", Some(VEHICLE_TYPE), None, &[PLACE], &["createBatch"]);
        let everything = registration("urn:ngsi-ld:csr:all", "inclusive", Some(VEHICLE_TYPE), None, &[], &["createBatch"]);
        let candidates = vec![speed.clone(), location, everything.clone()]; // clone: the candidate list and the subjects both own the registrations
        let context = BatchContexts::per_entity().context_of(None);
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let d1 = uri("urn:ngsi-ld:Device:1");
        let vehicle = document("urn:ngsi-ld:Vehicle:1", VEHICLE_TYPE, &[BRAND, SPEED, PLACE]);
        let device = document("urn:ngsi-ld:Device:1", "https://example.org/Device", &[BRAND]);
        let entities = [
            BatchEntity {
                id: &v1,
                document: Some(&vehicle),
                context: &context,
            },
            BatchEntity {
                id: &d1,
                document: Some(&device),
                context: &context,
            },
        ];

        let for_speed = narrow_input(&speed, &candidates, &entities);
        pretty_assertions::assert_eq!(for_speed.len(), 1, "the Device is not matched");
        pretty_assertions::assert_eq!(kept(&for_speed[0]), vec![SPEED]);
        pretty_assertions::assert_eq!(for_speed[0].proxied_attrs.iter().map(ToString::to_string).collect::<Vec<_>>(), vec![SPEED]);

        let for_everything = narrow_input(&everything, &candidates, &entities);
        pretty_assertions::assert_eq!(kept(&for_everything[0]), vec![BRAND], "the redirect-owned attributes leave the inclusive copy");
        assert!(for_everything[0].proxied_attrs.is_empty(), "an inclusive registration proxies nothing");

        let colour = registration(
            "urn:ngsi-ld:csr:colour",
            "redirect",
            Some(VEHICLE_TYPE),
            None,
            &["https://example.org/colour"],
            &["createBatch"],
        );
        assert!(
            narrow_input(&colour, &candidates, &entities).is_empty(),
            "a source owning nothing present is skipped"
        );
    }

    /// JSON-LD applies a top-level node's `@context` to that node alone: two
    /// entities naming `rate` through different contexts were expanded to
    /// different IRIs, so only the one whose own `rate` is the registered
    /// speed is covered, and it keeps the context it came with.
    #[test]
    fn each_entity_is_narrowed_by_what_its_own_context_expanded_it_to() {
        let speed = registration("urn:ngsi-ld:csr:speed", "inclusive", Some(VEHICLE_TYPE), None, &[SPEED], &["createBatch"]);
        let mut contexts = BatchContexts::per_entity();
        let registered = contexts.context_of(Some(JsonLiteral::admit(&json!({"rate": SPEED}).to_string()).expect("an admissible literal")));
        let other = contexts.context_of(Some(
            JsonLiteral::admit(&json!({"rate": "https://other.example.org/rate"}).to_string()).expect("an admissible literal"),
        ));
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let v2 = uri("urn:ngsi-ld:Vehicle:2");
        let first = document("urn:ngsi-ld:Vehicle:1", VEHICLE_TYPE, &[SPEED]);
        let second = document("urn:ngsi-ld:Vehicle:2", VEHICLE_TYPE, &["https://other.example.org/rate"]);
        let entities = [
            BatchEntity {
                id: &v1,
                document: Some(&first),
                context: &registered,
            },
            BatchEntity {
                id: &v2,
                document: Some(&second),
                context: &other,
            },
        ];

        let narrowed = narrow_input(&speed, from_ref(&speed), &entities);

        pretty_assertions::assert_eq!(
            narrowed.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1"],
            "only the entity whose own rate is the registered speed is covered"
        );
        pretty_assertions::assert_eq!(narrowed[0].context.id(), registered.id());
    }

    #[test]
    fn a_deletion_narrows_by_identifier_alone() {
        // A deletion names no type, so the registration's type selector is
        // not consulted and only its identifier narrows the array.
        let owner = registration(
            "urn:ngsi-ld:csr:v1",
            "redirect",
            Some(VEHICLE_TYPE),
            Some("urn:ngsi-ld:Vehicle:1"),
            &[],
            &["deleteBatch"],
        );
        let context = BatchContext::shared(ContextSpec::none());
        let v1 = uri("urn:ngsi-ld:Vehicle:1");
        let v2 = uri("urn:ngsi-ld:Vehicle:2");
        let entities = [
            BatchEntity {
                id: &v1,
                document: None,
                context: &context,
            },
            BatchEntity {
                id: &v2,
                document: None,
                context: &context,
            },
        ];
        let narrowed = narrow_input(&owner, from_ref(&owner), &entities);
        pretty_assertions::assert_eq!(
            narrowed.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Vehicle:1"]
        );
    }

    /// An entity no registration matches is not forwarded, and one a
    /// registration covers in full is not copied for it.
    #[test]
    fn an_entity_no_registration_matches_is_dropped_and_a_covered_one_is_not_copied() {
        let devices = registration(
            "urn:ngsi-ld:csr:devices",
            "inclusive",
            Some("https://example.org/Device"),
            None,
            &[],
            &["createBatch"],
        );
        let context = BatchContext::shared(ContextSpec::none());
        let vehicle_id = uri("urn:ngsi-ld:Vehicle:1");
        let device_id = uri("urn:ngsi-ld:Device:1");
        let vehicle = document("urn:ngsi-ld:Vehicle:1", VEHICLE_TYPE, &[SPEED]);
        let device = document("urn:ngsi-ld:Device:1", "https://example.org/Device", &[BRAND]);
        let entities = [
            BatchEntity {
                id: &vehicle_id,
                document: Some(&vehicle),
                context: &context,
            },
            BatchEntity {
                id: &device_id,
                document: Some(&device),
                context: &context,
            },
        ];

        let narrowed = narrow_input(&devices, from_ref(&devices), &entities);

        pretty_assertions::assert_eq!(
            narrowed.iter().map(|entity| entity.id.to_string()).collect::<Vec<_>>(),
            vec!["urn:ngsi-ld:Device:1"]
        );
        assert!(
            matches!(narrowed[0].body, Some(Cow::Borrowed(_))),
            "a document that loses nothing is not copied"
        );
    }
}
