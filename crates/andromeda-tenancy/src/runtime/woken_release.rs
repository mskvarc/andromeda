//! Freeing the places of woken runtimes whose due work has run, for a wake
//! pass that finds every place taken.
//!
//! A full cache asks its woken runtimes, the one woken longest ago first,
//! whether their work has run, and each that has gives up its place, until as
//! many places are free as one pass may fill. A cache with room asks nothing:
//! a woken runtime then keeps its place for its whole lifetime, and with it
//! any periodic notification falling due in that time.

use crate::{
    database::TenantDatabase,
    due_work::DueWork,
    runtime::{TenantRuntimeManager, woken_residency::WokenResidency},
    state_factory::TenantStateFactory,
};
use std::{sync::Arc, time::Instant};

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Ask the woken runtimes no request holds, the one woken longest ago
    /// first, whether their due work has run, until `wanted` of them have
    /// given up their places, returning how many did.
    ///
    /// A runtime asked recently that answered its work was outstanding is not
    /// asked again before its wait ends. Each answer is read outside the cache
    /// guards, so a slow answer holds up no request.
    pub(super) async fn release_drained_woken(&self, wanted: usize) -> usize {
        let now = Instant::now();
        let mut askable: Vec<Arc<WokenResidency>> = self
            .runtimes
            .iter()
            .filter(|entry| !entry.key().is_unnamed_default() && !entry.value().is_leased())
            .filter_map(|entry| entry.value().woken_residency().filter(|woken| woken.may_ask(now)).map(Arc::clone))
            .collect();
        askable.sort_by_key(|woken| woken.end());
        let mut released = 0;
        for woken in askable {
            if released == wanted {
                break;
            }
            match woken.ask().await {
                DueWork::Drained => released += 1,
                DueWork::Outstanding => {}
            }
        }
        released
    }
}
