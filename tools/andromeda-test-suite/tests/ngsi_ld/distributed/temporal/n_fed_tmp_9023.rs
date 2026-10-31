//! A source evolution under an aliasing context merges with the local history.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    broker,
    context_source::ContextSource,
    evolution_matcher::has_evolution,
    header_matcher::absent,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::temporal_query,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities over an inclusive registration whose source answers an Entity under its
/// own @context aliasing kind to @type and vals to `ngsi-ld:hasValues`, its speed written as kind Property with vals
/// pairs, answers 200 with the source's two instances merged beside the local one as Property instances and no
/// NGSILD-Warning
/// Clause: 4.4, 4.5.9, 5.5.4, 5.5.7, 5.7.4.4, 6.3.17
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9023_a_source_evolution_under_an_aliasing_context_merges_with_the_local_history() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let aliased_probe_id = namespace.urn("AliasedProbe", 1)?;
    let aliased_probe_type = namespace.entity_type("AliasedProbe")?;
    let registration_id = namespace.registration_id("history")?;
    let aliased_probe_evolution = TemporalEntity::builder()
        .id(aliased_probe_id.clone())
        .entity_type(aliased_probe_type.clone())
        .property("speed", Property::builder(19).observed_at("2025-12-31T23:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&aliased_probe_evolution).send().await?.created()?;
    // The source answers under an @context aliasing kind and vals, which no temporal entity the model builds carries, so the answer is written as text.
    source
        .answer(
            Method::GET,
            "/history/ngsi-ld/v1/temporal/entities",
            source_answer::text(
                StatusCode::OK,
                HeaderValue::from_static("application/json"),
                &format!(
                    r#"[{{"@context": {{"kind": "@type", "vals": "https://uri.etsi.org/ngsi-ld/hasValues"}}, "id": "urn:ngsi-ld:AliasedProbe:{namespace}-1", "kind": "AliasedProbe_{namespace}", "speed": {{"kind": "Property", "vals": [[20, "2026-01-01T00:00:00Z"], [21, "2026-01-01T01:00:00Z"]]}}}}]"#
                ),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("history")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(aliased_probe_type.clone()).with_id_pattern(namespace.id_pattern("AliasedProbe")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2025-01-01T00:00:00Z".parse()?))
        .entity_type(&aliased_probe_type)
        .send()
        .await?;
    expect_that!(page.warnings(), absent());
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&aliased_probe_id)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_evolution(
                "speed",
                all!(
                    len(eq(3)),
                    each(property()),
                    contains(any_attribute().value(19)),
                    contains(any_attribute().value(20).observed_at("2026-01-01T00:00:00Z".parse()?)),
                    contains(any_attribute().value(21).observed_at("2026-01-01T01:00:00Z".parse()?))
                )
            )
        )
    );
    broker.registrations().delete(&registration_id).send().await?;
    broker.temporal().delete(&aliased_probe_id).send().await?;
    Ok(())
}
