//! The loader settings unit tests build an offline loader with.

use crate::{
    engine::options::RemoteContextFetch,
    loader::{cache_lifetime::CacheLifetime, context_lifetimes::ContextLifetimes, loader_settings::LoaderSettings},
};
use andromeda_outbound_http::address_policy::AddressPolicy;
use std::time::Duration;

impl LoaderSettings {
    /// Settings for a loader that may not reach the network, with the
    /// defaults every other setting has.
    pub(crate) fn offline() -> LoaderSettings {
        LoaderSettings::builder()
            .allow_remote(RemoteContextFetch::Deny)
            .timeout(Duration::from_secs(30))
            .max_redirects(5)
            .addresses(AddressPolicy::default())
            .cache_capacity(1024)
            .lifetimes(ContextLifetimes {
                heuristic: CacheLifetime::DEFAULT,
                hosted_revalidation: CacheLifetime::HOSTED_REVALIDATION_DEFAULT,
            })
            .max_context_bytes(2_000_000)
            .build()
    }
}
