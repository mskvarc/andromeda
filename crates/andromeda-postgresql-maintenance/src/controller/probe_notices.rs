//! The once-per-process warning for a tenant whose index probes are off.
//!
//! The extension is optional and its absence is a standing condition, not an
//! event, so the operator hears of it once per tenant, not on every visit. A
//! tenant that regains the capability is forgotten, so losing it again, or
//! losing it in another way, warns again.

use crate::{
    capability::ProbeCapability,
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
    /// The capability each warned-about tenant lacked when it was warned.
    warned: DashMap<TenantKey, ProbeCapability>,
}

impl ProbeNotices {
    /// Warn about `tenant` lacking `capability` unless this process already
    /// has.
    pub(crate) fn report(&self, tenant: &RegisteredTenant, capability: ProbeCapability) {
        if self.notice(tenant.key, capability) == Notice::Warn {
            let reason: &'static str = capability.into();
            warn!(tenant = tenant.log_name(), database = %tenant.database, decision = "capability-unavailable", reason, "PostgreSQL maintenance measures no index without pgstattuple; every other check runs");
        }
    }

    /// Record `capability` for the tenant keyed `key` and say whether it is
    /// news.
    fn notice(&self, key: TenantKey, capability: ProbeCapability) -> Notice {
        match capability {
            ProbeCapability::Ready => {
                self.warned.remove(&key);
                Notice::Quiet
            }
            ProbeCapability::MissingPgstattuple | ProbeCapability::MissingPgstattupleExecute => match self.warned.insert(key, capability) {
                Some(warned) if warned == capability => Notice::Quiet,
                Some(_) | None => Notice::Warn,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        capability::ProbeCapability,
        controller::probe_notices::{Notice, ProbeNotices},
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
}
