//! An update mode upsert racing a create of the same id ends as one of their serial orders.

use andromeda_test_suite::{
    answered_status::AnsweredStatus,
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::{has_attribute, lacks_attribute},
    namespace::Namespace,
};
use futures::{StreamExt, stream};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    client::{ClientError, Received},
    http::{UpsertOutcome, headers::Accept, operation::UpsertMode},
    model::{Entity, EntityId, ErrorType, Input, NgsiLdMediaType},
};
use std::{result::Result as StdResult, slice};

/// The ids raced, each by one upsert and one create.
const RACES: u32 = 60;

/// The races in flight at once: 64 requests.
const RACES_IN_FLIGHT: usize = 32;

/// One id raced: the Entity the update mode upsert writes, carrying `u`, and the one the create writes, carrying `c`,
/// both holding the race's number.
#[derive(Debug)]
struct Race {
    /// The id both requests name.
    id: EntityId,
    /// The number both attributes hold.
    value: u32,
    /// The Entity the upsert sends, as a batch of one.
    upserted: [Entity<Input>; 1],
    /// The Entity the create sends.
    created: Entity<Input>,
}

/// How the two requests of one race were answered.
#[derive(Debug, PartialEq)]
struct RaceAnswers<'a> {
    /// The status of the upsert's answer.
    upsert: Option<StatusCode>,
    /// The Entities the upsert reports it created.
    upsert_created: &'a [EntityId],
    /// The status of the create's answer.
    create: Option<StatusCode>,
    /// The problem type the create was refused with.
    create_refused: Option<&'a ErrorType>,
}

/// The Entities a Batch Entity Upsert answer reports created: the 201 answer lists them (clause 6.15.3.1), and every
/// other answer creates none it reports.
fn reported_created<E>(answer: &StdResult<Received<UpsertOutcome>, ClientError<E>>) -> &[EntityId] {
    match answer {
        Ok(received) => match &received.outcome {
            UpsertOutcome::Created(ids) => ids,
            UpsertOutcome::Updated | UpsertOutcome::Partial(_) => &[],
        },
        Err(_) => &[],
    }
}

/// Pins: for each of 60 ids, a Batch Entity Upsert with options=update of an Entity carrying `u` and a Create Entity
/// of an Entity carrying `c`, sent at once, end as one of their two serial orders: the create first answers 201,
/// the upsert then updates the existing Entity and answers 204, and the Entity holds `c` and `u`; or the upsert
/// first creates the Entity and answers 201 listing its id, the create is refused with 409 `AlreadyExists`, and the
/// Entity holds `u` without `c`
/// Clause: 5.5.11.0, 5.6.1.4, 5.6.8.4, 6.15.3.1
#[gtest]
#[tokio::test]
async fn n_bat_ups_0018_an_update_mode_upsert_racing_a_create_of_the_same_id_ends_as_one_of_their_serial_orders() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let probe_type = namespace.entity_type("Probe")?;
    let mut races = Vec::new();
    for value in 0..RACES {
        let id = namespace.urn("Probe", value)?;
        // clone: both Entities of the race and the expectations name the same id and type
        let upserted = Entity::builder().id(id.clone()).entity_type(probe_type.clone()).property("u", value).build()?;
        let created = Entity::builder().id(id.clone()).entity_type(probe_type.clone()).property("c", value).build()?;
        races.push(Race {
            id,
            value,
            upserted: [upserted],
            created,
        });
    }
    let races = races;
    let responding = broker.with_responses();
    let entities = &responding.entities();
    let answered = stream::iter(&races)
        .map(|race| async move {
            let (upsert, create) = tokio::join!(
                entities.batch_upsert(&race.upserted).mode(UpsertMode::Update).send(),
                entities.create(&race.created).send(),
            );
            (race, upsert, create)
        })
        .buffer_unordered(RACES_IN_FLIGHT)
        .collect::<Vec<_>>()
        .await;
    let already_exists = ErrorType::AlreadyExists;
    for (race, upsert, create) in &answered {
        let observed = RaceAnswers {
            upsert: upsert.answered_status(),
            upsert_created: reported_created(upsert),
            create: create.answered_status(),
            create_refused: create.answered_problem().and_then(|problem| problem.error_type.as_ref()),
        };
        let create_first = RaceAnswers {
            upsert: Some(StatusCode::NO_CONTENT),
            upsert_created: &[],
            create: Some(StatusCode::CREATED),
            create_refused: None,
        };
        let upsert_first = RaceAnswers {
            upsert: Some(StatusCode::CREATED),
            upsert_created: slice::from_ref(&race.id),
            create: Some(StatusCode::CONFLICT),
            create_refused: Some(&already_exists),
        };
        expect_that!(observed, any![eq(&create_first), eq(&upsert_first)]);
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .entities()
            .retrieve(&race.id)
            .send()
            .await?;
        let entity = retrieved.body();
        expect_that!(entity, has_attribute("u", any_attribute().value(race.value)));
        if observed.create == Some(StatusCode::CREATED) {
            expect_that!(entity, has_attribute("c", any_attribute().value(race.value)));
        } else {
            expect_that!(entity, lacks_attribute("c"));
        }
    }
    Ok(())
}
