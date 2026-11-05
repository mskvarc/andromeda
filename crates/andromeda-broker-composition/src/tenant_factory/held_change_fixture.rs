//! A committed entity change for the tests that hold one in a tenant's
//! pending commits.

use andromeda_context_commit::{change_event::ChangeEvent, context_change_event::ContextChangeEvent, store_instant::StoreInstant};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    entity_change::{EntityChange, EntityLifecycle},
    notification::ChangeOrigin,
};
use chrono::Utc;

/// A modification of one Vehicle on the live branch, committed now.
pub(super) fn committed_change() -> ContextChangeEvent {
    ContextChangeEvent {
        target: ContextTarget::Live,
        event: ChangeEvent::builder()
            .entity_id("urn:ngsi-ld:Vehicle:held".parse::<Identifier>().expect("a valid identifier"))
            .change(EntityChange::new(EntityLifecycle::Modified, Vec::new()))
            .occurred_at(Utc::now())
            .origin(ChangeOrigin::Local)
            .build(),
        committed_at: StoreInstant::new(Utc::now()),
    }
}
