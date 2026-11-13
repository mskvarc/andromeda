//! The compact codes the last sealed release shipped, held frozen by every
//! registry.
//!
//! A stable release ships every code its registries assign when it is sealed,
//! and rows written with those codes outlive it. Each registry names the
//! release its frozen prefix was last brought up to, so a registry that still
//! names an earlier release than the ledger's last one has not frozen what
//! that release shipped. This fails on the commit that seals the release,
//! before it can be tagged.

use crate::repository::{PinError, repository_root};
use andromeda_postgresql_codes::registry_freeze::RegistryFreeze;
use andromeda_sql::seal_ledger::SealLedger;

#[test]
fn every_compact_code_registry_is_frozen_at_the_last_sealed_release() -> Result<(), PinError> {
    let ledger = SealLedger::load(&repository_root())?;
    let latest = ledger.latest_release().ok_or(PinError::NothingSealed)?.version();
    let stale: Vec<String> = RegistryFreeze::every()
        .filter(|freeze| freeze.frozen_at() != latest.as_ref())
        .map(|freeze| {
            let release = latest.as_ref();
            format!(
                "{} is frozen at v{} while {latest} is sealed; set frozen_at to ({}, {}, {}), frozen_prefix to {} and frozen_digest to {:#018x}",
                freeze.domain(),
                freeze.frozen_at(),
                release.major,
                release.minor,
                release.patch,
                freeze.assigned(),
                freeze.digest_of_every_code()
            )
        })
        .collect();
    assert!(stale.is_empty(), "{}", stale.join("\n"));
    Ok(())
}
