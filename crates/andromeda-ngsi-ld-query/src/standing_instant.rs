//! The instant a read judges what still stands at (clause 4.22).
//!
//! Clause 4.22 makes an Entity or an attribute instance invalid once its
//! `expiresAt` has passed, so every read of current state decides expiry at
//! some instant. A request reads at the instant the store reads, while an
//! evaluation that reads in several steps and decides the rest in memory fixes
//! one instant for all of them, so a row cannot stand for one step and not for
//! the next.

use chrono::{DateTime, Utc};

/// The instant a read of current state judges expiry at.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum StandingInstant {
    /// The store's own clock as it reads.
    #[default]
    WhenRead,
    /// This instant, whatever the store's clock reads.
    At(DateTime<Utc>),
}
