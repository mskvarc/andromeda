//! The once-per-process warning for a tenant whose index and heap probes are
//! off, or whose repacks are only reported.
//!
//! Both extensions are optional and their absence is a standing condition,
//! not an event, so the operator hears of it once per tenant, not on every
//! visit. A tenant that regains a capability is forgotten, so losing it
//! again, or losing it in another way, warns again.

use crate::{
    capability::{ProbeCapability, RepackCapability},
    state::{RegisteredTenant, TenantKey},
};
use dashmap::DashMap;
use tracing::warn;

/// Whether a visit's capability is news to the operator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Notice {
    /// The tenant lacks the capability in a way this process has not warned
    /// about.
    Warn,
    /// The tenant has the capability, or this process already warned about
    /// the same lack.
    Quiet,
}

/// The tenants this process has warned about, each with the lack it named.
#[derive(Debug, Default)]
pub(crate) struct ProbeNotices {
    /// The probe capability each warned-about tenant lacked when it was
    /// warned.
    warned: DashMap<TenantKey, ProbeCapability>,
    /// The repack capability each warned-about tenant lacked when it was
    /// warned.
    repack_warned: DashMap<TenantKey, RepackCapability>,
}

impl ProbeNotices {
    /// Warn about `tenant` lacking `capability` unless this process already
    /// has.
    pub(crate) fn report(&self, tenant: &RegisteredTenant, capability: ProbeCapability) {
        if self.notice(tenant.key, capability) == Notice::Warn {
            let reason: &'static str = capability.into();
            warn!(tenant = tenant.log_name(), database = %tenant.database, decision = "capability-unavailable", reason, "PostgreSQL maintenance measures no index and no heap without pgstattuple; every other check runs");
        }
    }

    /// Warn about `tenant` lacking a repack `capability` unless this process
    /// already has.
    pub(crate) fn report_repack(&self, tenant: &RegisteredTenant, capability: RepackCapability) {
        if let Some(reason) = capability.missing()
            && notice(&self.repack_warned, tenant.key, Some(capability)) == Notice::Warn
        {
            warn!(tenant = tenant.log_name(), database = %tenant.database, decision = "capability-unavailable", reason, capability = ?capability, "PostgreSQL maintenance only reports the repacks it would run; every other action runs");
        }
        if capability.missing().is_none() {
            notice(&self.repack_warned, tenant.key, None);
        }
    }

    /// Record `capability` for the tenant keyed `key` and say whether it is
    /// news.
    fn notice(&self, key: TenantKey, capability: ProbeCapability) -> Notice {
        let missing = match capability {
            ProbeCapability::Ready => None,
            ProbeCapability::MissingPgstattuple | ProbeCapability::MissingPgstattupleExecute => Some(capability),
        };
        notice(&self.warned, key, missing)
    }
}

/// Record the lack `missing` of the tenant keyed `key` in `warned`, `None`
/// when it lacks nothing, and say whether it is news.
fn notice<C: Copy + Eq>(warned: &DashMap<TenantKey, C>, key: TenantKey, missing: Option<C>) -> Notice {
    match missing {
        None => {
            warned.remove(&key);
            Notice::Quiet
        }
        Some(capability) => match warned.insert(key, capability) {
            Some(previous) if previous == capability => Notice::Quiet,
            Some(_) | None => Notice::Warn,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        capability::{ProbeCapability, RepackCapability},
        controller::probe_notices::{Notice, ProbeNotices, notice},
        state::TenantKey,
    };
    use pretty_assertions::assert_eq;

    #[test]
    fn a_tenant_without_pgstattuple_is_warned_about_once_over_ten_visits() {
        let notices = ProbeNotices::default();
        let notices = (0..10)
            .map(|_| notices.notice(TenantKey(1), ProbeCapability::MissingPgstattuple))
            .collect::<Vec<_>>();
        assert_eq!(notices.first(), Some(&Notice::Warn));
        assert!(notices.iter().skip(1).all(|notice| *notice == Notice::Quiet), "{notices:?}");
    }

    #[test]
    fn each_tenant_is_warned_about_on_its_own() {
        let notices = ProbeNotices::default();
        assert_eq!(notices.notice(TenantKey(1), ProbeCapability::MissingPgstattuple), Notice::Warn);
        assert_eq!(notices.notice(TenantKey(2), ProbeCapability::MissingPgstattuple), Notice::Warn);
    }

    #[test]
    fn a_tenant_with_pgstattuple_is_never_warned_about() {
        let notices = ProbeNotices::default();
        assert_eq!(notices.notice(TenantKey(1), ProbeCapability::Ready), Notice::Quiet);
    }

    #[test]
    fn a_tenant_that_loses_pgstattuple_again_or_differently_is_warned_about_again() {
        let notices = ProbeNotices::default();
        assert_eq!(notices.notice(TenantKey(1), ProbeCapability::MissingPgstattuple), Notice::Warn);
        assert_eq!(notices.notice(TenantKey(1), ProbeCapability::MissingPgstattupleExecute), Notice::Warn);
        assert_eq!(notices.notice(TenantKey(1), ProbeCapability::Ready), Notice::Quiet);
        assert_eq!(notices.notice(TenantKey(1), ProbeCapability::MissingPgstattupleExecute), Notice::Warn);
    }

    #[test]
    fn a_tenant_without_pg_repack_is_warned_about_once_and_again_after_it_regained_it() {
        let notices = ProbeNotices::default();
        let missing = Some(RepackCapability::MissingExtension);
        assert_eq!(notice(&notices.repack_warned, TenantKey(1), missing), Notice::Warn);
        assert_eq!(notice(&notices.repack_warned, TenantKey(1), missing), Notice::Quiet);
        assert_eq!(notice(&notices.repack_warned, TenantKey(1), Some(RepackCapability::MissingRight)), Notice::Warn);
        assert_eq!(notice::<RepackCapability>(&notices.repack_warned, TenantKey(1), None), Notice::Quiet);
        assert_eq!(notice(&notices.repack_warned, TenantKey(1), missing), Notice::Warn);
    }
}
