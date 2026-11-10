//! A source history past the instance limit is too many results naming it.

use andromeda_test_suite::{broker, cleanup::Cleanup, config::BrokerRole, context_source::ContextSource, namespace::Namespace, problem::problem};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{
        ErrorType,
        InstanceId,
        Lossless,
        Property,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};
use wiremock::ResponseTemplate;

/// Pins: Query Temporal Evolution of Entities on broker-f, whose limit is 3 attribute instances, as `aggregatedValues`
/// with `splitEntities` split and a `q` filter, over a source answering one Entity with 4 `speed` instances, answers 403
/// `TooManyResults` whose detail names `federation.query.max_instances`
/// Clause: 4.12, 5.7.4.4
/// Recorded: clause 4.12 leaves the threshold of `TooManyResults` to the implementation; broker-f gathers at most 3
/// attribute instances from the sources whose history it reads to the end, as the aggregates of an entity whose
/// filters are decided over its merged parts need (clause 5.7.4.4)
#[gtest]
#[tokio::test]
async fn o_fed_tmp_7457_a_source_history_past_the_instance_limit_is_too_many_results_naming_it() -> Result<()> {
    let namespace = Namespace::fresh();
    let bounded = broker::client(BrokerRole::F)?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    bounded.registrations().register(&registration).send().await?;
    let _registration_cleanup = Cleanup::of(&bounded).registration(&registration_id);
    let mut history = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone());
    for (number, observed_at) in [
        (1, "2020-08-01T12:00:00Z"),
        (2, "2020-08-01T13:00:00Z"),
        (3, "2020-08-01T14:00:00Z"),
        (4, "2020-08-01T15:00:00Z"),
    ] {
        history = history.identified(
            "speed",
            namespace.urn_text("instance", number).parse::<InstanceId>()?,
            Property::builder(number).observed_at(observed_at.parse()?),
        );
    }
    let history = history.build()?;
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            ResponseTemplate::new(StatusCode::OK.as_u16()).set_body_raw(format!("[{}]", history.render(Lossless::Normalized)), "application/json"),
        )
        .await?;
    let answer = bounded
        .with_responses()
        .with_accept(Accept::any())
        .temporal()
        .query_aggregated(temporal_query::before("2030-01-01T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::TotalCount]))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("speed").gt(0))
        .send()
        .await;
    expect_that!(
        &answer,
        problem(StatusCode::FORBIDDEN, ErrorType::TooManyResults).with_detail_containing("federation.query.max_instances")
    );
    bounded.registrations().delete(&registration_id).send().await?;
    Ok(())
}
