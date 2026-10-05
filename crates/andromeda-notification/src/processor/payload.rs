//! Notification payload building: the notified entity's selection and
//! compaction, what the notification reports of the change beside it, its
//! `join` and its format rendering -- every part of it typed, and written by
//! the walk a query response is written by.

use crate::{
    error::NotificationError,
    processor::payload::{
        cache::{NotificationCompactionCache, NotifiedSource, Reading, RenderingKey},
        carried_entities::{CarriedShape, NotifiedLinks, carried_entities},
        compaction::compact_notified_state,
        entity_instants::{ExpiryStamp, notified_instants},
        join::{linked_attribute_index, notified_linked_graph},
        notified_entity::{DeletedRendering, NotifiedEntity},
        selection::{Selection, SelectionRule},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::EntityQueryService;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_linked_retrieval::join::RetrievingJoin;
use andromeda_ngsi_ld_compaction::entity_compactor::{EntityCompactor, EntityRead};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, reads::attribute_change::AttributeChangeKind};
use andromeda_ngsi_ld_model::{
    entity::Entity,
    entity_change::{EntityChange, EntityLifecycle},
    notification::SysAttrsSnapshot,
};
use andromeda_ngsi_ld_render::{notification_entity::TargetSpelling, notified_changes::HeldAttributes, structural_projection::StructuralProjection};
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use andromeda_subscription::compiled::CompiledSubscription;
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use serde_json::Value;
use std::{slice::from_ref, sync::Arc};
use tracing::debug;

/// Everything one subscription's notification of one change is built from.
pub(super) struct NotificationBuildParams<'a> {
    /// The branch the change was committed on.
    pub(super) target: &'a ContextTarget,
    /// The tenant's JSON-LD processor.
    pub(super) engine: &'a JsonLdEngine,
    /// The entity the notification is about: as the write left it, or as a
    /// deletion removed it.
    pub(super) notified: &'a EntityState,
    /// The entity before the write, for deleted instances and `showChanges`.
    pub(super) previous: Option<&'a EntityState>,
    /// The entity's own clause 4.8 system attributes, when resolved.
    pub(super) sys_attrs: Option<&'a SysAttrsSnapshot>,
    /// The subscription notified.
    pub(super) sub: &'a CompiledSubscription,
    /// What the write did to the Entity: its lifecycle and every Attribute
    /// instance it touched, all of which the one notification reports.
    pub(super) change: &'a EntityChange,
    /// When the change was committed, the deletion instant of a deletion.
    pub(super) occurred_at: DateTime<Utc>,
    /// The instant every notification of the change reads the entity at.
    pub(super) now: DateTime<Utc>,
    /// Linked-entity resolution for `join`.
    pub(super) entity_query: &'a EntityQueryService,
    /// Renderings and resolutions shared by every notification of the change.
    pub(super) cache: &'a NotificationCompactionCache,
}

/// The selection a subscription applies to every entity it is notified of.
pub(crate) fn subscription_rule(sub: &CompiledSubscription) -> SelectionRule<'_> {
    SelectionRule {
        list: sub.list_in_force(),
        dataset_ids: sub.dataset_id.as_deref(),
    }
}

/// The entities one notification carries for a subscription (clause 5.8.6).
///
/// The entity the change supplies is reduced to what the subscription selects,
/// compacted under the subscription's context, given what the notification
/// reports of the change beside it and its `join` Linked Entities, rendered in
/// the subscription's `notification.format` and finally amended to its
/// declared `ngsildConformance`. An entity that cannot be compacted under any
/// context leaves nothing to notify, which is an empty payload.
pub(super) async fn build_notification_data(params: &NotificationBuildParams<'_>) -> Vec<Value> {
    let context = ContextSpec::for_subscription_context(params.sub.jsonld_context.as_ref());
    let context_compactor = params.engine.compactor(&context);
    let compactor = EntityCompactor::new(&context_compactor);
    let rendering = Rendering {
        cache: params.cache,
        engine: params.engine,
        context: params.sub.jsonld_context.as_ref(),
        compactor: &compactor,
    };
    let notified = match notified_entity(params, &rendering).await {
        Ok(notified) => notified,
        Err(error) => {
            debug!(%error, subscription = %params.sub.id, "the notified entity could not be compacted");
            return vec![];
        }
    };
    match notification_payload(&notified, params, &context).await {
        Ok(payload) => payload,
        Err(error) => {
            debug!(%error, subscription = %params.sub.id, "the notified entity could not be written");
            vec![]
        }
    }
}

