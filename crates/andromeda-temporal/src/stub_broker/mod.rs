//! A temporal service over one stubbed broker, for the tests that exercise
//! what the service does with what its ports answer.
//!
//! The broker's history is spelled as "this entity links to those": every
//! known entity has one plain observation and one Relationship observation per
//! identifier it links to. Everything else a port could be asked panics, so a
//! test that reaches it says so.

use crate::{
    instance_limits::TemporalInstanceLimits,
    merged_parts::LocalPart,
    port::{
        history_reader::TemporalHistoryReader,
        in_effect_reads::{InEffectReads, ScopeRead},
        mutation_barrier::TemporalMutationBarrier,
        mutator::TemporalMutator,
        service_ports::TemporalServicePorts,
    },
    remote_linked::RemoteTemporalLinkedSource,
    service::TemporalService,
    service_limits::TemporalServiceLimits,
};
use andromeda_configuration::schema::{request_limits::RequestLimitsConfig, temporal::query::DEFAULT_MAX_AGGREGATION_INSTANCES};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    configured_policy::{filter_complexity_limits, payload_budget},
    port::reader::EntityReader,
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_linked_retrieval::admission::TemporalJoin;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    entity_type_name::EntityTypeName,
    identifier::Identifier,
    instance_identity::InstanceIdentity,
    time::date_time::NgsiLdDateTime,
};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow, TemporalHistoryRow, TemporalValue, evolution_instance::EvolutionInstance};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    filter::language_targeting::LanguageTargeting,
    geo::near_distance_band::NearDistanceBand,
    split_filter::LeafTruths,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties as expanded_properties;
use andromeda_retry::retry_delay::RetryDelay;
use async_trait::async_trait;
use std::{
    collections::HashMap,
    iter,
    sync::{Arc, Mutex},
    time::Duration,
};

/// The ports the stubbed broker implements.
mod ports;

pub(crate) fn uri(value: &str) -> Identifier {
    value.parse().expect("valid URI")
}

pub(crate) const fn live_target() -> ContextTarget {
    ContextTarget::Live
}

/// A broker whose history holds, for each entity, one Relationship observation
/// per identifier that entity links to.
///
/// The graph is spelled as "this entity links to those", which is all the
/// traversal reads: how far it walks and what it refuses to walk twice.
pub(crate) struct StubBroker {
    /// Each entity's outgoing links, in declaration order.
    links: HashMap<Identifier, Vec<Identifier>>,
    /// Whether the entities have current state beside their history, which a
    /// clause 5.6.6 deletion removes while the Temporal Evolution stands.
    current_state: CurrentState,
    /// The aggregate rows every merged aggregation answers with.
    aggregated: Vec<TemporalAggregationRow>,
    /// The rows the sources answered with, as each merged aggregation was
    /// handed them.
    merged_answers: Mutex<Vec<TemporalHistoryRow>>,
    /// Each entity a merged aggregation ran for, whether its local part joined,
    /// and the types it was named with, in order.
    merged_entities: Mutex<Vec<(Identifier, LocalPart, Vec<EntityTypeName>)>>,
    /// The entity selections each identifier listing ran under, in order.
    listed_selections: Mutex<Vec<EntitySelection>>,
    /// The page of history an entity reads as in place of its links.
    pages: HashMap<Identifier, Vec<TemporalHistoryRow>>,
    /// What the store reads of a split query's filter leaves over each entity.
    leaf_truths: HashMap<Identifier, LeafTruths>,
    /// What each entity holds in effect when a split query's window opens.
    in_effect: HashMap<Identifier, Vec<TemporalHistoryRow>>,
    /// Each entity's hop Relationship instances inside a split query's window.
    window_relationships: HashMap<Identifier, Vec<TemporalHistoryRow>>,
}

/// Whether the stubbed entities have current state.
#[derive(Debug, Clone, Copy)]
pub(crate) enum CurrentState {
    /// Every known entity has current state.
    Held,
    /// No entity has current state, only history.
    Deleted,
}

impl StubBroker {
    pub(crate) fn new(links: impl IntoIterator<Item = (Identifier, Vec<Identifier>)>) -> StubBroker {
        StubBroker {
            links: links.into_iter().collect(),
            current_state: CurrentState::Held,
            aggregated: Vec::new(),
            merged_answers: Mutex::new(Vec::new()),
            merged_entities: Mutex::new(Vec::new()),
            listed_selections: Mutex::new(Vec::new()),
            pages: HashMap::new(),
            leaf_truths: HashMap::new(),
            in_effect: HashMap::new(),
            window_relationships: HashMap::new(),
        }
    }

