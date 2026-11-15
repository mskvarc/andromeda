//! A corpus of `csf` terms asked of Query Context Source Registrations and of a Context Source Registration
//! Subscription over the same registrations.
//!
//! Clause 4.9 gives a Query Term one meaning wherever a broker evaluates it, and a `csf` over registrations is
//! evaluated in two places: in the store for a discovery query (clause 5.10.2.4), and over the registration a write
//! produced for a Subscription to registrations (clause 5.11.2.4). A corpus either states once which registrations
//! each term selects and expects both answers to be exactly those, or states nothing and expects the two answers to be
//! one.

use crate::{
    broker::BrokerClient,
    cleanup::Cleanup,
    core_context::core_context_list,
    namespace::Namespace,
    notification_sink::{NotificationSink, SinkError},
    problem::answered,
};
use futures::future::join_all;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{EntityTypeName, Input, NgsiLdMediaType, RawJson},
    query::q::Query,
    registration::{CSourceRegistration, RegistrationId},
    subscription::{Endpoint, NotificationParams, Subscription},
};
use std::{collections::HashSet, time::Duration};
use tokio::time::sleep;

/// How long every endpoint is watched for a notification beyond the ones owed.
///
/// One Subscription's notifications are not delivered in commit order, so no
/// barrier registration created last can close its window: a registration the
/// term does not select is held to have notified nobody once the window passes.
const QUIET_WINDOW: Duration = Duration::from_secs(3);

/// One registration of a corpus: the registration the model writes, and the Context Source Properties it carries
/// beside it, each written as its registrant's JSON under its name.
#[derive(Debug, Clone, Copy)]
pub struct CsfRegistration<'c> {
    /// The registration, its `information` naming the corpus's entity type.
    pub registration: &'c CSourceRegistration<Input>,
    /// The Context Source Properties, by name, which the model refuses to write.
    pub properties: &'c [(&'c str, &'c RawJson)],
}

/// One Query Term of a `csf` corpus and the registrations clause 4.9 has it select.
#[derive(Debug, Clone, Copy)]
pub struct CsfCase<'c> {
    /// The term, as a client writes it.
    pub csf: &'c str,
    /// The registrations it selects among those the corpus registers, in any order.
    pub selects: &'c [&'c RegistrationId],
}

/// The two answers one term of a corpus received.
#[derive(Debug)]
struct Answers {
    /// The registrations Query Context Source Registrations named.
    queried: HashSet<RegistrationId>,
    /// The registrations the term's Subscription was notified of.
    notified: HashSet<RegistrationId>,
}