/// The entities a notification carries: the notified entity, with the Linked
/// Entities of a `join` (clause 4.5.23), written as [`carried_entities`]
/// writes every notification's entities.
///
/// An entity deletion resolves no join. Without `showChanges` clause 5.8.6
/// provides "only the deletedAt system property" of the deleted entity, and
/// with it every Attribute, each Relationship among them, is reported as the
/// NGSI-LD Null, which names no target.
async fn notification_payload(
    rooted: &RootedNotification,
    params: &NotificationBuildParams<'_>,
    context: &ContextSpec,
) -> Result<Vec<Value>, NotificationError> {
    let sub = params.sub;
    let list = sub.list_in_force();
    let linked = match (RetrievingJoin::from_mode(sub.notification_join), params.change.lifecycle) {
        (Some(_), EntityLifecycle::Created | EntityLifecycle::Modified) => notified_linked_graph(&rooted.notified.entity, &rooted.root, params, context).await,
        (Some(_), EntityLifecycle::Deleted) | (None, EntityLifecycle::Created | EntityLifecycle::Modified | EntityLifecycle::Deleted) => None,
    };
    let indexed = match &linked {
        Some(graph) => linked_attribute_index(params.engine, context, list, from_ref(&*rooted.notified.entity).iter(), graph)
            .await
            .map(|index| (graph, index)),
        None => None,
    };
    let shape = CarriedShape {
        format: sub.notification_format.unwrap_or_default(),
        join: sub.notification_join,
        targets: TargetSpelling::Decoded,
        projection: StructuralProjection::from(&sub.notification_projection),
        list,
        conformance: sub.ngsild_conformance,
    };
    let links = indexed.as_ref().map(|(graph, index)| NotifiedLinks { graph, index });
    carried_entities(from_ref(&rooted.notified), links, shape)
}

/// The notified entity, and the key of the rendering it was written from,
/// which the Linked Entities of its `join` are resolved for.
struct RootedNotification {
    /// The notified entity.
    notified: NotifiedEntity,
    /// The key of its rendering.
    root: RenderingKey,
}

/// One subscription's compaction of the states of one change.
struct Rendering<'r, 'c> {
    /// Renderings shared by every notification of the change.
    cache: &'r NotificationCompactionCache,
    /// The tenant's JSON-LD processor, for the core-context fallback.
    engine: &'r JsonLdEngine,
    /// The subscription's `jsonldContext`.
    context: Option<&'r IriBuf>,
    /// The compactor of the subscription's context.
    compactor: &'r EntityCompactor<'c>,
}

impl Rendering<'_, '_> {
    /// The key of compacting `selection` of the `source` state under the
    /// subscription's context.
    fn key(&self, source: NotifiedSource, selection: Selection, reading: Reading, sys_attrs: SysAttrsInclusion) -> RenderingKey {
        RenderingKey::new(source, selection, self.context, sys_attrs, reading)
    }

    /// The compact rendering `key` names of `state`, shared with every
    /// notification of the change asking for the same.
    async fn render(&self, key: &RenderingKey, state: &EntityState) -> Result<Arc<Entity>, NotificationError> {
        let (compactor, engine) = (self.compactor, self.engine);
        // clone: the cache owns the key of every rendering it holds, and the caller keeps its own
        self.cache
            .rendering(key.clone(), move || async move {
                let selected = key.selection().apply(state);
                let now = match key.reading() {
                    Reading::StandingAt(now) => now,
                    Reading::AsRecorded => DateTime::<Utc>::MIN_UTC,
                };
                let read = EntityRead {
                    id_override: None,
                    sys_attrs: key.sys_attrs(),
                    now,
                };
                compact_notified_state(compactor, engine, &selected, &read).await
            })
            .await
    }
}

