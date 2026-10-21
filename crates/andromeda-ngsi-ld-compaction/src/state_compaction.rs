//! Compacting the entity node of entity state (clause 5.2.4).
//!
//! The stored document's one node carries the entity's `@id` and `@type`, its
//! `scope` (clause 4.18) as plain string value objects, its `expiresAt`
//! (clause 4.22) as a `DateTime`, and its attributes.

use crate::{
    compacted::CompactedObject,
    expanded_item::{ExpandedItem, node_identifier},
    reserved_terms::reserved,
    undecidable::Undecidable,
    walk::Walk,
};
use andromeda_ngsi_ld_entity::{entity_state::EntityState, value_mode::Stored};
use andromeda_ngsi_ld_lexicon::time::written_date_time::DateTimeText;
use andromeda_ngsi_ld_representation::sys_attrs::SysAttrsInclusion;
use jsonld::Term;

impl Walk<'_> {
    /// Compact the entity node of `state`, writing the clause 4.8 times of its
    /// top-level instances as `system_times` says.
    pub(crate) fn compact_state<'a>(&mut self, state: &'a EntityState, system_times: SysAttrsInclusion) -> Result<CompactedObject<'a>, Undecidable> {
        let mut result = CompactedObject::new();
        let id = Term::Id(node_identifier(state.id().as_str()));
        let types: Vec<_> = state.types().as_slice().iter().map(|ty| Term::Id(node_identifier(ty.as_str()))).collect();
        self.write_identity(&mut result, Some(&id), &types)?;
        if let Some(scope) = state.scope() {
            let paths: Vec<ExpandedItem<'a, Stored>> = scope.as_slice().iter().map(|path| ExpandedItem::text(path.as_str())).collect();
            self.compact_property(&mut result, &reserved().scope, &paths)?;
        }
        self.compact_date_time(&mut result, &reserved().expires_at, state.expires_at().map(DateTimeText::as_str))?;
        self.compact_stored_attributes(&mut result, state.attributes(), system_times)?;
        Ok(result)
    }
}
