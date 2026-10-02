//! The entities a periodic (`timeInterval`) notification considers (clause
//! 5.8.6): those its subscription's entity selectors select, read from the
//! store a page at a time.

use crate::error::NotificationError;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::port::payload_pages::EntityPayloadPages;
use andromeda_ngsi_ld_entity::entity_state::EntityState;
use andromeda_ngsi_ld_lexicon::entity_type_name::EntityTypeName;
use andromeda_ngsi_ld_model::subscription::Subscription;

/// Rows read per round trip while a periodic notification assembles its
/// candidate set.
///
/// A selector whose type is a boolean expression cannot be pushed into a
/// `WHERE`, so the broker reads every entity in the tenant and decides in
/// memory. Reading that in pages bounds one round trip, and discarding
/// non-matching rows as each page arrives is what bounds what is held.
const INTERVAL_ENTITY_PAGE: u16 = 500;

/// The candidate entities of one periodic notification, read from `pages`.
pub(crate) struct IntervalCandidates<'a> {
    /// The context the entities are read from.
    pub(crate) target: &'a ContextTarget,
    /// The store's paged entity reads.
    pub(crate) pages: &'a dyn EntityPayloadPages,
}

impl IntervalCandidates<'_> {
    /// The entities `sub`'s entity selectors select (clause 5.8.6), every
    /// entity when it names none.
    ///
    /// # Errors
    ///
    /// Returns the store's failure to read a page.
    pub(crate) async fn matching(&self, sub: &Subscription) -> Result<Vec<EntityState>, NotificationError> {
        let Some(selectors) = sub.selection.entities.as_ref() else {
            // No entity selectors — every entity qualifies.
            return self.collect_pages(None, |_| true).await;
        };

        let mut all_entities = Vec::new();
        for selector in selectors {
            // A by-type lookup takes one type, so only a leaf selector can be
            // pushed down; a boolean expression reads every entity and is
            // applied in memory. Passing the expression text as a type name,
            // which is what happened before it was parsed, matched nothing at
            // all.
            let single_type = selector.type_.as_single_type();
            let matched = self
                .collect_pages(single_type, |entity| {
                    if single_type.is_none() && !selector.type_.matches(entity.types().as_slice()) {
                        return false;
                    }
                    let entity_id = entity.id();
                    if let Some(id) = selector.id.as_ref()
                        && entity_id != id
                    {
                        return false;
                    }
                    selector.id_pattern.as_ref().is_none_or(|pattern| pattern.is_match(entity_id.as_str()))
                })
                .await?;
            all_entities.extend(matched);
        }

        Ok(all_entities)
    }

    /// Read entities a page at a time, keeping only those `keep` accepts.
    ///
    /// `type_iri` narrows the read to one entity type when the selector names
    /// exactly one; otherwise every entity is a candidate and the predicate is
    /// the only filter. See [`INTERVAL_ENTITY_PAGE`] for why this pages.
    async fn collect_pages(
        &self,
        entity_type: Option<&EntityTypeName>,
        mut keep: impl FnMut(&EntityState) -> bool,
    ) -> Result<Vec<EntityState>, NotificationError> {
        let limit = i64::from(INTERVAL_ENTITY_PAGE);
        let mut kept = Vec::new();
        let mut offset = 0_i64;
        loop {
            let page = match entity_type {
                Some(type_name) => self
                    .pages
                    .get_entities_by_type(self.target, type_name, Some(limit), Some(offset))
                    .await
                    .map_err(NotificationError::from)?,
                None => self
                    .pages
                    .get_all_entities(self.target, Some(limit), Some(offset))
                    .await
                    .map_err(NotificationError::from)?,
            };
            let exhausted = page.len() < usize::from(INTERVAL_ENTITY_PAGE);
            kept.extend(page.into_iter().filter(|entity| keep(entity)));
            if exhausted {
                return Ok(kept);
            }
            offset += limit;
        }
    }
}
