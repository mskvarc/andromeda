//! The transactional notification outbox.
//!
//! A decision Andromeda made, not a resource ETSI GS CIM 009 defines: an entry
//! identifier, a worker's lease over it, a retry counter and a next-attempt
//! instant describe how the broker delivers a notification reliably, not what a
//! notification is. Clause 5.8.1 requires a notification to be sent when a
//! subscription matches; making that survive a crash is the broker's problem.
//!
//! The entries and the port live on the policy side because both the entity
//! write path (which appends) and the delivery worker (which drains) are policy.

/// The entries of the notification outbox, and the values written into one.
pub mod entry;
/// The outbox store port.
pub mod store;
