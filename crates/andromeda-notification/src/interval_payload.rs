//! What one periodic (`timeInterval`) notification carries of the entities
//! its subscription matched (clause 5.8.6).
//!
//! The selection is the periodic path's own, since it reads the stored
//! subscription; everything after it -- compaction, system times, the `join`
//! Linked Entities, the format rendering, `pick` / `omit` and the
//! `ngsildConformance` amendment -- is written as a change-driven notification
//! is written.

use crate::{
    error::NotificationError,
    processor::payload::{
        carried_entities::{CarriedShape, carried_entities},
        compaction::compact_notified_state,
        entity_instants::{ExpiryStamp, notified_instants},
        join::{PeriodicJoin, resolve_periodic_join},
        notified_entity::NotifiedEntity,
        selection::{Selection, SelectionRule},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{port::reader::EntityReader, query::EntityQueryService};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::entity_compactor::{EntityCompactor, EntityRead};
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{notification::SysAttrsSnapshot, subscription::Subscription};
use andromeda_ngsi_ld_query::projection::compiled::Projection;
use andromeda_ngsi_ld_render::{notification_entity::TargetSpelling, notified_changes::HeldAttributes, structural_projection::StructuralProjection};
use andromeda_ngsi_ld_representation::{join_mode::JoinMode, sys_attrs::SysAttrsInclusion};
use andromeda_subscription::compilation::notification_projection;
use chrono::Utc;
use serde_json::Value;
use std::{collections::HashMap, sync::Arc};
use tracing::debug;

/// What one tenant's periodic notifications read the entities they carry
/// through.
pub(crate) struct PeriodicPayload<'a> {
    /// The branch the entities are read from.
    pub(crate) target: &'a ContextTarget,
    /// The tenant's JSON-LD processor.
    pub(crate) engine: &'a JsonLdEngine,
    /// Entity system-attribute reads.
    pub(crate) entity_reader: &'a dyn EntityReader,
    /// Linked-entity resolution for `join`.
    pub(crate) entity_query: &'a EntityQueryService,
}

impl PeriodicPayload<'_> {
    /// Render the entities of one periodic notification the way clause 5.8.6
    /// prescribes: narrowed to `notification.attributes` and the
    /// subscription's `datasetId` instances, reduced by `pick` and `omit`,
    /// compacted under the subscription's context, joined to their Linked
    /// Entities and rendered in its `notification.format` -- every step after
    /// the selection written as a change-driven notification's is.
    pub(crate) async fn entities(&self, sub: &Subscription, entities: Vec<EntityState>) -> Result<Vec<Value>, NotificationError> {
        let projection = notification_projection(&sub.notification, sub.id.as_ref());
        let rule = periodic_rule(sub, &projection);
        // Table 5.2.14-1: without `sysAttrs` a notification carries no clause
        // 4.8 system attribute, the instances' included.
        let sys_attrs = match sub.notification.sys_attrs {
            Some(true) => SysAttrsInclusion::Include,
            Some(false) | None => SysAttrsInclusion::Strip,
        };
        let instants = self.entity_instants(&entities, sys_attrs).await;
        let context = ContextSpec::for_subscription_context(sub.delivery.jsonld_context.as_ref());
        let context_compactor = self.engine.compactor(&context);
        let compactor = EntityCompactor::new(&context_compactor);
        let read = EntityRead {
            id_override: None,
            sys_attrs,
            now: Utc::now(),
        };
        let mut roots = Vec::with_capacity(entities.len());
        for entity in &entities {
            let selected = Selection::of(entity, rule).apply(entity);
            roots.push(compact_notified_state(&compactor, self.engine, &selected, &read).await?);
        }
        let linked = match sub.notification.join {
            Some(JoinMode::Flat | JoinMode::Inline) => {
                let join = PeriodicJoin {
                    target: self.target,
                    join_level: sub.notification.effective_join_level(),
                    sys_attrs,
                    context: &context,
                };
                resolve_periodic_join(self.entity_query, &roots, join).await
            }
            Some(JoinMode::None) | None => None,
        };
        let notified: Vec<NotifiedEntity> = roots
            .into_iter()
            .map(|entity| NotifiedEntity {
                stamped: notified_instants(instants.get(&entity.id), ExpiryStamp::Written, None),
                entity: Arc::new(entity),
                held: HeldAttributes::Current,
                deleted: None,
                previous: None,
            })
            .collect();
        let shape = CarriedShape {
            format: sub.notification.format.unwrap_or_default(),
            join: sub.notification.join,
            // A periodic notification spells its Relationship targets as the
            // store holds them.
            targets: TargetSpelling::AsStored,
            projection: StructuralProjection::from(&projection),
            conformance: sub.delivery.ngsild_conformance,
        };
        carried_entities(&notified, linked.as_ref(), shape)
    }

    /// The clause 4.8 system attributes of every entity one periodic
    /// notification carries, read in one round trip when the subscription asks
    /// for them.
    ///
    /// Table 5.2.14.1-1 makes them `sysAttrs`'s alone to decide, so a
    /// `notification.attributes` list leaves them in place.
    ///
    /// A read that fails leaves the entities without them rather than failing
    /// the notification.
    async fn entity_instants(&self, entities: &[EntityState], sys_attrs: SysAttrsInclusion) -> HashMap<Identifier, SysAttrsSnapshot> {
        match sys_attrs {
            SysAttrsInclusion::Include => (),
            SysAttrsInclusion::Strip => return HashMap::new(),
        }
        // clone: the batch read takes the identifiers as one slice
        let ids: Vec<Identifier> = entities.iter().map(|entity| entity.id().clone()).collect();
        match self.entity_reader.get_sys_attrs_batch(self.target, &ids).await {
            Ok(instants) => instants,
            Err(error) => {
                debug!("Failed to fetch the system attributes of a periodic notification: {}", error);
                HashMap::new()
            }
        }
    }
}

