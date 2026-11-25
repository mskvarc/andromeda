//! One Vehicle split between the front broker and a context source under `splitEntities`, and the same Vehicle held
//! whole by the front broker, each asked one temporal query whose instances a case states once (ETSI GS CIM 009
//! v1.9.1 clause 5.7.4.4).
//!
//! Clause 5.7.4.4 applies `q` and the geoquery to a split Entity once its parts are aggregated, and its last step
//! includes only the Attribute instances "(up to lastN) ... which meet the temporal, query and geoquery restrictions".
//! A split answer and the answer of a broker holding the whole Entity therefore carry the same instances, and a case
//! holds both to the instances it states.

use crate::{
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    context_source::ContextSource,
    equivalence::Equivalent,
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        DateTime,
        EntityId,
        EntityTypeName,
        GeoProperty,
        Input,
        Property,
        Value,
        evolution::{SimplifiedTemporalEntity, TemporalEntity, TemporalValue, TemporalValues},
        geometry::{Position, Shape},
    },
    query::{EntitySplitting, geoquery::GeoQuery, q::Query, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};
use url::Url;

/// The path the source serves Query Temporal Evolution of Entities on.
const SOURCE_QUERY: &str = "/a/ngsi-ld/v1/temporal/entities";

/// What one instance holds.
#[derive(Debug, Clone, Copy)]
pub enum Observed {
    /// A Property holding a number.
    Number(i64),
    /// A `GeoProperty` holding a Point.
    Point {
        /// The first coordinate.
        longitude: f64,
        /// The second coordinate.
        latitude: f64,
    },
}

/// One instance of an Attribute of the Vehicle.
#[derive(Debug, Clone, Copy)]
pub struct Observation {
    /// The Attribute, as the requester's `@context` names it.
    pub attribute: &'static str,
    /// What the instance holds.
    pub value: Observed,
    /// Its `observedAt`.
    pub observed: DateTime,
}

/// The instance of `attribute` holding `value`, observed `minute` minutes past noon on 2020-08-01.
///
/// # Errors
///
/// Fails when the minute names no instant of the hour.
pub fn at_minute(attribute: &'static str, value: Observed, minute: u32) -> Result<Observation> {
    Ok(Observation {
        attribute,
        value,
        observed: format!("2020-08-01T12:{minute:02}:00Z").parse()?,
    })
}

/// The filters a case's query carries beside the window from 12:00 to 13:00 on 2020-08-01.
#[derive(Debug, Clone, Default)]
pub struct CaseFilters {
    /// `q`, when the query carries one.
    pub q: Option<Query>,
    /// The geoquery, when the query carries one.
    pub geo_q: Option<GeoQuery>,
    /// `lastN`, when the query carries one.
    pub last_n: Option<u64>,
}

/// One query over the Vehicle, the instances each holder holds, and the instances the query answers.
#[derive(Debug, Clone)]
pub struct SplitCase {
    /// The instances the front broker holds when the Vehicle is split.
    pub local: Vec<Observation>,
    /// The instances the context source holds when the Vehicle is split.
    pub source: Vec<Observation>,
    /// The filters the query carries.
    pub filters: CaseFilters,
    /// The instances the query answers, in any order.
    pub answers: Vec<Observation>,
}

/// The temporal evolution of `vehicle_id` holding `observations`.
///
/// # Errors
///
/// Fails when an instance cannot be built.
pub fn evolution(vehicle_id: &EntityId, vehicle_type: &EntityTypeName, observations: &[Observation]) -> Result<TemporalEntity<Input>> {
    let mut builder = TemporalEntity::builder().id(vehicle_id.clone()).entity_type(vehicle_type.clone());
    for observation in observations {
        builder = match observation.value {
            Observed::Number(number) => builder.property(observation.attribute, Property::builder(number).observed_at(observation.observed)),
            Observed::Point { longitude, latitude } => builder.geo_property(
                observation.attribute,
                GeoProperty::builder(Shape::Point(Position::try_new(longitude, latitude)?)).observed_at(observation.observed),
            ),
        };
    }
    Ok(builder.build()?)
}

/// Registers the source at `endpoint` for the Vehicles of `namespace` with `queryTemporal`, returning the
/// registration that is deleted when the guard drops.
///
/// # Errors
///
/// Fails when the registration cannot be built or is refused.
pub async fn register_source(broker: &BrokerClient, namespace: &Namespace, endpoint: Url) -> Result<Cleanup> {
    let registration_id = namespace.registration_id(1)?;
    let registration = CSourceRegistration::builder(endpoint)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(namespace.entity_type("Vehicle")?).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    Ok(Cleanup::of(broker).registration(&registration_id))
}

/// Query Temporal Evolution of Entities on `broker` for the Vehicles of `vehicle_type` over the window from 12:00 to
/// 13:00 on 2020-08-01 under `splitEntities`, with `filters`, in the simplified temporal representation.
///
/// # Errors
///
/// Fails when the query is refused or its answer cannot be read.
pub async fn split_query(broker: &BrokerClient, vehicle_type: &EntityTypeName, filters: &CaseFilters) -> Result<Vec<SimplifiedTemporalEntity>> {
    let window = temporal_query::between("2020-08-01T12:00:00Z".parse()?, "2020-08-01T13:00:00Z".parse()?);
    let window = match filters.last_n {
        Some(last_n) => window.last_n(last_n),
        None => window,
    };
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_values(window)
        .entity_type(vehicle_type)
        .split_entities(EntitySplitting::Split)
        .maybe_q(filters.q.clone())
        .maybe_geo_q(filters.geo_q.clone())
        .send()
        .await?;
    Ok(page.items().to_vec())
}