    /// The same broker reading `truths` as a split query's leaves over
    /// `entity_id`.
    pub(crate) fn judging(mut self, entity_id: Identifier, truths: LeafTruths) -> StubBroker {
        self.leaf_truths.insert(entity_id, truths);
        self
    }

    /// The same broker holding `rows` of `entity_id` in effect when a split
    /// query's window opens.
    pub(crate) fn holding_in_effect(mut self, entity_id: Identifier, rows: Vec<TemporalHistoryRow>) -> StubBroker {
        self.in_effect.insert(entity_id, rows);
        self
    }

    /// The same broker holding `rows` of `entity_id`'s hop Relationships
    /// inside a split query's window.
    pub(crate) fn relating(mut self, entity_id: Identifier, rows: Vec<TemporalHistoryRow>) -> StubBroker {
        self.window_relationships.insert(entity_id, rows);
        self
    }

    /// The entries of `held` for the entities of `entity_ids`.
    fn of_entities<T: Clone>(held: &HashMap<Identifier, T>, entity_ids: &[Identifier]) -> HashMap<Identifier, T> {
        entity_ids
            .iter()
            .filter_map(|entity_id| Some((entity_id.clone(), held.get(entity_id)?.clone()))) // clone: every read answers what the broker holds
            .collect()
    }

    /// The leaf truths the broker reads over `entity_ids`.
    pub(crate) fn leaf_truths_of(&self, entity_ids: &[Identifier]) -> HashMap<Identifier, LeafTruths> {
        StubBroker::of_entities(&self.leaf_truths, entity_ids)
    }

    /// The in-effect rows the broker holds of `entity_ids`, of what `reads`
    /// covers.
    pub(crate) fn in_effect_of(&self, entity_ids: &[Identifier], reads: InEffectReads<'_>) -> HashMap<Identifier, Vec<TemporalHistoryRow>> {
        let covered = |row: &TemporalHistoryRow| {
            if row.attribute_name.as_str() == expanded_properties::SCOPE.as_str() {
                reads.scope == ScopeRead::Read
            } else {
                reads.attributes.contains(&&row.attribute_name)
            }
        };
        let mut held = StubBroker::of_entities(&self.in_effect, entity_ids);
        for rows in held.values_mut() {
            rows.retain(covered);
        }
        held.retain(|_, rows| !rows.is_empty());
        held
    }

    /// The window Relationship rows the broker holds of `entity_ids`.
    pub(crate) fn window_relationships_of(&self, entity_ids: &[Identifier]) -> HashMap<Identifier, Vec<TemporalHistoryRow>> {
        StubBroker::of_entities(&self.window_relationships, entity_ids)
    }

    /// The same graph with `entity_id` known and reading as `rows`, whatever
    /// the window, the way a store answers one page of its history.
    pub(crate) fn paging(mut self, entity_id: Identifier, rows: Vec<TemporalHistoryRow>) -> StubBroker {
        self.links.entry(entity_id.clone()).or_default(); // clone: the graph and the page both key by the entity
        self.pages.insert(entity_id, rows);
        self
    }

    /// The entity selections the identifier listings ran under, in the order
    /// they ran.
    pub(crate) fn listed_selections(&self) -> Vec<EntitySelection> {
        self.listed_selections.lock().expect("the recording lock is never poisoned").clone() // clone: the recording keeps what it was handed
    }

    /// The same graph answering every merged aggregation with `rows`.
    pub(crate) fn aggregating(mut self, rows: Vec<TemporalAggregationRow>) -> StubBroker {
        self.aggregated = rows;
        self
    }

    /// The rows the sources answered with that merged aggregations were handed,
    /// in the order they were handed them.
    pub(crate) fn merged_answers(&self) -> Vec<TemporalHistoryRow> {
        self.merged_answers.lock().expect("the recording lock is never poisoned").clone() // clone: the recording keeps what it was handed
    }

    /// Each entity a merged aggregation ran for, whether its local part joined,
    /// and the types it was named with, in the order they ran.
    pub(crate) fn merged_entities(&self) -> Vec<(Identifier, LocalPart, Vec<EntityTypeName>)> {
        self.merged_entities.lock().expect("the recording lock is never poisoned").clone() // clone: the recording keeps what it was handed
    }

    /// The same graph with the current state of every entity deleted.
    pub(crate) fn without_current_state(mut self) -> StubBroker {
        self.current_state = CurrentState::Deleted;
        self
    }

