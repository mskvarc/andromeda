//! A write's `type` selector decides which registrations cover it (ETSI GS CIM
//! 009 clause 5.12): "an Entity specification consisting of selector of Entity
//! Types, Entity identifiers and id pattern matches an EntityInfo element of the
//! RegistrationInfo if the type selector matches the entity types in the
//! EntityInfo element" and an identifier condition holds.

use crate::{
    FederationService,
    fixed_registration_store::{offline_service, service_holding_entity_of},
    operation::{ForwardingOperation, ProvisionResource},
    request::{ForwardProvisionRequest, TriggeringRequest},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::type_selection::grammar::TypeExprRaw;
use iri_rs::IriBuf;
use serde_json::json;

const ENTITY_ID: &str = "urn:ngsi-ld:Vehicle:1";
const VEHICLE: &str = "https://example.org/Vehicle";
const BUILDING: &str = "https://example.org/Building";

/// An exclusive registration of the fixture Vehicle that advertises no write,
/// so a write it covers is claimed by it without contacting it.
fn vehicle_registration() -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:csr:vehicle",
        "type": "ContextSourceRegistration",
        "endpoint": "https://peer.example.org",
        "mode": "exclusive",
        "information": [{"entities": [{"id": ENTITY_ID, "type": VEHICLE}]}],
        "operations": ["retrieveEntity"],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("valid registration")
}

/// Whether a registration covers a Delete Entity of the fixture entity named
/// with `selector` (or with no selector at all).
async fn covers_the_write(service: &FederationService, selector: Option<&str>) -> bool {
    let selector: Option<TypeExprRaw> = selector.map(|raw| raw.parse().expect("a clause 4.17 selector"));
    let entity_id: Identifier = ENTITY_ID.parse().expect("valid identifier");
    let context = ContextSpec::none();
    let request = ForwardProvisionRequest {
        resource: ProvisionResource::Entities,
        entity_id: &entity_id,
        entity_type_selector: selector.as_ref(),
        attribute_name: None,
        method: reqwest::Method::DELETE,
        context: &context,
        operation: ForwardingOperation::DeleteEntity,
        path_suffix: None,
        query_string: None,
        triggering: TriggeringRequest { via: None, headers: None },
    };
    service
        .plan_provision(&ContextTarget::Live, &request)
        .await
        .expect("the plan is made")
        .reads_payload()
}

/// Clause 5.12: a selector naming a type the registration does not hold leaves
/// the write uncovered, though the registration covers the entity identifier.
#[tokio::test]
async fn a_write_selector_excludes_a_registration_of_another_type() {
    let service = offline_service(vec![vehicle_registration()]);
    assert!(!covers_the_write(&service, Some(BUILDING)).await);
    assert!(covers_the_write(&service, Some(VEHICLE)).await);
}

/// Clause 4.17: a selector matches when any of its alternatives names a
/// registered type, and a conjunction needs every one of them registered.
#[tokio::test]
async fn a_write_selector_matches_any_of_its_alternatives() {
    let service = offline_service(vec![vehicle_registration()]);
    assert!(covers_the_write(&service, Some(&format!("{BUILDING}|{VEHICLE}"))).await);
    assert!(covers_the_write(&service, Some(&format!("{VEHICLE},{BUILDING}"))).await);
    assert!(!covers_the_write(&service, Some(&format!("({VEHICLE};{BUILDING})"))).await);
}

/// A write that names no selector is matched by the types of the entity this
/// broker holds, so a registration of an unrelated type does not cover it.
#[tokio::test]
async fn a_write_without_a_selector_still_matches_by_the_local_entity_types() {
    let building: IriBuf = BUILDING.parse().expect("an IRI");
    let vehicle: IriBuf = VEHICLE.parse().expect("an IRI");
    let held_as_building = service_holding_entity_of(vec![vehicle_registration()], vec![building]);
    assert!(!covers_the_write(&held_as_building, None).await);
    let held_as_vehicle = service_holding_entity_of(vec![vehicle_registration()], vec![vehicle]);
    assert!(covers_the_write(&held_as_vehicle, None).await);
    let held_nowhere = offline_service(vec![vehicle_registration()]);
    assert!(
        covers_the_write(&held_nowhere, None).await,
        "an entity held only at a source falls back to the identifier"
    );
}
