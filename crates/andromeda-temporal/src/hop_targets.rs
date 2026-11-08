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
//! A target is read from every holder, as the parts of a split entity are:
//! this broker when it records the entity, and every context source whose
//! registration covers it, which is asked for the attributes the hops read of
//! it and the window alone. The parts are merged by Attribute and `datasetId`
//! into one entity. A holder that fails leaves its part out and is reported, so
//! the query is answered with a warning. A hop judges a target on its
//! instances inside the window and on those it holds in effect when the window
//! opens, gathered from every holder, so a target written only before the
//! window is reached, and so is any entity a Relationship in effect names.

use crate::{
    answered_evolution::AnsweredEvolution,
    error::TemporalError,
    hop_target::HopTarget,
    in_effect_entity::InEffectEntity,
    port::read_scope::{ScopeReach, TemporalReadScope},
    read::rendered_entity::HistoryPresence,
    remote_linked::TemporalLinkedWindow,
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_entity::document::{attribute_member::AttributeMember, instance_document::InstanceDocument};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier, temporal_format::TemporalFormat, time_prop::TimeProp};
use andromeda_ngsi_ld_model::temporal::{
    temporal_document::{MergeMode, TemporalDocument},
    temporal_history::TemporalHistory,
};
use andromeda_ngsi_ld_query::{
    projection::attrs_selection::AttrsAdmission,
    queried_entity::linked_entities::LinkedEntities,
    split_filter::HopReads,
    temporal::{compiled::CompiledTemporalQuery, window::TemporalWindow},
};
use andromeda_ngsi_ld_representation::linked_resolution_failure::LinkedResolutionFailure;
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
};

/// The entities a hop reaches, by identifier.
#[derive(Debug, Default)]
pub(crate) struct HopTargets<'a> {
    /// Every target read, as a hop judges it.
    entities: HashMap<&'a str, HopTarget<'a>>,
}

impl<'a> HopTargets<'a> {
    /// The targets `reading` found, read over `window`.
    pub(crate) fn of(reading: &'a HopReading, window: TemporalWindow) -> HopTargets<'a> {
        let mut in_effect: HashMap<&str, Vec<&TemporalDocument>> = HashMap::new();
        for part in &reading.in_effect {
            if let Some(id) = part.id() {
                in_effect.entry(id.as_str()).or_default().push(part);
            }
        }
        let mut entities: HashMap<&str, HopTarget<'a>> = HashMap::new();
        for document in &reading.targets {
            if let Some(id) = document.id() {
                entities.insert(
                    id.as_str(),
                    HopTarget {
                        window: Some(AnsweredEvolution::of(document, window)),
                        in_effect: None,
                    },
                );
            }
        }
        for (id, parts) in in_effect {
            let held = InEffectEntity::merged(&parts, window);
            entities.entry(id).or_insert(HopTarget { window: None, in_effect: None }).in_effect = held;
        }
        HopTargets { entities }
    }
}

impl<'a> LinkedEntities<'a> for &'a HopTargets<'a> {
    type Entity = &'a HopTarget<'a>;

    fn linked(self, object: &str) -> Option<&'a HopTarget<'a>> {
        self.entities.get(object)
    }
}

/// How the targets of one level are read.
pub(crate) struct TargetRead<'r> {
    /// The branch the targets are read from.
    pub(crate) target: &'r ContextTarget,
    /// The read scope this broker's history is read in.
    pub(crate) scope: &'r TemporalReadScope<'r>,
    /// The window as this broker's own history answers it.
    pub(crate) compiled: &'r CompiledTemporalQuery,
    /// The same window as a remote holder is asked for it.
    pub(crate) window: &'r TemporalLinkedWindow,
    /// The attributes whose instance in effect when the window opens the hops
    /// read.
    pub(crate) in_effect: &'r [&'r AttributeName],
    /// The request context.
    pub(crate) context: &'r ContextSpec,
}

/// What reading the entities the hops reach found.
#[derive(Debug, Default)]
pub(crate) struct HopReading {
    /// Every target read over the window, each the union of what its holders
    /// answered.
    pub(crate) targets: Vec<TemporalDocument>,
    /// What every holder answered for the instances the targets hold in
    /// effect when the window opens, unmerged.
    pub(crate) in_effect: Vec<TemporalDocument>,
    /// The holders that could not be consulted or whose answer was unusable;
    /// the targets they hold are judged on the parts that did answer.
    pub(crate) incomplete: Vec<LinkedResolutionFailure>,
}

