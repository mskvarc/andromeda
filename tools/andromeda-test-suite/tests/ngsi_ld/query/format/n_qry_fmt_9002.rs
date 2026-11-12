//! Every representation the tables define is accepted.

use andromeda_test_suite::{broker, namespace::Namespace, outcome::Done, problem::answered};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{
            NormalizedTemporal,
            RepresentationSpelling,
            RetrieveEntity,
            RetrieveTemporalEvolution,
            Simplified,
            SimplifiedTemporal,
            SystemAttributes,
            TemporalSelection,
        },
    },
    model::{Entity, Lossless, NgsiLdMediaType, Property},
    query::temporal_query::TemporalQuery,
};

/// Pins: Retrieve Entity with each format and options spelling of Table 6.3.7-1, `sysAttrs` alone or beside
/// `keyValues`, and Retrieve Temporal Evolution with `temporalValues` as format or option, `sysAttrs`, or both,
/// answer 200
/// Clause: Table 6.3.7-1, Table 6.3.12-1, 6.3.11
#[gtest]
#[tokio::test]
async fn n_qry_fmt_9002_every_representation_the_tables_define_is_accepted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "supported-representation")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", Property::builder(60).observed_at("2025-01-01T00:00:00Z".parse()?))
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json_ld = broker.with_accept(Accept::new(NgsiLdMediaType::JsonLd));
    let spelling = RepresentationSpelling::default();
    let mut requests = Vec::new();
    for (format, spelling, system_attributes) in [
        (Lossless::Normalized, spelling.explicit_normalized(), SystemAttributes::Omit),
        (Lossless::Concise, spelling, SystemAttributes::Omit),
        (Lossless::Normalized, spelling.in_options().explicit_normalized(), SystemAttributes::Omit),
        (Lossless::Concise, spelling.in_options(), SystemAttributes::Omit),
        (Lossless::Normalized, spelling, SystemAttributes::Include),
    ] {
        let retrieve = RetrieveEntity::builder()
            .id(&vehicle_id)
            .format(format)
            .spelling(spelling)
            .system_attributes(system_attributes)
            .build();
        requests.push(json_ld.unchecked(&retrieve)?);
    }
    for (spelling, system_attributes) in [
        (spelling, SystemAttributes::Omit),
        (spelling.key_values(), SystemAttributes::Omit),
        (spelling.in_options(), SystemAttributes::Omit),
        (spelling.in_options().key_values(), SystemAttributes::Omit),
        (spelling.in_options().key_values(), SystemAttributes::Include),
    ] {
        let retrieve = RetrieveEntity::builder()
            .id(&vehicle_id)
            .format(Simplified)
            .spelling(spelling)
            .system_attributes(system_attributes)
            .build();
        requests.push(json_ld.unchecked(&retrieve)?);
    }
    let after = TemporalQuery::after("2000-01-01T00:00:00Z".parse()?);
    for (spelling, system_attributes) in [
        (spelling, SystemAttributes::Omit),
        (spelling.in_options(), SystemAttributes::Omit),
        (spelling.in_options(), SystemAttributes::Include),
    ] {
        let retrieve = RetrieveTemporalEvolution::builder()
            .id(&vehicle_id)
            .selection(TemporalSelection::Query(&after))
            .format(SimplifiedTemporal)
            .spelling(spelling)
            .system_attributes(system_attributes)
            .build();
        requests.push(json_ld.unchecked(&retrieve)?);
    }
    let retrieve = RetrieveTemporalEvolution::builder()
        .id(&vehicle_id)
        .selection(TemporalSelection::Query(&after))
        .format(NormalizedTemporal)
        .system_attributes(SystemAttributes::Include)
        .build();
    requests.push(json_ld.unchecked(&retrieve)?);
    for request in requests {
        expect_that!(&json_ld.send_unchecked(request).await, answered(StatusCode::OK));
    }
    Ok(())
}
