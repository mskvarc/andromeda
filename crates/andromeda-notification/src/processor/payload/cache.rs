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
use andromeda_entity::query::EntityQueryService;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use indexmap::IndexMap;
use iri_rs::IriBuf;
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    num::NonZeroU32,
    slice::from_ref,
    sync::Arc,
};
use tracing::warn;

/// The Linked Entities one notified entity reaches, keyed by identifier in the
/// traversal's first-encounter order (clause 4.5.23.3).
pub(super) type LinkedEntities = Arc<IndexMap<Identifier, Entity>>;

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
}

/// Everything one linked-entity resolution depends on.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct LinkedKey {
    /// The entity whose links are followed.
    entity: Identifier,
    /// How many levels are followed (clause 4.5.23).
    join_level: NonZeroU32,
    /// Whether the linked entities carry their system attributes.
    instants: InstantsKey,
    /// The subscription's `jsonldContext`, `None` for the core context.
    context: Option<IriBuf>,
}

/// Cache shared across every notification produced for one change event.
///
/// The entry guards are never held across an await: a miss computes outside
/// the map and races another notification to insert, the first insert winning
/// and every notification then reading the entry that won.
#[derive(Default)]
pub(crate) struct NotificationCompactionCache {
    renderings: DashMap<RenderingKey, Arc<Entity>>,
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

/// The entities `main_entity` links to, resolved once per change for every
/// notification asking for the same join.
pub(super) async fn resolve_linked_cached(
    target: &ContextTarget,
    entity_query: &EntityQueryService,
    main_entity: &Entity,
    join_level: NonZeroU32,
    sys_attrs: SysAttrsInclusion,
    context: (&ContextSpec, Option<&IriBuf>),
    cache: &NotificationCompactionCache,
) -> Result<LinkedEntities, NotificationError> {
    let (spec, context_iri) = context;
    let key = LinkedKey {
        entity: main_entity.id.clone(), // clone: the key owns the identifier it names
        join_level,
        instants: InstantsKey(sys_attrs),
        context: context_iri.cloned(), // clone: the key owns the context it names
    };
    if let Some(hit) = cache.linked_entities.get(&key) {
        return Ok(Arc::clone(&hit));
    }
    let resolution = entity_query
        .resolve_linked_entities(target, from_ref(main_entity), join_level, sys_attrs, spec, &HashSet::new())
        .await
        .map_err(NotificationError::from)?;
    // A notification has no response to carry a clause 6.3.17 warning on, so an
    // unresolvable Linked Entity is reported to the operator instead of the
    // subscriber, and the notification goes out with the entity simply absent.
    for failure in &resolution.failures {
        warn!(
            entity = %failure.id,
            condition = %failure.kind,
            detail = %failure.detail,
            "linked entity left unresolved in a notification payload"
        );
    }
    let linked = Arc::new(resolution.entities);
    Ok(Arc::clone(&cache.linked_entities.entry(key).or_insert(linked)))
}

#[cfg(test)]
mod tests {
    use crate::processor::payload::{
        cache::{NotificationCompactionCache, NotifiedSource, Reading, RenderingKey},
        selection::Selection,
    };
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
    use serde_json::json;
    use std::sync::Arc;

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
}
