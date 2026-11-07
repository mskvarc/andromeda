//! Per-event memoization of notification renderings and linked-entity
//! resolutions.
//!
//! One committed change is notified to every subscription it reaches, and most
//! of them ask for the same thing: the same state of the entity, selected the
//! same way, compacted under the same context. Each distinct rendering is
//! computed once per change and shared, which is why a key names everything a
//! rendering depends on and nothing else.

use crate::{error::NotificationError, processor::payload::selection::Selection};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::query::{EntityQueryService, core_linked_source::CoreLinkedSource, notification_join::report_unresolved};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::{
    graph::{LinkedGraph, RootEdges},
    join::TraversalBounds,
    traversal::traverse,
    typed::TypedEntities,
};
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use iri_rs::IriBuf;
use std::{
    hash::{Hash, Hasher},
    num::NonZeroU32,
    slice::from_ref,
    sync::Arc,
};

/// The Linked Entities one notified entity reaches, as the engine traversed
/// them (clause 4.5.23), shared by every notification of one change.
pub(super) type LinkedEntities = Arc<LinkedGraph<Entity>>;

/// Which state of one change a rendering is read from.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum NotifiedSource {
    /// The state the notification is about: the entity as the write left it,
    /// or as a deletion removed it.
    Notified,
    /// The state before the write.
    Previous,
}

/// The instant a state is read at.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) enum Reading {
    /// As it stands at that instant: an instance past its `expiresAt` is no
    /// longer part of it (clause 4.22).
    StandingAt(DateTime<Utc>),
    /// As it was recorded, every instance included: what an instance held
    /// before a change is reported whatever became of it since.
    AsRecorded,
}

/// The clause 4.8 system-attribute inclusion a rendering was made with, as a
/// key member.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct InstantsKey(SysAttrsInclusion);

// `SysAttrsInclusion` derives no `Hash`, so the key hashes the variant it holds;
// the match names every variant, so a new one cannot hash like another.
impl Hash for InstantsKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let included = match self.0 {
            SysAttrsInclusion::Include => true,
            SysAttrsInclusion::Strip => false,
        };
        included.hash(state);
    }
}

/// Everything one compaction of a notified state depends on.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RenderingKey {
    /// The state compacted.
    source: NotifiedSource,
    /// What of it is compacted.
    selection: Selection,
    /// The subscription's `jsonldContext`, `None` for the core context.
    context: Option<IriBuf>,
    /// Whether the instances' system attributes are written.
    instants: InstantsKey,
    /// The instant the state is read at.
    reading: Reading,
}

impl RenderingKey {
    /// The key of compacting `selection` of `source` under `context`.
    pub(crate) fn new(source: NotifiedSource, selection: Selection, context: Option<&IriBuf>, sys_attrs: SysAttrsInclusion, reading: Reading) -> RenderingKey {
        RenderingKey {
            source,
            selection,
            context: context.cloned(), // clone: the key owns the context it names
            instants: InstantsKey(sys_attrs),
            reading,
        }
    }

    /// What of the state is compacted.
    pub(crate) const fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Whether the instances' system attributes are written.
    pub(crate) const fn sys_attrs(&self) -> SysAttrsInclusion {
        self.instants.0
    }

    /// The instant the state is read at.
    pub(crate) const fn reading(&self) -> Reading {
        self.reading
    }
}

/// Everything one linked-entity resolution depends on.
///
/// Clause 5.7.1.5 follows the Relationships among the "returned Attributes",
/// and what a notification returns of an entity is its subscription's
/// selection (clause 5.8.6: `notification.attributes`, `pick`, `omit` and the
/// `datasetId` instances), so the traversal is keyed on the rendering it
/// starts from rather than on the entity alone: two subscriptions selecting
/// the same instances share one traversal, and two selecting differently
/// never read each other's.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct LinkedKey {
    /// The rendering of the notified entity whose links are followed.
    root: RenderingKey,
    /// How many levels are followed (clause 4.5.23).
    join_level: NonZeroU32,
    /// Whether the linked entities carry their system attributes.
    instants: InstantsKey,
}

impl LinkedKey {
    /// The key of following the links of the rendering `root` names.
    pub(crate) const fn new(root: RenderingKey, join_level: NonZeroU32, sys_attrs: SysAttrsInclusion) -> LinkedKey {
        LinkedKey {
            root,
            join_level,
            instants: InstantsKey(sys_attrs),
        }
    }
}

/// Cache shared across every notification produced for one change event.
///
/// The entry guards are never held across an await: a miss computes outside
/// the map and races another notification to insert, the first insert winning
/// and every notification then reading the entry that won.
#[derive(Default)]
pub(crate) struct NotificationCompactionCache {
    /// Compacted renderings of the notified state, one per distinct key.
    renderings: DashMap<RenderingKey, Arc<Entity>>,
    /// Linked entities resolved for `join`, one traversal per rendering and
    /// join level.
    linked_entities: DashMap<LinkedKey, LinkedEntities>,
}

