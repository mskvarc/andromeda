//! Preparing a batch whose entities name different contexts.
//!
//! JSON-LD applies a top-level node's `@context` to that node alone, so each
//! entity of an `application/ld+json` batch is expanded under the context it
//! carried itself (clause 5.5.7, "the @context [...] provided by each API
//! call"). Entities naming one and the same context -- one
//! [`BatchContextId`](andromeda_jsonld::batch_contexts::BatchContextId), given
//! when the batch was parsed -- are prepared together, so a batch whose
//! entities agree resolves its context once and keeps the shared fast path, and
//! a batch whose entities differ never reads one entity's terms through
//! another's definitions.

use crate::{
    EntityService,
    batch::prepare::{ClassifiedEntry, PreparedBatch, PreparedEntity},
    null_sentinel_admission::NullSentinelAdmission,
};
use andromeda_jsonld::batch_contexts::ContextGroups;

impl EntityService {
    /// Prepare every group under its own context, answering one slot per
    /// position below `slot_count`.
    ///
    /// Each entry's `idx` is its position in the whole batch, so the groups'
    /// slots interleave back into submission order; the failures come back
    /// group by group, each against its own entity's identifier.
    pub(super) async fn prepare_grouped(&self, slot_count: usize, groups: ContextGroups<ClassifiedEntry>, admission: NullSentinelAdmission) -> PreparedBatch {
        let mut slots: Vec<Option<PreparedEntity>> = (0..slot_count).map(|_| None).collect();
        let mut errors = Vec::new();
        for (context, entries) in groups {
            let group = self.prepare_payloads(slot_count, entries, context.spec(), admission).await;
            for (slot, prepared) in slots.iter_mut().zip(group.slots) {
                if prepared.is_some() {
                    *slot = prepared;
                }
            }
            errors.extend(group.errors);
        }
        PreparedBatch { slots, errors }
    }
}
