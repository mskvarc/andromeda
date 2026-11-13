//! Location written as a Property under any spelling of its name is bad request data.

use andromeda_test_suite::{
    broker::{self, BrokerClient},
    core_context::core_entry,
    namespace::Namespace,
    problem::problem,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::{Context, ContextEntry, Entity, EntityTypeName, ErrorType, InlineContext, Lossless, NgsiLdMediaType, RawJson},
};

/// Writes the probe `suffix` carrying `location` as a Property under the name `spelling` through `client`, and
/// expects 400 `BadRequestData` naming `location`.
async fn expect_refused(client: &BrokerClient, namespace: &Namespace, probe_type: &EntityTypeName, suffix: &str, spelling: &str) -> Result<()> {
    let probe = Entity::builder()
        .id(namespace.urn("GeoMemberProbe", suffix)?)
        .entity_type(probe_type.clone())
        .build()?;
    // The model admits `location` only as a GeoProperty, so it is written as text.
    let location: RawJson = r#"{"type": "Property", "value": "somewhere"}"#.parse()?;
    let document = UncheckedDocument::from_entity(&probe, Lossless::Normalized).set_member(spelling, &location);
    let request = client.unchecked(&CreateEntity::builder().entity(&probe).build())?.document(&document);
    let answer = client.send_unchecked(request).await;
    verify_that!(
        &answer,
        problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData).with_detail_containing("'location' must be a GeoProperty")
    )
}

/// Pins: Create Entity holding `location` as a Property under the core prefix (`ngsi-ld:location`), under its IRI,
/// or under a term of the user `@context` that expands to its IRI, answers 400 `BadRequestData` naming `location`
/// and stores nothing
/// Clause: 5.2.4, 5.5.4, 5.5.7, 5.6.1.4
#[gtest]
#[tokio::test]
async fn n_ent_cre_9064_location_written_as_a_property_under_any_spelling_of_its_name_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("GeoMemberProbe")?;
    expect_refused(&broker, &namespace, &probe_type, "prefixed", "ngsi-ld:location").await?;
    expect_refused(&broker, &namespace, &probe_type, "expanded", "https://uri.etsi.org/ngsi-ld/location").await?;
    let alias: InlineContext = r#"{"whereabouts": "https://uri.etsi.org/ngsi-ld/location"}"#.parse()?;
    let aliasing = broker
        .with_context(Context::Multiple(vec![ContextEntry::Inline(alias), core_entry()?]))?
        .with_context_delivery(ContextDelivery::LdJsonBody);
    expect_refused(&aliasing, &namespace, &probe_type, "aliased", "whereabouts").await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .query()
        .entity_type(&probe_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    Ok(())
}
