//! A forwarded query names a major dot minor target version.

use andromeda_test_suite::{broker, context_source::ContextSource, forwarded_head, namespace::Namespace, source_answer};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::{Accept, Prefer},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities by type is forwarded once with a Prefer header ngsi-ld=<major>.<minor>
/// Clause: 4.3.6.8, 6.3.10
/// Recorded: 4.3.6.8 lets a broker indicate the version it can read without requiring it to do so on every
/// forwarded request; Andromeda always sends its own major.minor in Prefer
#[gtest]
#[tokio::test]
async fn o_fed_qry_6417_a_forwarded_query_names_a_major_dot_minor_target_version() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let source_entities = [];
    source
        .answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    let forwarded = source.received_on(Method::GET, "/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_one = forwarded.first().or_fail()?;
    let version = forwarded_head::head(forwarded_one)
        .typed::<Prefer>()
        .valid()
        .and_then(Prefer::version)
        .map(|version| version.to_string());
    expect_that!(version, some(matches_regex(r"^[0-9]+\.[0-9]+$")));
    Ok(())
}