/// What a periodic notification selects of every entity it carries (clause
/// 5.8.6): the `notification.attributes`, the instances matching the
/// subscription's `datasetId` -- "For each entity in the notification, only
/// the Attribute instances are to be included that match the datasetId member
/// in Subscription" -- and the attributes `pick` and `omit` keep.
fn periodic_rule<'s>(sub: &'s Subscription, projection: &'s Projection) -> SelectionRule<'s> {
    SelectionRule {
        attributes: sub.notification.attributes.as_deref(),
        dataset_ids: sub.filters.dataset_id.as_deref(),
        projection,
    }
}

#[cfg(test)]
mod tests {
    use crate::{interval_payload::periodic_rule, processor::payload::selection::Selection};
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::subscription::Subscription;
    use andromeda_subscription::compilation::notification_projection;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json};

    const NAME: &str = "https://example.org/name";
    const FRENCH: &str = "urn:ngsi-ld:Dataset:french-name";

    /// A periodic subscription on Buildings, with `extra` merged into it.
    fn subscription(extra: &Value) -> Subscription {
        let mut subscription = json!({
            "id": "urn:ngsi-ld:Subscription:periodic",
            "type": "Subscription",
            "entities": [{"type": "https://example.org/Building"}],
            "timeInterval": 60,
            "notification": {"endpoint": {"uri": "http://subscriber.example.org/notify", "accept": "application/json"}},
            "status": "active",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        });
        if let (Some(target), Some(extra)) = (subscription.as_object_mut(), extra.as_object()) {
            target.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
        serde_json::from_value(subscription).expect("valid subscription")
    }

    /// A Building whose `name` holds a default and a French instance.
    fn building() -> EntityState {
        let instance = |value: &str, dataset: Option<&str>| {
            let mut instance = json!({
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": value}]
            });
            if let Some(dataset) = dataset {
                instance["https://uri.etsi.org/ngsi-ld/datasetId"] = json!([{"@id": dataset}]);
            }
            instance
        };
        let document = json!([{
            "@id": "urn:ngsi-ld:Building:1",
            "@type": ["https://example.org/Building"],
            NAME: [instance("Eiffel Tower", None), instance("Tour Eiffel", Some(FRENCH))]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    fn name_datasets(state: &EntityState) -> Vec<Option<&str>> {
        state
            .attributes()
            .get(NAME)
            .into_iter()
            .flat_map(|held| held.iter().map(|(dataset_id, _instance)| dataset_id.map(Identifier::as_str)))
            .collect()
    }

    /// Clause 5.8.6, periodic notifications: "only the Attribute instances
    /// are to be included that match the datasetId member in Subscription".
    #[test]
    fn a_periodic_notification_carries_only_the_selected_dataset_instances() {
        let sub = subscription(&json!({"datasetId": [FRENCH]}));
        let projection = notification_projection(&sub.notification, sub.id.as_ref());
        let state = building();
        let selected = Selection::of(&state, periodic_rule(&sub, &projection)).apply(&state).into_owned();
        assert_eq!(name_datasets(&selected), vec![Some(FRENCH)]);
    }

    /// Without a `datasetId` the periodic notification carries every instance.
    #[test]
    fn a_periodic_notification_without_a_dataset_selection_carries_every_instance() {
        let sub = subscription(&json!({}));
        let projection = notification_projection(&sub.notification, sub.id.as_ref());
        let state = building();
        assert_eq!(Selection::of(&state, periodic_rule(&sub, &projection)), Selection::Whole);
    }
}
