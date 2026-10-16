//! A temporal service over one stubbed broker, for the tests that exercise
//! what the service does with what its ports answer.
//!
//! The broker's history is spelled as "this entity links to those": every
//! known entity has one plain observation and one Relationship observation per
//! identifier it links to. Everything else a port could be asked panics, so a
//! test that reaches it says so.

use crate::{
    config::DEFAULT_AGGREGATION_INSTANCE_BUDGET,
    instance_limits::TemporalInstanceLimits,
    port::{history_reader::TemporalHistoryReader, mutation_barrier::TemporalMutationBarrier, mutator::TemporalMutator, service_ports::TemporalServicePorts},
    remote_linked::RemoteTemporalLinkedSource,
    service::TemporalService,
    service_limits::TemporalServiceLimits,
};
use andromeda_context_commit::committer::ContextCommitter;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    change_notifier::{ChangeNotifier, SubscriptionMatching},
    config::EntityConfig,
    port::{reader::EntityReader, retry_delay::RetryDelay},
};
use andromeda_jsonld::engine::processor::JsonLdEngine;
use andromeda_linked_retrieval::admission::TemporalJoin;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier, instance_identity::InstanceIdentity};
use andromeda_ngsi_ld_model::temporal::{StreamExtent, TemporalAggregationRow, TemporalHistoryRow, TemporalValue};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_query::{entity_selection::EntitySelection, geo::near_distance_band::NearDistanceBand};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
    /// The entity selections each identifier listing ran under, in order.
    listed_selections: Mutex<Vec<EntitySelection>>,
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
            listed_selections: Mutex::new(Vec::new()),
        }
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
        let instant = "2024-01-01T00:00:00Z".parse::<DateTime<Utc>>().expect("valid instant");
        let Some(targets) = self.links.get(entity_id) else {
            return Vec::new();
        };
        let reading = TemporalHistoryRow {
            attribute_name: "https://example.com/reading".parse::<AttributeName>().expect("valid attribute IRI"),
            attribute_type: AttributeType::Property,
            dataset_id: None,
            value: TemporalValue::Number(JsonNumber::try_from(1.0).expect("a finite number")),
            observed_at: Some(instant),
            recorded_at: instant,
            created_at: Some(instant),
            modified_at: Some(instant),
            instance: InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{entity_id}:reading"))),
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
                instance: InstanceIdentity::Verbatim(uri(&format!("urn:ngsi-ld:Instance:{entity_id}:{target}"))),
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
    TemporalService::new(
        TemporalServicePorts {
            entity_reader: Arc::clone(&broker) as Arc<dyn EntityReader>, // clone: one test double supplies every focused capability
            reader: Arc::clone(&broker) as Arc<dyn TemporalHistoryReader>,
            mutation_barrier: Arc::clone(&broker) as Arc<dyn TemporalMutationBarrier>,
            mutator: Arc::clone(&broker) as Arc<dyn TemporalMutator>,
            writes: Arc::clone(&broker) as Arc<dyn ContextCommitter>, // clone: one test double supplies every focused capability
            retry_delay: broker as Arc<dyn RetryDelay>,
        },
        JsonLdEngine::builder().allow_remote(false).build().expect("build engine"),
        Arc::new(ChangeNotifier::new(None, SubscriptionMatching::Absent)),
        TemporalServiceLimits {
            instance_limits: TemporalInstanceLimits {
                default_page: 100,
                maximum: 1000,
                aggregation_instances: DEFAULT_AGGREGATION_INSTANCE_BUDGET,
            },
            delete_by_query_max_matches: 100,
            aggregation_grouping_max_groups: 100,
            payload_budget: EntityConfig::default().payload_budget(),
            near_band: NearDistanceBand::Refused,
        },
        remote,
        TemporalJoin::Extended,
    )
}

/// The stubbed broker's commits never lose, so it never waits between attempts.
#[async_trait]
impl RetryDelay for StubBroker {
    async fn wait(&self, _delay: Duration) {}
}
