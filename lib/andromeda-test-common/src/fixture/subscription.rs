//! Subscription fixture factories and `with_*` modifier chain.
//!
//! Migrated from the legacy `tests/notification/helpers.rs:252-359` and
//! `tests/subscription/helpers.rs:40-171` builders. Batch 4 ports the full
//! chain (`base_subscription`, `with_time_interval`, `with_watched_attributes`,
//! etc.) from those files into this module.

#![allow(clippy::missing_const_for_fn)]
