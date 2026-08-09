//! The narrow persistence port for batched Context resolver hits.

use crate::error::ContextRegistryError;
use andromeda_jsonld::hit_counter::HitDelta;
use std::{future::Future, pin::Pin};

/// Owned future returned by a Context hit store implementation.
pub type ContextHitStoreFuture<'a> = Pin<Box<dyn Future<Output = Result<(), ContextRegistryError>> + Send + 'a>>;

/// Durably applies additive resolver-hit deltas.
pub trait ContextHitStore: Send + Sync + 'static {
    /// Add every pending delta to its stored Context's hit count.
    fn flush_hits<'a>(&'a self, deltas: &'a [HitDelta]) -> ContextHitStoreFuture<'a>;
}

#[cfg(test)]
mod tests {
    use crate::hit_store::ContextHitStoreFuture;

    #[test]
    fn hit_store_future_is_send() {
        fn require_send<T: Send>() {}
        require_send::<ContextHitStoreFuture<'static>>();
    }
}
