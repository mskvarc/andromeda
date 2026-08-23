//! A committed entity change paired with its context-space identity.

use crate::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;

/// One entity change routed to consumers within its owning context space.
#[derive(Clone, Debug)]
pub struct ContextChangeEvent {
    /// Context branch in which the mutation committed.
    pub target: ContextTarget,
    /// Semantic entity change emitted by the mutation.
    pub event: ChangeEvent,
}
