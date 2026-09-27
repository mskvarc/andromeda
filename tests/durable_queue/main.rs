//! Verification of the durable queues a write leaves behind: the notification
//! event/delivery split, the side-effect queue, the Subscription surrogate key
//! the delivery rows carry, and the privacy properties of broker-minted
//! identifiers.
//!
//! Naming convention: `durable_queue_<scenario>_<expected_outcome>`.
//!
//! The queue verifications run against a database this suite owns, built from
//! the same tenant migrations the broker runs: claiming, leasing and reaping
//! address whatever due row the tenant holds, so a second claimant — another
//! suite, or a broker with the delivery worker enabled — would make "exactly
//! one worker holds this row" unobservable. The verifications that are about
//! what a client sees, or about what the broker's own delivery drain does with
//! rows its tenant database queued, run against the configured broker instead.

// Test infrastructure legitimately uses expect/unwrap to fail fast on transport and persistence errors.
#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

#[path = "../support/notification_adapter.rs"]
pub mod notification_adapter;

mod live_delivery_drain;
mod live_side_effect_drain;
mod notification_cleanup;
mod notification_delivery_slots;
mod notification_fan_out;
mod notification_fixture;
mod notification_lease;
mod notification_poison;
mod notification_retry;
mod notification_unreadable_subscription;
mod queue_database;
mod queue_workers;
mod recorded_changes;
mod replay_committer;
mod replay_interruption;
mod side_effect_attempt_ceiling;
mod side_effect_lapsed_release;
mod side_effect_lease;
mod side_effect_replay_settlement;
mod side_effect_shared_document;
mod subscription_surrogate;
mod uuid_privacy;
