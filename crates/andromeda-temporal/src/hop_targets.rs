//! The entities a split temporal query's hops reach, read over its window
//! (clauses 4.9 and 5.7.4.4, under `temporal_linked_retrieval`).
//!
//! A Query Term crossing a Relationship hop asks about the entity each object
//! names, and under `temporal_linked_retrieval` it reads the linked entity's
//! instances valid within the window of the roots, as the join renders it. Any
//! part of a split entity may hold the Relationship and any broker the entity
//! it names, so the targets the Relationship instances of every part name are
//! read over the window level by level, as many levels as the deepest hop
//! crosses: this broker's own Temporal Evolutions from the temporal catalogue,
//! every other one through the remote half of the temporal Linked Entity
//! retrieval. The filters then judge each hop in memory over those targets.
//!
//! A target is read from one holder: this broker when it records the entity,
//! the remainder of the system otherwise. Its instances inside the window
//! count; the instance in effect when the window opens is the latest across
//! the target's own parts, which a hop does not gather.

use crate::{
    answered_evolution::AnsweredEvolution,
    error::TemporalError,
    port::read_scope::{ScopeReach, TemporalReadScope},
    read::rendered_entity::HistoryPresence,
    remote_linked::TemporalLinkedWindow,
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_entity::document::{attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, temporal_format::TemporalFormat};
use andromeda_ngsi_ld_model::temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::{
    queried_entity::linked_entities::LinkedEntities,
    temporal::{compiled::CompiledTemporalQuery, window::TemporalWindow},
};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
};

/// The entities a hop reaches, each read over the window, by identifier.
#[derive(Debug, Default)]
pub(crate) struct HopTargets<'a> {
    /// Every target read, as the filters read an answered part.
    entities: HashMap<&'a str, AnsweredEvolution<'a>>,
}

impl<'a> HopTargets<'a> {
    /// The targets `documents` hold, read over `window`.
    pub(crate) fn of(documents: &'a [TemporalDocument], window: TemporalWindow) -> HopTargets<'a> {
        HopTargets {
            entities: documents
                .iter()
                .filter_map(|document| Some((document.id()?.as_str(), AnsweredEvolution::of(document, window))))
                .collect(),
        }
    }
}

impl<'a> LinkedEntities<'a> for &'a HopTargets<'a> {
    type Entity = &'a AnsweredEvolution<'a>;

    fn linked(self, object: &str) -> Option<&'a AnsweredEvolution<'a>> {
        self.entities.get(object)
    }
}

/// How the targets of one level are read.
struct TargetRead<'r> {
    /// The branch the targets are read from.
    target: &'r ContextTarget,
    /// The read scope this broker's history is read in.
    scope: &'r TemporalReadScope<'r>,
    /// The window as this broker's own history answers it.
    compiled: &'r CompiledTemporalQuery,
    /// The same window as a remote holder is asked for it.
    window: &'r TemporalLinkedWindow,
    /// The request context.
    context: &'r ContextSpec,
}

impl TemporalService {
    /// Read over the window of `params` every entity the hops reach from
    /// `parts`, `hops` naming the Relationships crossed at each depth.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the window does not compile or this
    /// broker's temporal catalogue cannot be read. A target whose history
    /// cannot be read is left out: clause 4.5.23 leaves such a Relationship a
    /// plain URI, and a hop reaches nothing through it.
    pub(crate) async fn read_hop_targets(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
        hops: &[Vec<&AttributeName>],
        parts: &[&TemporalDocument],
    ) -> Result<Vec<TemporalDocument>, TemporalError> {
        // A filter reads instances whatever representation the client asked
        // for, and every instance of the window, before any `lastN` cut.
        let window = TemporalLinkedWindow {
            format: TemporalFormat::Normalized,
            last_n: None,
            aggr_methods: None,
            aggr_period_duration: None,
            ..TemporalLinkedWindow::of_request(params)
        };
        let compiled = self.compile_temporal_query(&window.to_parameters(), context, self.language_targeting()).await?;
        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let read = TargetRead {
            target,
            scope: &scope,
            compiled: &compiled,
            window: &window,
            context,
        };
        let mut targets: Vec<TemporalDocument> = Vec::new();
        let mut seen: HashSet<Identifier> = HashSet::new();
        let mut frontier = objects_of(parts.iter().copied(), hops.first());
        for depth in 1..=hops.len() {
            frontier.retain(|object| seen.insert(object.clone())); // clone: the walk remembers every target it has read
            if frontier.is_empty() {
                break;
            }
            let level = self.read_target_level(&read, &frontier, &seen).await?;
            frontier = objects_of(level.iter(), hops.get(depth));
            targets.extend(level);
        }
        Ok(targets)
    }