impl NotificationCompactionCache {
    /// The rendering `key` names, computed by `compact` on its first request.
    ///
    /// # Errors
    ///
    /// Returns the failure of `compact`, which leaves nothing cached.
    pub(crate) async fn rendering<F, C>(&self, key: RenderingKey, compact: C) -> Result<Arc<Entity>, NotificationError>
    where
        C: FnOnce() -> F + Send,
        F: Future<Output = Result<Entity, NotificationError>> + Send,
    {
        if let Some(hit) = self.renderings.get(&key) {
            return Ok(Arc::clone(&hit));
        }
        let rendered = Arc::new(compact().await?);
        Ok(Arc::clone(&self.renderings.entry(key).or_insert(rendered)))
    }
}

/// The entities `main_entity`, the rendering `key` starts from, links to,
/// resolved once per change for every notification asking for the same join
/// of the same rendering.
pub(super) async fn cached_linked_graph(
    target: &ContextTarget,
    entity_query: &EntityQueryService,
    main_entity: &Entity,
    key: LinkedKey,
    context: &ContextSpec,
    cache: &NotificationCompactionCache,
) -> Result<LinkedEntities, NotificationError> {
    if let Some(hit) = cache.linked_entities.get(&key) {
        return Ok(Arc::clone(&hit));
    }
    let source = CoreLinkedSource::new(entity_query, target, key.instants.0, context);
    let bounds = TraversalBounds::of(Some(key.join_level), None);
    let graph = traverse(&source, RootEdges::of::<TypedEntities>(from_ref(main_entity)), &bounds)
        .await
        .map_err(NotificationError::from)?;
    report_unresolved(&graph.failures);
    let linked = Arc::new(graph);
    Ok(Arc::clone(&cache.linked_entities.entry(key).or_insert(linked)))
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::{
        cache::{LinkedKey, NotificationCompactionCache, NotifiedSource, Reading, RenderingKey},
        selected_instances::{AttributeSelection, SelectedInstances},
        selection::{EntityMembers, Selection},
    };
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use serde_json::json;
    use std::{collections::BTreeMap, num::NonZeroU32, sync::Arc};

    fn entity(id: &str) -> Entity {
        serde_json::from_value(json!({"id": id, "type": "Thing"})).expect("entity")
    }

    fn key(sys_attrs: SysAttrsInclusion) -> RenderingKey {
        RenderingKey::new(NotifiedSource::Notified, Selection::Whole, None, sys_attrs, Reading::AsRecorded)
    }

    /// A rendering is computed on its first request and served from the cache
    /// to every later request of the same key; a key differing in anything
    /// the rendering depends on is computed on its own.
    #[tokio::test]
    async fn a_rendering_is_computed_once_per_key() {
        let cache = NotificationCompactionCache::default();
        let first = cache
            .rendering(key(SysAttrsInclusion::Strip), || async { Ok(entity("urn:ngsi-ld:T:1")) })
            .await
            .expect("the first rendering");
        let second = cache
            .rendering(key(SysAttrsInclusion::Strip), || async { Ok(entity("urn:ngsi-ld:T:recomputed")) })
            .await
            .expect("the cached rendering");
        assert!(Arc::ptr_eq(&first, &second), "a cached rendering is not computed again");
        pretty_assertions::assert_eq!(second.id.as_str(), "urn:ngsi-ld:T:1");
        let other = cache
            .rendering(key(SysAttrsInclusion::Include), || async { Ok(entity("urn:ngsi-ld:T:2")) })
            .await
            .expect("another rendering");
        pretty_assertions::assert_eq!(other.id.as_str(), "urn:ngsi-ld:T:2");
    }

    /// The rendering of one attribute of the notified entity, as a
    /// `notification.attributes` list selects it.
    fn listed(attribute: &str) -> RenderingKey {
        let selection = Selection::Narrowed {
            instances: SelectedInstances::from(BTreeMap::from([(attribute.parse().expect("valid attribute name"), AttributeSelection::Whole)])),
            members: EntityMembers::Unscoped,
        };
        RenderingKey::new(NotifiedSource::Notified, selection, None, SysAttrsInclusion::Strip, Reading::AsRecorded)
    }

    /// Clause 5.7.1.5 with clause 5.8.6: the Linked Entities follow the
    /// Relationships a subscription's selection returned, so two subscriptions
    /// selecting different attributes of one entity resolve apart, and two
    /// selecting the same share one resolution.
    #[test]
    fn a_linked_resolution_is_keyed_by_the_rendering_it_starts_from() {
        let one = NonZeroU32::MIN;
        let owner = LinkedKey::new(listed("https://example.com/owner"), one, SysAttrsInclusion::Strip);
        let speed = LinkedKey::new(listed("https://example.com/speed"), one, SysAttrsInclusion::Strip);
        assert_ne!(owner, speed);
        assert_eq!(owner, LinkedKey::new(listed("https://example.com/owner"), one, SysAttrsInclusion::Strip));
        let deeper = NonZeroU32::new(2).expect("a positive depth");
        assert_ne!(owner, LinkedKey::new(listed("https://example.com/owner"), deeper, SysAttrsInclusion::Strip));
        assert_ne!(owner, LinkedKey::new(listed("https://example.com/owner"), one, SysAttrsInclusion::Include));
    }
}
