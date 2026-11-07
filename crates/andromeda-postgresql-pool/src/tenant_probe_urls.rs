//! Which base a transient probe of a tenant's database connects over.
//!
//! A probe of an established tenant connects over the request base, the
//! pooler when one is configured, like every other session that serves the
//! tenant. A provisional tenant's database may not exist yet or may not carry
//! the session defaults its provisioning installs over the direct base, and a
//! transaction pooler keeps the server session a probe opened and later hands
//! it to the tenant's runtime with the settings it started under. A probe of a
//! provisional tenant therefore connects directly, so no pooled session reaches
//! a tenant database before its defaults are in place.

use crate::database_url::DatabaseUrl;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
use andromeda_tenancy::tenant_existence::TenantExistence;
use url::Url;

/// The two bases a tenant probe may connect over.
#[derive(Clone, Debug)]
pub struct TenantProbeUrls {
    /// The base a tenant's request path connects over, the pooler when one is
    /// configured.
    request: DatabaseUrl,
    /// The base that reaches PostgreSQL directly.
    direct: DatabaseUrl,
}

impl TenantProbeUrls {
    /// Probe bases over the request and the direct base URLs.
    #[must_use]
    pub const fn new(request: DatabaseUrl, direct: DatabaseUrl) -> TenantProbeUrls {
        TenantProbeUrls { request, direct }
    }

    /// The URL a probe of `database`, serving a tenant of `existence`,
    /// connects to.
    #[must_use]
    pub fn tenant_url(&self, database: &TenantDatabaseName, existence: TenantExistence) -> Url {
        match existence {
            TenantExistence::Provisional => self.direct.with_database_name(database),
            TenantExistence::Established => self.request.with_database_name(database),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{database_url::DatabaseUrl, tenant_probe_urls::TenantProbeUrls};
    use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;
    use andromeda_tenancy::tenant_existence::TenantExistence;
    use pretty_assertions::assert_eq;

    fn urls() -> TenantProbeUrls {
        TenantProbeUrls::new(
            DatabaseUrl::new("postgres://broker@pooler:6432/andromeda".parse().expect("request URL")),
            DatabaseUrl::new("postgres://broker@postgres:5432/andromeda".parse().expect("direct URL")),
        )
    }

    #[test]
    fn a_provisional_tenant_is_probed_directly() {
        let database = TenantDatabaseName::from("andromeda_tenant_new".to_owned());
        assert_eq!(
            urls().tenant_url(&database, TenantExistence::Provisional).as_str(),
            "postgres://broker@postgres:5432/andromeda_tenant_new"
        );
    }

    #[test]
    fn an_established_tenant_is_probed_over_the_request_base() {
        let database = TenantDatabaseName::from("andromeda_tenant_acme".to_owned());
        assert_eq!(
            urls().tenant_url(&database, TenantExistence::Established).as_str(),
            "postgres://broker@pooler:6432/andromeda_tenant_acme"
        );
    }
}
