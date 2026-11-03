//! Reading the runtimes the cache holds open.

use crate::{database::TenantDatabase, runtime::TenantRuntimeManager, state_factory::TenantStateFactory, tenant_ref::TenantRef};

impl<D: TenantDatabase, F: TenantStateFactory> TenantRuntimeManager<D, F> {
    /// Read something from each runtime held open right now, with the tenant it belongs to.
    ///
    /// A runtime evicted or being torn down is not held and is not read. `read`
    /// runs under the cache's shard lock, so it answers from memory and never
    /// waits.
    pub fn read_resident<T>(&self, read: impl Fn(&F::Runtime) -> T) -> Vec<(TenantRef, T)> {
        self.runtimes.iter().map(|entry| (entry.key().clone(), read(&entry.value().runtime))).collect() // clone: the report owns the tenant reference after the shard lock is released.
    }
}
