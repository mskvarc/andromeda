//! Where the index stands against the store, and which queries it has served.

use crate::port::spatial_snapshot::ChangeHorizon;
use std::mem;

/// Where the index stands against the store.
#[derive(Clone, Debug, Default)]
pub(super) enum Standing {
    /// No rebuild has seeded the index. It narrows no query, and nothing is
    /// caught up.
    #[default]
    Unseeded,
    /// The index holds every change below the horizon. A query takes in the
    /// changes above it itself, when they are few enough.
    Current(ChangeHorizon),
    /// The index holds every change below the horizon, and more changes are
    /// waiting above it than a query takes in. No query narrows or writes the
    /// index until the background catch-up has taken them in.
    Behind(ChangeHorizon),
}

/// Where the index stands, and which queries a catch-up has served.
#[derive(Debug, Default)]
pub(super) struct WatermarkState {
    /// Where the index stands against the store.
    pub(super) standing: Standing,
    /// The arrival count at the start of the newest completed catch-up's read.
    /// Every query that arrived at or below it has been served.
    pub(super) served_through: u64,
}

impl WatermarkState {
    /// Record a catch-up whose read started at arrival `round` and was taken
    /// at `horizon`.
    pub(super) fn caught_up(&mut self, horizon: ChangeHorizon, round: u64) {
        self.standing = Standing::Current(horizon);
        self.served_through = self.served_through.max(round);
    }

    /// Record that more changes wait above the horizon than a query takes in.
    pub(super) fn fell_behind(&mut self) {
        self.standing = match mem::take(&mut self.standing) {
            Standing::Current(horizon) | Standing::Behind(horizon) => Standing::Behind(horizon),
            Standing::Unseeded => Standing::Unseeded,
        };
    }

    /// Record the first rebuild, whose read started at arrival `round` and was
    /// taken at `horizon`. A seeded index keeps its horizon, which a rebuild
    /// never moves.
    pub(super) fn seeded(&mut self, horizon: ChangeHorizon, round: u64) {
        match self.standing {
            Standing::Unseeded => self.caught_up(horizon, round),
            Standing::Current(_) | Standing::Behind(_) => {}
        }
    }
}