/// The case's Vehicle split between the front broker and a context source answers the instances the case states,
/// and the source is asked for the window alone, with no `lastN` and none of the filters.
///
/// # Errors
///
/// Fails when a request fails or the answer differs from the case.
pub async fn expect_split_answer(case: &SplitCase) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    if !case.local.is_empty() {
        broker
            .temporal()
            .create_or_update(&evolution(&vehicle_id, &vehicle_type, &case.local)?)
            .send()
            .await?
            .created()?;
    }
    let _registration_cleanup = register_source(&broker, &namespace, source.endpoint("a")?).await?;
    source
        .answer(
            Method::GET,
            SOURCE_QUERY,
            source_answer::temporal_entities(&[evolution(&vehicle_id, &vehicle_type, &case.source)?]),
        )
        .await?;
    let answer = split_query(&broker, &vehicle_type, &case.filters).await?;
    expect_answers(&answer, &vehicle_id, &case.answers)?;
    let asked = source.received_on(Method::GET, SOURCE_QUERY).await?;
    let windowed = SourceRequest::new(Method::GET, SOURCE_QUERY)
        .query("timerel", "between")
        .without_query("lastN")
        .without_query("q")
        .without_query("georel");
    verify_that!(source.received_matching(&windowed).await?, len(eq(asked.len())))?;
    verify_that!(asked, not(is_empty()))
}

/// The case's Vehicle held whole by the front broker, which no registration covers, answers the instances the case
/// states to the same query.
///
/// # Errors
///
/// Fails when a request fails or the answer differs from the case.
pub async fn expect_whole_answer(case: &SplitCase) -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let whole: Vec<Observation> = case.local.iter().chain(&case.source).copied().collect();
    broker
        .temporal()
        .create_or_update(&evolution(&vehicle_id, &vehicle_type, &whole)?)
        .send()
        .await?
        .created()?;
    let answer = split_query(&broker, &vehicle_type, &case.filters).await?;
    expect_answers(&answer, &vehicle_id, &case.answers)
}

/// One instance an answer carries: its Attribute, its instant, and the number a Property holds.
type AnsweredInstance<'a> = (&'a str, DateTime, Option<&'a Value>);

/// Every instance `entity` carries.
#[must_use]
pub fn answered_instances(entity: &SimplifiedTemporalEntity) -> Vec<AnsweredInstance<'_>> {
    entity
        .attributes
        .iter()
        .flat_map(|(name, values)| {
            let name = name.as_str();
            let instances: Vec<AnsweredInstance<'_>> = match values {
                TemporalValues::Property(pairs) => pairs.iter().map(|pair| (name, pair.time, Some(&pair.value))).collect(),
                TemporalValues::GeoProperty(pairs) => instants(name, pairs),
                TemporalValues::LanguageProperty(pairs) => instants(name, pairs),
                TemporalValues::VocabProperty(pairs) => instants(name, pairs),
                TemporalValues::ListProperty(pairs) => instants(name, pairs),
                TemporalValues::JsonProperty(pairs) => instants(name, pairs),
                TemporalValues::Relationship(pairs) => instants(name, pairs),
                TemporalValues::ListRelationship(pairs) => instants(name, pairs),
            };
            instances
        })
        .collect()
}

/// The instants of `pairs`, each the instance of `name` it dates.
fn instants<'a, T>(name: &'a str, pairs: &[TemporalValue<T>]) -> Vec<AnsweredInstance<'a>> {
    pairs.iter().map(|pair| (name, pair.time, None)).collect()
}

/// `answer` is the one Vehicle `vehicle_id` carrying exactly the instances `expected` states: the same Attribute and
/// instant for each, and the same number for each Property.
fn expect_answers(answer: &[SimplifiedTemporalEntity], vehicle_id: &EntityId, expected: &[Observation]) -> Result<()> {
    verify_that!(answer, len(eq(1)))?;
    let entity = answer.first().or_fail()?;
    verify_that!(&entity.id, eq(vehicle_id))?;
    let mut carried = answered_instances(entity);
    carried.sort_by_key(|(name, time, _)| (*name, *time));
    let mut stated: Vec<&Observation> = expected.iter().collect();
    stated.sort_by_key(|observation| (observation.attribute, observation.observed));
    let carried_keys: Vec<(&str, DateTime)> = carried.iter().map(|(name, time, _)| (*name, *time)).collect();
    let stated_keys: Vec<(&str, DateTime)> = stated.iter().map(|observation| (observation.attribute, observation.observed)).collect();
    verify_that!(carried_keys, eq(&stated_keys))?;
    for ((_, time, value), observation) in carried.iter().zip(&stated) {
        if let (Some(value), Observed::Number(number)) = (value, observation.value) {
            verify_that!(value.equivalent(&Value::from(number)), eq(true)).with_failure_message(|| format!("the instance at {time} holds {number}"))?;
        }
    }
    Ok(())
}
