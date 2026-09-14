//! Persistence layer for the notification delivery queue.
//!
//! One write produces one immutable `notification_events` row per affected
//! entity, and one `notification_deliveries` row per subscription that matched
//! it. The split is what keeps a fan-out cheap: the entity snapshots a
//! notification carries are written once no matter how many subscribers are due
//! them, and the queue columns a worker updates — the claim triple, the attempt
//! counter, the next-attempt instant — live on the narrow child row that is
//! scanned and rewritten, never on the row holding the payload.
//!
//! A write whose entity matched no subscription persists nothing at all: an
//! event no delivery references would be a payload nobody is waiting for.
//!
//! The SQL decode shape stays private; [`rows`] exposes only records whose
//! domain-valued columns have passed validation.

/// Releasing a claimed delivery back to the queue, or out of it.
pub mod acknowledge;
/// Enumerating the context branches a per-branch delivery drain sweeps.
pub mod branches;
/// Leasing due deliveries and reading the events they answer.
pub mod claim;
/// Reaping delivered rows and the events they leave behind.
pub mod cleanup;
/// Writing one write's notification events and their deliveries.
pub mod event_insert;
/// The queue behind the notification-owned store port.
pub mod queue;
/// Validated notification delivery records and their mapper errors.
pub mod rows;
/// The number `notification_events.trigger` stores a trigger condition as.
pub mod trigger_code;
