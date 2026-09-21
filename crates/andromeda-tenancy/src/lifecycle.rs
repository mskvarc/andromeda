//! Mutating a tenant's persisted administrative state.

use crate::{error::TenancyError, status::TenantStatus};
use andromeda_ngsi_ld_lexicon::tenant::TenantName;
use async_trait::async_trait;

/// Writes a tenant's administrative state to the registry.
///
/// One focused capability, kept apart from [`TenantDatabase`](crate::database::TenantDatabase):
/// where a tenant lives and how it is provisioned is a different concern from
/// flipping the suspend/resume toggle, and the runtime cache depends on the
/// former without ever needing the latter.
#[async_trait]
pub trait TenantLifecycle: Send + Sync + 'static {
    /// Set the tenant's administrative state and stamp its `modifiedAt`.
    #[must_use = "tenant status writes must be awaited"]
    async fn set_status(&self, name: &TenantName, status: TenantStatus) -> Result<(), TenancyError>;
}