/// Subscribes to the registrations of `entity_type` once for each case, with its `csf`, registers `registrations`,
/// and expects Query Context Source Registrations by `entity_type` with each `csf`, and the notifications each
/// Subscription receives, to name exactly the registrations the case selects.
///
/// # Errors
///
/// Fails when a value cannot be built, a term is not a query, a request is refused or the sink cannot be read.
pub async fn expect_registration_query_and_subscription_alike(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    registrations: &[CsfRegistration<'_>],
    cases: &[CsfCase<'_>],
) -> Result<()> {
    let terms: Vec<&str> = cases.iter().map(|case| case.csf).collect();
    let answers = answer_every_term(broker, namespace, entity_type, registrations, &terms, |index, _| {
        cases.get(index).map_or(0, |case| case.selects.len())
    })
    .await?;
    for (case, answer) in cases.iter().zip(&answers) {
        let selected: HashSet<RegistrationId> = case.selects.iter().map(|id| (*id).clone()).collect();
        expect_that!(answer.queried, eq(&selected), "Query Context Source Registrations with csf={}", case.csf);
        expect_that!(
            answer.notified,
            eq(&selected),
            "a Context Source Registration Subscription with csf={}",
            case.csf
        );
    }
    Ok(())
}

/// Subscribes to the registrations of `entity_type` once for each term, registers `registrations`, and expects the
/// notifications each Subscription receives to name exactly the registrations Query Context Source Registrations by
/// `entity_type` answers with the same `csf`, whatever those are.
///
/// # Errors
///
/// Fails when a value cannot be built, a term is not a query, a request is refused or the sink cannot be read.
pub async fn expect_registration_query_and_subscription_agree(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    registrations: &[CsfRegistration<'_>],
    terms: &[&str],
) -> Result<()> {
    let answers = answer_every_term(broker, namespace, entity_type, registrations, terms, |_, queried| queried.len()).await?;
    for (term, answer) in terms.iter().zip(&answers) {
        expect_that!(
            answer.notified,
            eq(&answer.queried),
            "a Context Source Registration Subscription and Query Context Source Registrations with csf={term}"
        );
    }
    Ok(())
}

/// The two answers each of `terms` receives over `registrations`: every Subscription exists before the first
/// registration, so each registration is matched against every term, and each endpoint is watched until it holds
/// the notifications `owed` counts for it, given the term's index and its query answer, and for the quiet window
/// beyond. A notification owed but never delivered shows as a missing registration after the delivery deadline.
async fn answer_every_term(
    broker: &BrokerClient,
    namespace: &Namespace,
    entity_type: &EntityTypeName,
    registrations: &[CsfRegistration<'_>],
    terms: &[&str],
    owed: impl Fn(usize, &HashSet<RegistrationId>) -> usize,
) -> Result<Vec<Answers>> {
    let sink = NotificationSink::start().await?;
    let mut cleanups = Vec::with_capacity(terms.len() + registrations.len());
    for (endpoint, term) in terms.iter().enumerate() {
        let subscription_id = namespace.subscription_id(format!("csf-{endpoint}"))?;
        let subscription = Subscription::registration_builder()
            .id(subscription_id.clone())
            .entities([entity_type.as_str()])
            .csf(term.parse::<Query>()?)
            .notification(NotificationParams::builder(Endpoint::builder(sink.endpoint(endpoint)?).accept(NgsiLdMediaType::Json).build()?).build()?)
            .build()?;
        broker.registration_subscriptions().create(&subscription).send().await?;
        // Subscriptions are deleted when the test ends, however it ends, so none outlives it to notify another test's
        // sink.
        cleanups.push(Cleanup::of(broker).registration_subscription(&subscription_id));
    }
    let with_properties = broker.with_context(core_context_list()?)?.with_context_delivery(ContextDelivery::LdJsonBody);
    for registered in registrations {
        let mut document = UncheckedDocument::try_from(registered.registration.render())?;
        for (name, value) in registered.properties {
            document = document.set_member(name, value);
        }
        let request = with_properties
            .unchecked(&RegisterContextSource {
                registration: registered.registration,
            })?
            .document(&document);
        expect_that!(with_properties.send_unchecked(request).await, answered(StatusCode::CREATED));
        // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
        // cooldown that opens there holds back a later test's registration naming the same endpoint.
        cleanups.push(Cleanup::of(&with_properties).registration(registered.registration.id().or_fail()?));
    }
    let mut queried = Vec::with_capacity(terms.len());
    for term in terms {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .registrations()
            .query()
            .entity_type(entity_type.as_str())
            .csf(term.parse::<Query>()?)
            .send()
            .await?;
        queried.push(
            page.items()
                .iter()
                .map(|registration| registration.id().clone())
                .collect::<HashSet<RegistrationId>>(),
        );
    }
    // Every endpoint is watched at once, so a term whose notifications fall short costs one delivery deadline, not one
    // per term.
    let waits = queried
        .iter()
        .enumerate()
        .map(|(endpoint, answer)| sink.wait_for_at_least(endpoint, owed(endpoint, answer)));
    for outcome in join_all(waits).await {
        match outcome {
            Ok(_) | Err(SinkError::TimedOut(_)) => {}
            Err(failure @ SinkError::Server(_)) => return Err(failure.into()),
        }
    }
    sleep(QUIET_WINDOW).await;
    let mut answers = Vec::with_capacity(terms.len());
    for (endpoint, queried) in queried.into_iter().enumerate() {
        let mut notified = HashSet::new();
        for notification in sink.received(endpoint).await? {
            notified.extend(notification.registrations()?.data().iter().map(|registration| registration.id().clone()));
        }
        answers.push(Answers { queried, notified });
    }
    Ok(answers)
}
