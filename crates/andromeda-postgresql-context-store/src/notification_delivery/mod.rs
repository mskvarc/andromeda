//! Persistence layer for the notification delivery queue.
//!
//! One write produces one immutable `notification_events` row per affected
//! entity (its lifecycle and every Attribute instance it touched) and one
//! `notification_deliveries` row per subscription that matched it. The split
//! is what keeps a fan-out cheap: the entity snapshots a notification carries
//! are written once no matter how many subscribers are due them, and the queue columns a worker updates (the claim pair, the attempt
//! counter, the next-attempt instant) live on the narrow child row that is
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
/// The numbers `notification_events` stores one entity change as.
pub(crate) mod change_code;
/// Leasing due deliveries, then reading back the events they answer.
pub mod claim;
/// The events one delivery claim read back, each decoded once.
pub mod claimed_event;
/// Reading the events one claim's deliveries answer.
pub mod claimed_event_read;
/// Reaping delivered rows and the events they leave behind.
pub mod cleanup;
/// Which commit a stored notification event records the instant of.
pub mod event_commit;
/// Writing one write's notification events and their deliveries.
pub mod event_insert;
/// The candidates of a claim that serves the Subscriptions with the fewest deliveries in flight first.
pub(crate) mod fewest_in_flight;
/// The hold a failing endpoint puts on the rest of its Subscription's queue.
pub(crate) mod hold;
/// The queue behind the notification-owned store port.
pub mod queue;
/// Validated notification delivery records and their mapper errors.
pub mod rows;
/// Reading back one stored entity change.
pub(crate) mod stored_change;
