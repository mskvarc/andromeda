//! How many dormant tenants a wake pass may load now, and when it may load the
//! next one if none.
//!
//! A pass loads a woken tenant into a free place, or into the place of a
//! runtime it may unload: one no request holds that is past its woken lifetime.
//! A pass that claimed and probed more tenants than that could load none of
//! the rest, so it claims only as many as there is room for, and with no room
//! it claims nothing and sleeps until a place frees.

use crate::{database::TenantDatabase, runtime::TenantRuntimeManager, state_factory::TenantStateFactory};
use std::{
    num::NonZeroUsize,
    time::{Duration, Instant, SystemTime},
};

/// Room for woken tenants in the runtime cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WakeRoom {
    /// This many tenants may be loaded now.
    Free(NonZeroUsize),
    /// No tenant may be loaded now.
    Full(RoomOpening),
}

/// When a full cache next has room for a woken tenant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoomOpening {
    /// When the first woken place of a runtime no request holds may free, as
    /// its woken lifetime ends or it may next be asked whether its work has
    /// run, this long from now.
    In(Duration),
    /// When a request lets go of a runtime: every runtime that holds no woken
    /// lifetime is held by a request.
    OnRelease,
}

impl RoomOpening {
    /// The instant room opens, counted from `now`, or `None` when no instant
    /// is known.
    pub(super) fn at(self, now: SystemTime) -> Option<SystemTime> {
        match self {
            RoomOpening::In(left) => now.checked_add(left),
            RoomOpening::OnRelease => None,
        }
    }
}

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// The room the cache has for woken tenants now.
    pub(super) fn wake_room(&self) -> WakeRoom {
        let now = Instant::now();
        let free = self.config.residency.max_resident.get().saturating_sub(self.runtimes.len());
        let unloadable = self
            .runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && entry.value().is_unloadable(now))
            .count();
        match NonZeroUsize::new(free.saturating_add(unloadable)) {
            Some(places) => WakeRoom::Free(places),
            None => WakeRoom::Full(self.room_for_wake_in().map_or(RoomOpening::OnRelease, RoomOpening::In)),
        }
    }

    /// How long until a runtime that only its woken place keeps from being
    /// unloaded may free that place for a woken tenant, as its woken lifetime
    /// ends or it may next be asked whether its work has run, or `None` when no
    /// runtime holds a woken place.
    pub(super) fn room_for_wake_in(&self) -> Option<Duration> {
        let now = Instant::now();
        self.runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && !entry.value().is_leased())
            .filter_map(|entry| entry.value().place_frees_in(now))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use crate::runtime::wake_room::RoomOpening;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_room_opening_after_a_woken_lifetime_is_that_long_from_now() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        assert_eq!(RoomOpening::In(Duration::from_secs(7)).at(now), Some(now + Duration::from_secs(7)));
        assert_eq!(RoomOpening::OnRelease.at(now), None, "a request's release has no instant");
    }
}