    /// Read `ids` over the window, from this broker when it records them, from
    /// the remainder of the system otherwise; `visited` names every target the
    /// walk has read or is reading.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the temporal catalogue cannot be read.
    async fn read_target_level(
        &self,
        read: &TargetRead<'_>,
        ids: &[Identifier],
        visited: &HashSet<Identifier>,
    ) -> Result<Vec<TemporalDocument>, TemporalError> {
        let catalogued = self.reader.entity_types(read.scope, ids).await?;
        let compactor = self.engine.compactor(read.context);
        let compactor = EntityCompactor::new(&compactor);
        let remote = self.remote_temporal_linked.as_ref().filter(|_| read.target.permits_federation());
        let held: Vec<Identifier> = visited.iter().cloned().collect(); // clone: a remote holder is told every target already read
        let mut level = Vec::new();
        for id in ids {
            if let Some(types) = catalogued.get(id) {
                match self.render_one_temporal_entity(read.scope, id, types, read.compiled, &compactor).await {
                    Ok(rendered) if rendered.presence == HistoryPresence::Present => level.extend(instances_of(rendered.history)),
                    Ok(_) => {}
                    Err(error) => tracing::debug!("Failed to read the temporal evolution of hop target {}: {}", id, error),
                }
                continue;
            }
            let Some(remote) = remote else {
                continue;
            };
            match remote.fetch_linked_temporal(id, NonZeroU32::MIN, &held, read.window, read.context).await {
                Ok(resolution) => {
                    for failure in &resolution.failures {
                        tracing::warn!("Hop target {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
                    }
                    level.extend(resolution.entities.into_iter().filter_map(instances_of));
                }
                Err(error) => tracing::warn!("Failed to read the temporal evolution of hop target {} outside this broker: {}", id, error),
            }
        }
        Ok(level)
    }
}

/// The instances `history` holds; an aggregate holds none a filter reads.
fn instances_of(history: TemporalHistory) -> Option<TemporalDocument> {
    match history {
        TemporalHistory::Instances(document) => Some(document),
        TemporalHistory::Aggregates(_) => None,
    }
}

/// Every entity the instances of `relationships` in `parts` name, without
/// repetition; none when no hop crosses this depth.
fn objects_of<'a>(parts: impl Iterator<Item = &'a TemporalDocument>, relationships: Option<&Vec<&AttributeName>>) -> Vec<Identifier> {
    let Some(relationships) = relationships else {
        return Vec::new();
    };
    let mut objects: Vec<Identifier> = Vec::new();
    for part in parts {
        for relationship in relationships {
            let named = part
                .entity()
                .attributes()
                .get(relationship.as_str())
                .into_iter()
                .flat_map(AttributeMember::instances)
                .flat_map(InstanceDocument::objects)
                // clone: the walk owns the identifiers it reads
                .filter_map(|object| Identifier::try_from(object.clone()).ok());
            for object in named {
                if !objects.contains(&object) {
                    objects.push(object);
                }
            }
        }
    }
    objects
}

#[cfg(test)]
mod tests {
    use crate::{
        answered_evolution::AnsweredEvolution,
        answered_fixture::answered,
        hop_targets::{HopTargets, objects_of},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        split_filter::SplitFilter,
        temporal::window::{TemporalWindow, TimeInterval},
    };
    use serde_json::{Value, json};

    const PARKED_IN: &str = "https://example.com/parkedIn";

    /// The window from 12:00 to 13:00 on `observedAt`.
    fn noon_to_one() -> TemporalWindow {
        let at = |raw: &str| raw.parse::<NgsiLdDateTime>().expect("a valid instant").get();
        TemporalWindow::builder()
            .timeproperty(TimeProp::Observed)
            .interval(TimeInterval::Between {
                from: at("2020-08-01T12:00:00Z"),
                to: at("2020-08-01T13:00:00Z"),
            })
            .build()
    }

