//! A killed pool never answers a federated query with local data alone.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    config::BrokerRole,
    context_source::ContextSource,
    namespace::Namespace,
    pool_outage::{MAX_OUTAGE_REQUESTS, kill_then_resume, through_outage},
    pooler_console::PoolerConsole,
    problem::answered,
    source_answer,
    tenant_management::database_of,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    client::ClientError,
    model::{Entity, EntityId},
    registration::{CSourceRegistration, DistributedOperation, RegistrationInfo, RegistrationMode},
};
use std::iter;

/// What one query of the drive got: the Entities of a 200, the status of an error answer, or no answer.
#[derive(Debug)]
enum FederatedAnswer {
    /// A success, with the Entities it held.
    Entities(Vec<EntityId>),
    /// An error answer.
    Refused(StatusCode),
    /// No answer.
    Unanswered,
}

/// Pins: with an inclusive registration of the test's Entity type whose source holds one remote Entity, while the
/// pooler KILLs the tenant's database pool and RESUMEs it a second later, Query Entities of the type, sent every
/// 200 ms until one succeeds after the resume, is answered 200 holding the remote Entity, 503, or not at all, never
/// 200 without it and never another error; and one succeeds within 45 s
/// Clause: 6.3.17, 5.5.6, 5.7.2
/// Tenant: private (the subject is the pool of a whole tenant database, which the pooler addresses by database)
/// Recorded: Andromeda deployment, not v1.9.1: the broker serves its requests through a transaction-pooling `PgBouncer`
#[gtest]
#[tokio::test]
async fn p_res_out_0004_a_killed_pool_never_answers_a_federated_query_with_local_data_alone() -> Result<()> {
    let namespace = Namespace::fresh();
    let resilience = broker::client(BrokerRole::Resilience)?;
    let set = TenantSet::provision(&resilience, &namespace, 1, 1).await?;
    let tenant = &set.member(0).client;
    let database = database_of(&resilience, &set.member(0).tenant).await?;
    let probe_type = namespace.entity_type("OutageProbe")?;
    let remote_id = namespace.urn("OutageProbe", "remote")?;
    let source = ContextSource::start().await?;
    let remote = Entity::output_builder().id(remote_id.clone()).entity_type(probe_type.clone()).build()?;
    source.answer(Method::GET, "/ngsi-ld/v1/entities", source_answer::entities(&[remote])).await?;
    let registration_id = namespace.registration_id("outage")?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder().entities([probe_type.as_str()]).build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    let registered = tenant.with_responses().registrations().register(&registration).send().await;
    expect_that!(&registered, answered(StatusCode::CREATED));
    let _registration_cleanup = Cleanup::of(tenant).registration(&registration_id);
    let console = PoolerConsole::of(BrokerRole::Resilience)?;
    let drive = through_outage(
        iter::repeat_n((), MAX_OUTAGE_REQUESTS),
        |()| async {
            match tenant.entities().query().entity_type(&probe_type).send().await {
                Ok(page) => FederatedAnswer::Entities(page.items().iter().map(|entity| entity.id.clone()).collect()),
                Err(ClientError::Response(response)) => FederatedAnswer::Refused(response.status()),
                Err(_unanswered) => FederatedAnswer::Unanswered,
            }
        },
        |answer| matches!(answer, FederatedAnswer::Entities(_)),
    );
    let (answers, killed) = tokio::join!(drive, kill_then_resume(&console, &database));
    killed?;
    let answers = answers.into_iter().map(|((), answer)| answer).collect::<Vec<_>>();
    expect_that!(
        answers,
        each(predicate(|answer: &FederatedAnswer| match answer {
            FederatedAnswer::Entities(ids) => ids.contains(&remote_id),
            FederatedAnswer::Refused(status) => *status == StatusCode::SERVICE_UNAVAILABLE,
            FederatedAnswer::Unanswered => true,
        }))
    );
    verify_that!(matches!(answers.last(), Some(FederatedAnswer::Entities(_))), eq(true))
}