/// The notified entity, with what the notification reports of the change
/// beside it (clause 5.8.6).
///
/// A deleted entity is carried as its identity alone, or, under `showChanges`,
/// with every Attribute reported deleted so the subscriber sees what went --
/// whichever clause 5.2.12 trigger matched the subscription, since an entity
/// deletion reaches an `attributeDeleted` subscriber too and owes it the same
/// `deletedAt`-only document. The Attribute instances a surviving entity lost
/// are reported beside the ones it holds.
///
/// Clause 5.8.6 stamps every entity deletion with `deletedAt`, and Table
/// 5.2.14.1-1 gives a subscription asking for `sysAttrs` the entity's own
/// `createdAt` and `modifiedAt`; neither depends on which attributes the
/// subscription selects, so a `notification.attributes` list changes neither.
async fn notified_entity(params: &NotificationBuildParams<'_>, rendering: &Rendering<'_, '_>) -> Result<RootedNotification, NotificationError> {
    let sub = params.sub;
    let rule = subscription_rule(sub);
    let standing = Reading::StandingAt(params.now);
    let instants = match sub.notification_sys_attrs {
        SysAttrsInclusion::Include => params.sys_attrs,
        SysAttrsInclusion::Strip => None,
    };
    Ok(match params.change.lifecycle {
        EntityLifecycle::Deleted if !sub.notification_show_changes => {
            let root = rendering.key(NotifiedSource::Notified, Selection::identity(), standing, SysAttrsInclusion::Strip);
            let notified = NotifiedEntity {
                entity: rendering.render(&root, params.notified).await?,
                stamped: notified_instants(instants, ExpiryStamp::Omitted, Some(params.occurred_at)),
                held: HeldAttributes::Current,
                deleted: None,
                previous: None,
            };
            RootedNotification { notified, root }
        }
        EntityLifecycle::Deleted => {
            let root = rendering.key(
                NotifiedSource::Notified,
                Selection::of(params.notified, rule),
                standing,
                sub.notification_sys_attrs,
            );
            let notified = NotifiedEntity {
                entity: rendering.render(&root, params.notified).await?,
                stamped: notified_instants(instants, ExpiryStamp::Written, Some(params.occurred_at)),
                held: HeldAttributes::Deleted,
                deleted: None,
                previous: previous_rendering(params, rendering).await,
            };
            RootedNotification { notified, root }
        }
        EntityLifecycle::Created | EntityLifecycle::Modified => {
            let root = rendering.key(
                NotifiedSource::Notified,
                Selection::of(params.notified, rule),
                standing,
                sub.notification_sys_attrs,
            );
            let notified = NotifiedEntity {
                entity: rendering.render(&root, params.notified).await?,
                stamped: notified_instants(instants, ExpiryStamp::Written, None),
                held: HeldAttributes::Current,
                deleted: if params.change.deletes_instances() {
                    deleted_rendering(params, rendering, rule).await?
                } else {
                    None
                },
                previous: previous_rendering(params, rendering).await,
            };
            RootedNotification { notified, root }
        }
    })
}

/// The Attribute instances a change removed, so the notification carries
/// what was deleted and not only what remains.
///
/// A subscription with `watchedAttributes` is told about those attributes
/// alone, so the deleted instances are narrowed to them; a subscription asking
/// for `sysAttrs` has each stamped with the change's `deletedAt`.
async fn deleted_rendering(
    params: &NotificationBuildParams<'_>,
    rendering: &Rendering<'_, '_>,
    rule: SelectionRule<'_>,
) -> Result<Option<DeletedRendering>, NotificationError> {
    let sub = params.sub;
    let Some(previous) = params.previous else {
        return Ok(None);
    };
    let deleted = params
        .change
        .instances
        .iter()
        .filter(|instance| instance.kind == AttributeChangeKind::Deleted)
        .filter(|instance| sub.watched_attrs.as_ref().is_none_or(|watched| watched.contains(&instance.key.attr)))
        .map(|instance| &instance.key);
    let Some(selection) = Selection::deleted(previous, params.notified, deleted, rule) else {
        return Ok(None);
    };
    let key = rendering.key(NotifiedSource::Previous, selection, Reading::AsRecorded, sub.notification_sys_attrs);
    let instances = rendering.render(&key, previous).await?;
    let deleted_at = match sub.notification_sys_attrs {
        SysAttrsInclusion::Include => Some(params.occurred_at),
        SysAttrsInclusion::Strip => None,
    };
    Ok(Some(DeletedRendering { instances, deleted_at }))
}