impl TemporalService {
    /// Read over the window of `params` every entity the hops reach from
    /// `parts`, and what each holds in effect when the window opens, `hops`
    /// naming the Relationships crossed at each depth and `reads` what each
    /// depth is read for.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the window does not compile or this
    /// broker's temporal catalogue cannot be read. A holder that cannot be
    /// consulted is left out and reported in [`HopReading::incomplete`]:
    /// clause 4.5.23 leaves such a Relationship a plain URI, and a hop reaches
    /// what the other holders answered.
    pub(crate) async fn read_hop_targets(
        &self,
        target: &ContextTarget,
        params: &TemporalQueryParameters,
        context: &ContextSpec,
        (hops, reads): (&[Vec<&AttributeName>], &[HopReads<'_>]),
        parts: &[&TemporalDocument],
    ) -> Result<HopReading, TemporalError> {
        let scope = self.reader.read_scope(target, ScopeReach::Every).await?;
        let mut reading = HopReading::default();
        let mut seen: HashSet<Identifier> = HashSet::new();
        let mut frontier = objects_of(parts.iter().copied(), hops.first());
        for depth in 1..=hops.len() {
            frontier.retain(|object| seen.insert(object.clone()));
            if frontier.is_empty() {
                break;
            }
            let level_reads = reads.get(depth - 1);
            // A filter reads instances whatever representation the client
            // asked for, and every instance of the window, before any `lastN`
            // cut; a target is asked only for what the hops read of it.
            let window = TemporalLinkedWindow {
                format: TemporalFormat::Normalized,
                last_n: None,
                aggr_methods: None,
                aggr_period_duration: None,
                attrs: level_reads
                    .and_then(HopReads::projection)
                    .map(|names| names.iter().map(|name| (*name).clone()).collect()),
                ..TemporalLinkedWindow::of_request(params)
            };
            let compiled = self
                .compile_temporal_query(
                    &window.to_parameters(),
                    context,
                    self.language_targeting(),
                    AttrsAdmission::ReifiedScopeAdmitted,
                )
                .await?;
            let read = TargetRead {
                target,
                scope: &scope,
                compiled: &compiled,
                window: &window,
                in_effect: level_reads.map_or(&[][..], |level| level.attributes.as_slice()),
                context,
            };
            let level = self.read_target_level(&read, &frontier, &seen).await?;
            frontier = objects_of(level.targets.iter().chain(&level.in_effect), hops.get(depth));
            reading.targets.extend(level.targets);
            reading.in_effect.extend(level.in_effect);
            for failure in level.incomplete {
                if !reading.incomplete.contains(&failure) {
                    reading.incomplete.push(failure);
                }
            }
        }
        Ok(reading)
    }

    /// Read `ids` over the window, and what they hold in effect when it
    /// opens, from every holder: this broker when it records them, and the
    /// remainder of the system whether or not it does, since clause 5.7.4.4
    /// makes the Temporal Evolution of an Entity the union of what every part
    /// holds. `visited` names every target the walk has read or is reading.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the temporal catalogue cannot be read.
    async fn read_target_level(&self, read: &TargetRead<'_>, ids: &[Identifier], visited: &HashSet<Identifier>) -> Result<HopReading, TemporalError> {
        let catalogued = self.reader.entity_types(read.scope, ids).await?;
        let compactor = self.engine.compactor(read.context);
        let compactor = EntityCompactor::new(&compactor);
        let remote = self.remote_temporal_linked.as_ref().filter(|_| read.target.permits_federation());
        let held: Vec<Identifier> = visited.iter().cloned().collect();
        let mut level = HopReading::default();
        for id in ids {
            let mut parts: Vec<TemporalHistory> = Vec::new();
            if let Some(types) = catalogued.get(id) {
                match self.render_one_temporal_entity(read.scope, id, types, read.compiled, &compactor).await {
                    Ok(rendered) if rendered.presence == HistoryPresence::Present => parts.push(rendered.history),
                    Ok(_) => {}
                    Err(error) => tracing::debug!("Failed to read the temporal evolution of hop target {}: {}", id, error),
                }
            }
            if let Some(remote) = remote {
                match remote.fetch_linked_temporal(id, NonZeroU32::MIN, &held, read.window, read.context).await {
                    Ok(resolution) => {
                        for failure in &resolution.failures {
                            tracing::warn!("Hop target {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
                        }
                        level.incomplete.extend(resolution.failures);
                        parts.extend(resolution.entities);
                    }
                    Err(error) => {
                        tracing::warn!("Failed to read the temporal evolution of hop target {} outside this broker: {}", id, error);
                        level.incomplete.push(LinkedResolutionFailure::unreachable(id, &error));
                    }
                }
            }
            level.targets.extend(union_of(parts, read.window.timeproperty));
        }
        let in_effect = self.read_level_in_effect(read, ids, &catalogued, &level.targets).await?;
        level.in_effect = in_effect.parts;
        level.incomplete.extend(in_effect.incomplete);
        Ok(level)
    }
}

/// The instances of the parts of one or more entities, the parts of an entity
/// merged into one document.
fn union_of(parts: Vec<TemporalHistory>, timeproperty: TimeProp) -> impl Iterator<Item = TemporalDocument> {
    let mut gathered: Vec<TemporalHistory> = Vec::new();
    for part in parts {
        match gathered.iter_mut().find(|held| held.id() == part.id()) {
            Some(held) => held.merge(part, MergeMode::Inclusive, timeproperty),
            None => gathered.push(part),
        }
    }
    gathered.into_iter().filter_map(TemporalHistory::into_instances)
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
        hop_targets::{HopReading, HopTargets, objects_of},
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
        let mut reading = HopReading::default();
        for target in targets {
            reading.targets.push(answered(target).await);
        }
        let selection = free_parking();
        let filter = SplitFilter::of(&selection);
        let linked = HopTargets::of(&reading, noon_to_one());
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
            let reading = HopReading {
                targets: vec![answered(&parking).await, answered(&company(name)).await],
                ..HopReading::default()
            };
            let linked = HopTargets::of(&reading, noon_to_one());
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