    /// A remote part of Vehicle 1 parked in Parking 1 at 12:10.
    fn parked() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            PARKED_IN: [{"type": "Relationship", "object": "urn:ngsi-ld:Parking:1", "observedAt": "2020-08-01T12:10:00Z"}]
        })
    }

    /// Parking 1 holding `free` spots, observed at `at`.
    fn parking(free: u32, at: &str) -> Value {
        json!({
            "id": "urn:ngsi-ld:Parking:1",
            "type": "Parking",
            "https://example.com/free": [{"type": "Property", "value": free, "observedAt": at}]
        })
    }

    /// `parkedIn{free}>0`.
    fn free_parking() -> EntitySelection {
        EntitySelection::builder()
            .q_expr(Some(
                "\"https://example.com/parkedIn\"{\"https://example.com/free\"}>0".parse().expect("a valid q"),
            ))
            .build()
    }

    /// Whether `parkedIn{free}>0` holds over `part` with the hop reaching
    /// `targets`.
    async fn holds_over(part: &Value, targets: &[Value]) -> bool {
        let part = answered(part).await;
        let mut documents = Vec::new();
        for target in targets {
            documents.push(answered(target).await);
        }
        let selection = free_parking();
        let filter = SplitFilter::of(&selection);
        let linked = HopTargets::of(&documents, noon_to_one());
        filter.holds(&filter.truths_of(&AnsweredEvolution::of(&part, noon_to_one()), &linked))
    }

    /// Clause 4.9 with clause 5.7.4.4: a hop from a remote part reaches the
    /// entity its Relationship names, judged on its instances inside the
    /// window.
    #[tokio::test]
    async fn a_remote_relationship_reaches_a_target_matching_in_window() {
        assert!(holds_over(&parked(), &[parking(3, "2020-08-01T12:20:00Z")]).await);
    }

    /// A target whose window holds no matching instance, one observed only
    /// before the window, or one that could not be read reaches nothing.
    #[tokio::test]
    async fn a_remote_relationship_reaches_nothing_when_the_target_fails() {
        assert!(!holds_over(&parked(), &[parking(0, "2020-08-01T12:20:00Z")]).await);
        assert!(!holds_over(&parked(), &[parking(3, "2020-08-01T11:00:00Z")]).await);
        assert!(!holds_over(&parked(), &[]).await);
    }

    /// The walk starts from the objects every part's Relationship instances
    /// name, once each.
    #[tokio::test]
    async fn the_targets_are_the_objects_the_relationship_names() {
        let first = answered(&parked()).await;
        let second = answered(&parked()).await;
        let parked_in: AttributeName = PARKED_IN.parse().expect("a valid attribute name");
        let level = vec![&parked_in];
        let objects = objects_of([&first, &second].into_iter(), Some(&level));
        pretty_assertions::assert_eq!(objects, vec!["urn:ngsi-ld:Parking:1".parse::<Identifier>().expect("a valid identifier")]);
        pretty_assertions::assert_eq!(objects_of([&first].into_iter(), None), Vec::<Identifier>::new());
    }

    /// Clause 4.9: hops nest, so a two-hop term reaches the entity the
    /// target's own Relationship names, judged over the same window.
    #[tokio::test]
    async fn a_two_hop_term_reaches_the_second_level() {
        let part = answered(&parked()).await;
        let parking = json!({
            "id": "urn:ngsi-ld:Parking:1",
            "type": "Parking",
            "https://example.com/operator": [{"type": "Relationship", "object": "urn:ngsi-ld:Company:1", "observedAt": "2020-08-01T12:20:00Z"}]
        });
        let company = |name: &str| {
            json!({
                "id": "urn:ngsi-ld:Company:1",
                "type": "Company",
                "https://example.com/name": [{"type": "Property", "value": name, "observedAt": "2020-08-01T12:30:00Z"}]
            })
        };
        let selection = EntitySelection::builder()
            .q_expr(Some(
                "\"https://example.com/parkedIn\"{\"https://example.com/operator\"{\"https://example.com/name\"}}==\"Acme\""
                    .parse()
                    .expect("a valid q"),
            ))
            .build();
        let filter = SplitFilter::of(&selection);
        for (name, expected) in [("Acme", true), ("Other", false)] {
            let documents = vec![answered(&parking).await, answered(&company(name)).await];
            let linked = HopTargets::of(&documents, noon_to_one());
            pretty_assertions::assert_eq!(
                filter.holds(&filter.truths_of(&AnsweredEvolution::of(&part, noon_to_one()), &linked)),
                expected,
                "{name}"
            );
        }
    }

    /// Clause 4.5.22: a `ListRelationship` names every object of its list.
    #[tokio::test]
    async fn a_list_relationship_names_every_object() {
        let part = answered(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            PARKED_IN: [{"type": "ListRelationship", "objectList": [{"object": "urn:ngsi-ld:Parking:1"}, {"object": "urn:ngsi-ld:Parking:2"}], "observedAt": "2020-08-01T12:10:00Z"}]
        }))
        .await;
        let parked_in: AttributeName = PARKED_IN.parse().expect("a valid attribute name");
        let level = vec![&parked_in];
        let objects: Vec<String> = objects_of([&part].into_iter(), Some(&level)).iter().map(ToString::to_string).collect();
        pretty_assertions::assert_eq!(objects, ["urn:ngsi-ld:Parking:1", "urn:ngsi-ld:Parking:2"]);
    }
}
