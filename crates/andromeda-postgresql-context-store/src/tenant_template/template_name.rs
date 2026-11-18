//! The name of the tenant template.
//!
//! The template of the tenant databases named after one prefix is
//! `template_<prefix>`, beside PostgreSQL's own `template0` and `template1`.
//! Each broker has a template of its own, so two brokers sharing a server
//! under different prefixes never migrate or rebuild each other's.
//!
//! A tenant database name ends in an underscore and a hexadecimal digest of
//! the tenant's name, and a template name ends in its prefix, so the two meet
//! only for a prefix that itself ends in the digest of some tenant name.

use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use andromeda_ngsi_ld_lexicon::tenant_database_name::TenantDatabaseName;

/// What every template name starts with.
const TEMPLATE_NAME_START: &str = "template_";

/// The name of the template the tenant databases named after `prefix` are
/// copied from.
///
/// The name fits PostgreSQL's 63 bytes for every prefix the configuration
/// admits.
#[must_use]
pub fn template_database_name(prefix: &DbNamePrefix) -> TenantDatabaseName {
    TenantDatabaseName::from(format!("{TEMPLATE_NAME_START}{prefix}"))
}

#[cfg(test)]
mod tests {
    use crate::{tenant_database_naming::derive_database_name, tenant_template::template_name::template_database_name};
    use andromeda_configuration::schema::db_name_prefix::{DB_NAME_PREFIX_BYTE_LIMIT, DbNamePrefix};
    use andromeda_ngsi_ld_lexicon::tenant::TenantName;
    use andromeda_postgresql_schema::tenancy::DATABASE_NAME_BYTE_LIMIT;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_template_of_the_default_prefix_sits_beside_template0_and_template1() {
        assert_eq!(template_database_name(&DbNamePrefix::default()).as_str(), "template_andromeda_tenant");
    }

    #[test]
    fn a_template_name_fits_the_identifier_limit_at_the_longest_prefix() {
        let prefix = DbNamePrefix::try_from("p".repeat(DB_NAME_PREFIX_BYTE_LIMIT)).expect("a prefix at the limit");
        let name = template_database_name(&prefix);
        assert!(name.as_str().len() <= DATABASE_NAME_BYTE_LIMIT, "{} bytes: {name}", name.as_str().len());
    }

    #[test]
    fn no_tenant_database_of_any_prefix_is_named_like_a_template() {
        for prefix in ["andromeda_tenant", "template", "template_andromeda", "t"] {
            let prefix = DbNamePrefix::try_from(prefix).expect("a prefix");
            for tenant in ["andromeda", "andromeda_tenant", "template", "tenant", "a"] {
                let tenant_database = derive_database_name(&prefix, &tenant.parse::<TenantName>().expect("a tenant name"));
                for template_prefix in ["andromeda_tenant", "andromeda", "tenant", "t"] {
                    let template = template_database_name(&DbNamePrefix::try_from(template_prefix).expect("a prefix"));
                    assert_ne!(tenant_database, template, "{prefix} {tenant} against {template_prefix}");
                }
            }
        }
    }
}
