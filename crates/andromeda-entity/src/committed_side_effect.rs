//! The change events a committed durable side effect publishes once its
//! acknowledgement is confirmed.
//!
//! A durable side-effect delivery commits its temporal writes and notification
//! events idempotently (the replay collides with the first attempt's rows on
//! their `ON CONFLICT ... DO NOTHING` constraints), but the in-memory change-sink
//! republication has no such guard: it is a local fan-out, not a durable insert.
//! Publishing before the acknowledgement is confirmed would therefore let a job
//! whose commit succeeded and whose acknowledgement was then lost to a lease
//! reclaim be redelivered, with both workers publishing and a duplicate
//! notification reaching a live receiver. Holding the change events here until
//! the acknowledgement returns [`LeaseAcknowledgement::Applied`](andromeda_context_commit::lease_acknowledgement::LeaseAcknowledgement::Applied)
//! narrows the best-effort live channel to a single publication while leaving
//! the durable at-least-once guarantee untouched.

use andromeda_context_commit::change_event::ChangeEvent;
use andromeda_context_space::context_target::ContextTarget;

/// The change events a committed side effect must publish, and the branch they
/// belong to, deferred until the delivery's acknowledgement is confirmed applied.
#[derive(Debug)]
pub(crate) struct CommittedSideEffect {
    /// Context branch every change event is published against.
    pub(crate) target: ContextTarget,
    /// Change events to publish once the acknowledgement is confirmed applied.
    pub(crate) change_events: Vec<ChangeEvent>,
}
