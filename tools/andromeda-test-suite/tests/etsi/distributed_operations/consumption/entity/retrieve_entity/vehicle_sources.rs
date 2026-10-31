//! The context sources the retrieval cases of the Vehicle `urn:ngsi-ld:Vehicle:<ns>-1` set up in the default tenant:
//! two redirect sources each holding a part of it, or one exclusive source of its `speed`.

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{
            create_entity_locally,
            register,
            serve,
            source_endpoint,
            stub,
            unregister,
            vehicle_registration,
            vehicle_reply,
            vehicle_retrieval_rule,
        },
    },
    entity_operations::delete_entities,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker::BrokerClient, cleanup::Cleanup, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::registration::RegistrationMode;
use serde_json::json;

/// The terms of the `@context` the redirect cases register.
const REDIRECT_TERMS: [&str; 5] = ["Vehicle", "brandName", "isParked", "providedBy", "isParked2"];

/// The terms of the `@context` the exclusive cases register.
const EXCLUSIVE_TERMS: [&str; 6] = ["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"];

/// The redirect sources of the Vehicle: registers the vocabulary, registers the Vehicle in the redirect mode for the
/// redirection operations at the source's `/broker1` (`-1`) and `/broker2` (`-2`), starts the source, and stubs its
/// retrieval at `/broker1` as a Mercedes parked at Downtown1 and at `/broker2` as a BMW parked (`isParked2`) at
/// Downtown2; answers the vocabulary and the source.
pub(crate) async fn redirect_vehicle_sources(broker: &BrokerClient, namespace: &Namespace) -> Result<(Vocabulary, ContextSource, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &REDIRECT_TERMS).await?;
    let source = ContextSource::refusing()?;
    let first = vehicle_registration(namespace, 1, &source_endpoint(&source, namespace, "/broker1")?, RegistrationMode::Redirect, &[])?;
    let first = register(broker, &vocabulary, &first).await?;
    let second = vehicle_registration(namespace, 2, &source_endpoint(&source, namespace, "/broker2")?, RegistrationMode::Redirect, &[])?;
    let registrations = first.absorbing(register(broker, &vocabulary, &second).await?);
    let source = serve(source).await?;
    stub(
        &source,
        vehicle_retrieval_rule(namespace, "/broker1", 1),
        StatusCode::OK,
        Some(&vehicle_reply(namespace, 1, "Mercedes", "isParked", "Downtown1", "Bob")),
    )
    .await?;
    stub(
        &source,
        vehicle_retrieval_rule(namespace, "/broker2", 1),
        StatusCode::OK,
        Some(&vehicle_reply(namespace, 1, "BMW", "isParked2", "Downtown2", "Alice")),
    )
    .await?;
    Ok((vocabulary, source, registrations))
}

/// The exclusive source of the Vehicle's `speed`: registers the vocabulary, creates the Vehicle locally as a Mercedes
/// parked at Downtown1, registers its `speed` in the exclusive mode for the redirection operations at the source's
/// `/broker1`, starts the source, and stubs the Vehicle's retrieval there with a `speed` of 56; answers the vocabulary
/// and the source.
pub(crate) async fn exclusive_speed_source(broker: &BrokerClient, namespace: &Namespace) -> Result<(Vocabulary, ContextSource, Cleanup)> {
    let vocabulary = Vocabulary::register(broker, namespace, &EXCLUSIVE_TERMS).await?;
    let source = ContextSource::refusing()?;
    create_entity_locally(broker, &vocabulary, &vehicle_parked(namespace, 1)?).await?;
    let endpoint = source_endpoint(&source, namespace, "/broker1")?;
    let registrations = register(
        broker,
        &vocabulary,
        &vehicle_registration(namespace, 1, &endpoint, RegistrationMode::Exclusive, &["speed"])?,
    )
    .await?;
    let source = serve(source).await?;
    let reply = json!({
        "id": namespace.urn_text("Vehicle", 1),
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 56, "source": {"type": "Property", "value": "Speedometer"}},
    });
    stub(&source, vehicle_retrieval_rule(namespace, "/broker1", 1), StatusCode::OK, Some(&reply)).await?;
    Ok((vocabulary, source, registrations))
}

/// The stub rules of the redirect sources' retrievals, at `/broker1` and `/broker2`.
pub(crate) fn redirect_rules(namespace: &Namespace) -> [ForkRule; 2] {
    [
        vehicle_retrieval_rule(namespace, "/broker1", 1),
        vehicle_retrieval_rule(namespace, "/broker2", 1),
    ]
}

/// The teardown of the cases: deletes the registrations `suffixes` (204 each) and the Vehicle.
pub(crate) async fn torn_down(broker: &BrokerClient, namespace: &Namespace, suffixes: &[u32]) -> Result<()> {
    for suffix in suffixes {
        unregister(broker, namespace, *suffix).await?;
    }
    delete_entities(broker, &[&namespace.urn("Vehicle", 1)?]).await;
    Ok(())
}