    /// The history rows one entity's links render into.
    ///
    /// Every known entity also carries one plain observation, so that a leaf
    /// with no outgoing link still has history in the window: an entity with an
    /// empty history is legitimately dropped from a join, and the traversal
    /// tests are about which entities are *reached*, not about that rule.
    pub(crate) fn rows(&self, entity_id: &Identifier) -> Vec<TemporalHistoryRow> {
        if let Some(page) = self.pages.get(entity_id) {
            return page.clone(); // clone: every read of the entity answers the same page
        }
        let instant = "2024-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("valid instant").get();
        let Some(targets) = self.links.get(entity_id) else {
            return Vec::new();
        };
        let reading = TemporalHistoryRow {
            attribute_name: "https://example.com/reading".parse::<AttributeName>().expect("valid attribute IRI"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number {
                number: JsonNumber::try_from(1.0).expect("a finite number"),
                datatype: None,
            },
            observed_at: Some(instant),
            recorded_at: instant,
            created_at: Some(instant),
            modified_at: Some(instant),
            instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{entity_id}:reading")))),
            metadata: None,
            stream_extent: StreamExtent::Whole,
        };
        iter::once(reading)
            .chain(targets.iter().map(|target| TemporalHistoryRow {
                attribute_name: "https://example.com/linksTo".parse::<AttributeName>().expect("valid attribute IRI"),
                attribute_type: AttributeType::Relationship,
                dataset_id: None,
                value: TemporalValue::Uri(target.as_uri().clone()), // clone: the row owns the object it names
                observed_at: Some(instant),
                recorded_at: instant,
                created_at: Some(instant),
                modified_at: Some(instant),
                instance: EvolutionInstance::Attribute(InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{entity_id}:{target}")))),
                metadata: None,
                stream_extent: StreamExtent::Whole,
            }))
            .collect()
    }
}

/// A service whose every port is the same stubbed broker, joining purely
/// against local history.
pub(crate) fn service(broker: StubBroker) -> TemporalService {
    service_over(Arc::new(broker), None)
}

/// A service whose history is the stubbed `broker` and whose non-local half is
/// `remote`.
pub(crate) fn service_over(broker: Arc<StubBroker>, remote: Option<Arc<dyn RemoteTemporalLinkedSource>>) -> TemporalService {
    service_under(broker, remote, limits())
}

/// The bounds a stubbed service runs under unless a test narrows them.
pub(crate) fn limits() -> TemporalServiceLimits {
    TemporalServiceLimits {
        instance_limits: TemporalInstanceLimits {
            default_page: 100,
            maximum: 1000,
            edge_group: 100_000,
            aggregation_instances: DEFAULT_MAX_AGGREGATION_INSTANCES,
        },
        delete_by_query_max_matches: 100,
        aggregation_grouping_max_groups: 100,
        payload_budget: payload_budget(&RequestLimitsConfig::default()),
        near_band: NearDistanceBand::Refused,
        language_targeting: LanguageTargeting::Specification,
        filter_complexity: filter_complexity_limits(&RequestLimitsConfig::default()),
    }
}

/// A service whose history is the stubbed `broker`, whose non-local half is
/// `remote`, bounded by `limits`.
pub(crate) fn service_under(broker: Arc<StubBroker>, remote: Option<Arc<dyn RemoteTemporalLinkedSource>>, limits: TemporalServiceLimits) -> TemporalService {
    let reader = Arc::clone(&broker) as Arc<dyn TemporalHistoryReader>; // clone: one test double supplies every focused capability
    service_over_reader(broker, reader, remote, limits)
}

/// A service whose history is read through `reader`, every other port of it
/// an empty stubbed broker.
pub(crate) fn service_reading(reader: Arc<dyn TemporalHistoryReader>) -> TemporalService {
    service_over_reader(Arc::new(StubBroker::new([])), reader, None, limits())
}

/// A service whose history is read through `reader` and whose every other
/// port is `broker`.
fn service_over_reader(
    broker: Arc<StubBroker>,
    reader: Arc<dyn TemporalHistoryReader>,
    remote: Option<Arc<dyn RemoteTemporalLinkedSource>>,
    limits: TemporalServiceLimits,
) -> TemporalService {
    TemporalService::new(
        TemporalServicePorts {
            entity_reader: Arc::clone(&broker) as Arc<dyn EntityReader>, // clone: one test double supplies every focused capability
            reader,
            mutation_barrier: Arc::clone(&broker) as Arc<dyn TemporalMutationBarrier>,
            mutator: Arc::clone(&broker) as Arc<dyn TemporalMutator>,
            writes: Arc::clone(&broker) as Arc<dyn ContextCommitter>, // clone: one test double supplies every focused capability
            retry_delay: broker as Arc<dyn RetryDelay>,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
        Arc::new(ChangeNotifier::new(None, SubscriptionMatching::Absent)),
        limits,
        remote,
        TemporalJoin::Extended,
    )
}

/// The stubbed broker's commits never lose, so it never waits between attempts.
#[async_trait]
impl RetryDelay for StubBroker {
    async fn wait(&self, _delay: Duration) {}
}
