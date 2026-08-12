//! Durable consequences committed alongside the entity mutation.
//!
//! Each consequence is a distinct destination with its own row shape, so each
//! owns its own module; the committer sequences them inside one transaction and
//! knows nothing of their statements.

pub(crate) mod notification_events;
pub(crate) mod side_effects;
