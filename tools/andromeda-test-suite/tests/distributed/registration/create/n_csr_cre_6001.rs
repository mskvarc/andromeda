//! Every optional member reads back as written.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    raw_answer::send_raw,
    wire::request_to,
};
use googletest::prelude::*;
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::{
    http::headers::Accept,
    model::{ContextSourceAlias, DatasetId, Duration, NgsiLdMediaType, OneOrMany, Scope},
    query::Locality,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo, RegistrationManagementInfo, TimeInterval},
};

/// Pins: Create Context Source Registration with `registrationName`, description, `refreshRate`,
/// `contextSourceAlias`, `datasetId` including @none, scope, management and an open-ended `managementInterval`
/// answers 201, and the registration reads back with each member as written and without an `expiresAt`
/// Clause: 5.9.2, 5.2.9, 5.2.34, 5.2.11
#[gtest]
#[tokio::test]
async fn n_csr_cre_6001_every_optional_member_reads_back_as_written() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let dataset_id = namespace.dataset_id("Dataset", 1)?;
    let scope = OneOrMany::try_from(vec!["/Madrid/Gardens".parse::<Scope>()?, "/Madrid/Parks".parse::<Scope>()?])?;
    let interval = TimeInterval::starting_at("2020-01-01T00:00:00Z".parse()?);
    let registration = CSourceRegistration::builder(source.endpoint("1")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .registration_name(format!("Fleet_{namespace}"))
        .description(format!("Telemetry source {namespace}"))
        .refresh_rate("PT5M".parse::<Duration>()?)
        .context_source_alias(format!("alias-{namespace}").parse::<ContextSourceAlias>()?)
        .dataset_ids([dataset_id.clone(), DatasetId::DefaultInstance])
        .scope(scope.clone())
        .management(
            RegistrationManagementInfo::builder()
                .locality(Locality::Local)
                .cache_duration("PT10S".parse::<Duration>()?)
                .timeout(3000)
                .cooldown(500)
                .build()?,
        )
        .management_interval(interval)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .registrations()
        .retrieve(&registration_id)
        .send()
        .await?;
    expect_that!(retrieved.id(), eq(&registration_id));
    expect_that!(retrieved.name(), some(eq(format!("Fleet_{namespace}").as_str())));
    expect_that!(retrieved.description(), some(eq(format!("Telemetry source {namespace}").as_str())));
    expect_that!(retrieved.refresh_rate(), some(eq(&"PT5M".parse::<Duration>()?)));
    expect_that!(
        retrieved.context_source_alias(),
        some(eq(&format!("alias-{namespace}").parse::<ContextSourceAlias>()?))
    );
    expect_that!(retrieved.dataset_ids(), some(eq([dataset_id, DatasetId::DefaultInstance].as_slice())));
    expect_that!(retrieved.scope(), some(eq(&scope)));
    let management = retrieved.management().or_fail()?;
    expect_that!(management.cache_duration(), some(eq(&"PT10S".parse::<Duration>()?)));
    expect_that!(management.locality(), eq(Locality::Local));
    expect_that!(management.timeout().map(ToString::to_string), some(eq("3000")));
    expect_that!(management.cooldown().map(ToString::to_string), some(eq("500")));
    expect_that!(retrieved.management_interval(), some(eq(&interval)));
    expect_that!(retrieved.expires_at(), none());
    // The typed read normalizes the spellings the test also pins (`localOnly`, the duration texts, the instant, the
    // absent `endAt` and `expiresAt`), so the answer is read as received too.
    let read = request_to(&broker, Method::GET, &format!("csourceRegistrations/{registration_id}"))?
        .append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = send_raw(read).await?;
    expect_that!(response.body(), json_equals("$.refreshRate", "PT5M"));
    expect_that!(response.body(), json_equals("$.management.cacheDuration", "PT10S"));
    expect_that!(response.body(), json_equals("$.management.localOnly", true));
    expect_that!(response.body(), json_equals("$.managementInterval.startAt", "2020-01-01T00:00:00Z"));
    expect_that!(response.body(), json_absent("$.managementInterval.endAt"));
    expect_that!(response.body(), json_absent("$.expiresAt"));
    Ok(())
}
