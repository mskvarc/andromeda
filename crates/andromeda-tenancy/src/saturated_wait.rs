//! The wait a request was shed at when its tenant was refused as saturated.

use strum::{Display, IntoStaticStr};

/// Where a request was waiting when it was refused as saturated.
///
/// Rendered as a kebab-case value in the operator log line and as the `wait`
/// label of the shed-request counter. It is never disclosed to the client,
/// whose answer reads the same whatever the wait.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Display, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum SaturatedWait {
    /// The tenant's single-flight build lock, held by another request building
    /// the same runtime, past the build admission deadline.
    BuildLock,
    /// A build permit, every one of them held by cold builds of other tenants,
    /// past the build admission deadline.
    BuildPermit,
    /// A teardown slot to release a cached runtime and make room for this one,
    /// past the build admission deadline.
    TeardownSlot,
    /// A platform pool connection to read the tenant's registry row.
    RegistryRead,
    /// A platform pool connection to write the tenant's registry row: settling
    /// or retiring a provisional tenant, or changing its status.
    RegistryWrite,
    /// A platform pool connection for the transaction that registers a new
    /// tenant as provisional.
    RegistrationTransaction,
    /// The one-connection pool a new tenant database's migrations run over.
    MigrationPool,
    /// A connection the runtime build acquires: from the tenant's serving pool,
    /// or from the direct pool a schema behind this binary is migrated over.
    RuntimePool,
}

#[cfg(test)]
mod tests {
    use crate::saturated_wait::SaturatedWait;

    #[test]
    fn saturated_waits_render_as_kebab_case_values() {
        for (wait, expected) in [
            (SaturatedWait::BuildLock, "build-lock"),
            (SaturatedWait::BuildPermit, "build-permit"),
            (SaturatedWait::TeardownSlot, "teardown-slot"),
            (SaturatedWait::RegistryRead, "registry-read"),
            (SaturatedWait::RegistryWrite, "registry-write"),
            (SaturatedWait::RegistrationTransaction, "registration-transaction"),
            (SaturatedWait::MigrationPool, "migration-pool"),
            (SaturatedWait::RuntimePool, "runtime-pool"),
        ] {
            let label: &'static str = wait.into();
            pretty_assertions::assert_eq!(label, expected);
            pretty_assertions::assert_eq!(wait.to_string(), expected);
        }
    }
}