/// The `showChanges` view of the change (clause 5.8.6): the previous state of
/// every instance the change touched -- every instance, when the Entity itself
/// was deleted -- whose value the notification reports beside the current one.
///
/// A previous state that cannot be compacted leaves the notification with its
/// current values alone rather than failing the delivery.
async fn previous_rendering(params: &NotificationBuildParams<'_>, rendering: &Rendering<'_, '_>) -> Option<Arc<Entity>> {
    if !params.sub.notification_show_changes {
        return None;
    }
    let previous = params.previous?;
    let selection = match params.change.lifecycle {
        EntityLifecycle::Deleted => Selection::Whole,
        EntityLifecycle::Created | EntityLifecycle::Modified => Selection::touched(previous, params.change),
    };
    let key = rendering.key(NotifiedSource::Previous, selection, Reading::AsRecorded, SysAttrsInclusion::Strip);
    match rendering.render(&key, previous).await {
        Ok(previous) => Some(previous),
        Err(error) => {
            debug!(%error, "the previous state of a showChanges notification could not be compacted");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::{
        Rendering,
        cache::{NotificationCompactionCache, NotifiedSource, Reading},
        selection::{EntityMembers, Selection},
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_model::attribute::instance::{Attribute, AttributeValue};
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use chrono::Utc;
    use serde_json::json;
    use std::sync::Arc;

    /// A Vehicle with a `speed`.
    fn vehicle() -> EntityState {
        let document = json!([{
            "@id": "urn:ngsi-ld:Vehicle:1",
            "@type": ["https://example.com/Vehicle"],
            "https://example.com/speed": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 80}]
            }]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored entity decodes")
    }

    /// Design 5.3: one change is rendered once per state, selection and
    /// context, and that rendering serves every subscription asking for the
    /// same, each subscription compacting through its own compactor.
    #[tokio::test]
    async fn one_rendering_serves_every_subscription_with_the_same_context() {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds");
        let cache = NotificationCompactionCache::default();
        let state = vehicle();
        let reading = Reading::StandingAt(Utc::now());
        let core = ContextSpec::for_subscription_context(None);

        let first_context = engine.compactor(&core);
        let first_compactor = EntityCompactor::new(&first_context);
        let first_rendering = Rendering {
            cache: &cache,
            engine: &engine,
            context: None,
            compactor: &first_compactor,
        };
        let whole = first_rendering.key(NotifiedSource::Notified, Selection::Whole, reading, SysAttrsInclusion::Strip);
        let first = first_rendering.render(&whole, &state).await.expect("the first subscription's rendering");

        let second_context = engine.compactor(&core);
        let second_compactor = EntityCompactor::new(&second_context);
        let second_rendering = Rendering {
            cache: &cache,
            engine: &engine,
            context: None,
            compactor: &second_compactor,
        };
        let second = second_rendering.render(&whole, &state).await.expect("the second subscription's rendering");
        assert!(Arc::ptr_eq(&first, &second), "one rendering serves both subscriptions");
        // No term names the attribute under the core context, so it keeps its IRI.
        let Some(AttributeValue::Single(speed)) = first.attrs.get("https://example.com/speed") else {
            panic!("the speed is one instance: {first:?}");
        };
        let Attribute::Property(speed) = speed.as_ref() else {
            panic!("the speed is a Property: {speed:?}");
        };
        pretty_assertions::assert_eq!(serde_json::to_value(&speed.value).expect("a written value"), json!(80));

        let identity_key = second_rendering.key(NotifiedSource::Notified, Selection::identity(), reading, SysAttrsInclusion::Strip);
        let identity = second_rendering.render(&identity_key, &state).await.expect("another selection's rendering");
        assert!(!Arc::ptr_eq(&first, &identity), "another selection is rendered on its own");
        assert!(identity.attrs.is_empty(), "{identity:?}");
        pretty_assertions::assert_eq!(identity.id.as_str(), "urn:ngsi-ld:Vehicle:1");
        assert!(matches!(
            Selection::identity(),
            Selection::Narrowed {
                members: EntityMembers::Dropped,
                ..
            }
        ));
    }
}

/// Per-event memoization of renderings and linked-entity resolutions.
pub(super) mod cache;
/// The entities one notification carries, joined and amended.
pub(crate) mod carried_entities;
/// Compaction shared by both notification paths.
pub(crate) mod compaction;
/// The entity's own clause 4.8 system attributes in a notified entity.
pub(crate) mod entity_instants;
/// The Linked Entities a subscription's `join` reaches.
pub(crate) mod join;
/// A notified entity on its way into the subscription's format.
pub(crate) mod notified_entity;
/// What a notification reports of each kind of change, as the typed walk writes it.
#[cfg(test)]
mod notified_entity_tests;
/// What of a notified entity state one notification carries.
pub(crate) mod selection;
