//! The databases a pass observes but never visits.
//!
//! Transaction-ID wraparound is cluster-wide: PostgreSQL stops assigning IDs
//! to every database once the oldest `datfrozenxid` of any database reaches its
//! limit, and a database ages with the cluster's transaction rate whether or
//! not anything writes to it. The server's own databases, a suspended or
//! provisional tenant's database and a database an unfinished removal left
//! behind all age, and autovacuum must freeze each of them. The controller
//! visits only active tenants, so these are reported from the fleet statement
//! alone, under their database name.

use crate::{
    fleet_observation::DatabaseFreeze,
    freeze_policy::FreezePressure,
    observation::{Finding, ObservationDecision, ObservationLevel},
};
use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
use strum::IntoStaticStr;

/// A PostgreSQL database's name as `pg_database` holds it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DatabaseName(pub(crate) String);

impl DatabaseName {
    /// The name, for a log field.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Why the controller does not visit a database.
#[derive(Clone, Copy, Debug, Eq, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum UnvisitedRegistration {
    /// Its tenant is administratively suspended.
    Suspended,
    /// Its tenant is provisional: registered for a creating operation and not
    /// known to exist yet.
    Provisional,
    /// No registry row names it.
    Unregistered,
}

/// One database that accepts connections and that no visit reaches.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UnvisitedDatabase {
    /// Its name.
    pub(crate) name: DatabaseName,
    /// Why no visit reaches it.
    pub(crate) registration: UnvisitedRegistration,
    /// Its freeze ages.
    pub(crate) freeze: DatabaseFreeze,
}

impl UnvisitedDatabase {
    /// The database's freeze pressure once raised, at the level the pressure
    /// warrants.
    fn freeze_finding(&self) -> Option<Finding> {
        match self.freeze.pressure() {
            FreezePressure::Normal => None,
            FreezePressure::Overdue => Some(Finding::warning(ObservationDecision::FreezePressure)),
            FreezePressure::Critical => Some(Finding::error(ObservationDecision::FreezePressure)),
        }
    }

    /// An unregistered database named like a tenant database, reported as an
    /// orphan. The server's own databases are not named like one.
    fn orphan_finding(&self, tenant_database_prefix: &DbNamePrefix) -> Option<Finding> {
        match self.registration {
            UnvisitedRegistration::Unregistered if is_tenant_shaped(&self.name, tenant_database_prefix) => {
                Some(Finding::warning(ObservationDecision::OrphanTenantDatabase))
            }
            UnvisitedRegistration::Unregistered | UnvisitedRegistration::Suspended | UnvisitedRegistration::Provisional => None,
        }
    }

    /// Log every finding of the database.
    pub(crate) fn report(&self, tenant_database_prefix: &DbNamePrefix) {
        let registration: &'static str = self.registration.into();
        if let Some(finding) = self.freeze_finding() {
            observation_event!(
                finding,
                database = self.name.as_str(),
                registration,
                transaction_age = self.freeze.transaction.age,
                transaction_freeze_max_age = self.freeze.transaction.freeze_max_age,
                multixact_age = self.freeze.multixact.age,
                multixact_freeze_max_age = self.freeze.multixact.freeze_max_age,
                "PostgreSQL anti-wraparound vacuum is behind in a database the maintenance controller does not visit"
            );
        }
        if let Some(finding) = self.orphan_finding(tenant_database_prefix) {
            observation_event!(
                finding,
                database = self.name.as_str(),
                registration,
                "PostgreSQL database named like a tenant database has no registry row"
            );
        }
    }
}

/// Whether `name` has the shape of a tenant database name, the configured
/// prefix and a per-tenant suffix after an underscore.
fn is_tenant_shaped(name: &DatabaseName, prefix: &DbNamePrefix) -> bool {
    name.0
        .strip_prefix(prefix.as_str())
        .and_then(|rest| rest.strip_prefix('_'))
        .is_some_and(|suffix| !suffix.is_empty())
}

#[cfg(test)]
mod tests {
    use crate::{
        fleet_observation::DatabaseFreeze,
        freeze_policy::FreezeAge,
        observation::{Finding, ObservationDecision},
        unvisited_database::{DatabaseName, UnvisitedDatabase, UnvisitedRegistration},
    };
    use andromeda_configuration::schema::db_name_prefix::DbNamePrefix;
    use pretty_assertions::assert_eq;

    /// A database with transaction ID age `age` against the default
    /// `autovacuum_freeze_max_age` of 200,000,000, so 250,000,000 is overdue
    /// and 1,000,000,000 critical.
    fn database(name: &str, registration: UnvisitedRegistration, age: u64) -> UnvisitedDatabase {
        UnvisitedDatabase {
            name: DatabaseName(name.to_owned()),
            registration,
            freeze: DatabaseFreeze {
                transaction: FreezeAge {
                    age,
                    freeze_max_age: 200_000_000,
                },
                multixact: FreezeAge {
                    age: 0,
                    freeze_max_age: 400_000_000,
                },
            },
        }
    }

    /// The database's freeze finding beside its orphan finding.
    fn findings(database: &UnvisitedDatabase) -> (Option<Finding>, Option<Finding>) {
        (database.freeze_finding(), database.orphan_finding(&DbNamePrefix::default()))
    }

    #[test]
    fn a_suspended_tenant_database_reports_its_raised_freeze_pressure_at_the_level_it_warrants() {
        let suspended = |age| database("andromeda_tenant_acme_0badc0de", UnvisitedRegistration::Suspended, age);
        assert_eq!(findings(&suspended(249_999_999)), (None, None));
        assert_eq!(
            findings(&suspended(250_000_000)),
            (Some(Finding::warning(ObservationDecision::FreezePressure)), None)
        );
        assert_eq!(
            findings(&suspended(1_000_000_000)),
            (Some(Finding::error(ObservationDecision::FreezePressure)), None)
        );
    }

    #[test]
    fn a_provisional_tenant_database_is_no_orphan_and_reports_only_its_freeze_pressure() {
        let provisional = |age| database("andromeda_tenant_acme_0badc0de", UnvisitedRegistration::Provisional, age);
        assert_eq!(findings(&provisional(10)), (None, None));
        assert_eq!(
            findings(&provisional(1_000_000_000)),
            (Some(Finding::error(ObservationDecision::FreezePressure)), None)
        );
    }

    #[test]
    fn an_unregistered_tenant_shaped_database_is_an_orphan_whatever_its_age() {
        assert_eq!(
            findings(&database("andromeda_tenant_acme_0badc0de", UnvisitedRegistration::Unregistered, 10)),
            (None, Some(Finding::warning(ObservationDecision::OrphanTenantDatabase)))
        );
        assert_eq!(
            findings(&database("andromeda_tenant_acme_0badc0de", UnvisitedRegistration::Unregistered, 1_000_000_000)),
            (
                Some(Finding::error(ObservationDecision::FreezePressure)),
                Some(Finding::warning(ObservationDecision::OrphanTenantDatabase))
            )
        );
    }

    #[test]
    fn the_servers_own_databases_report_only_their_freeze_pressure() {
        for name in ["postgres", "template1", "andromeda_tenant", "andromeda_tenant_", "andromeda_tenantx_a"] {
            assert_eq!(findings(&database(name, UnvisitedRegistration::Unregistered, 10)), (None, None), "{name}");
            assert_eq!(
                findings(&database(name, UnvisitedRegistration::Unregistered, 300_000_000)),
                (Some(Finding::warning(ObservationDecision::FreezePressure)), None),
                "{name}"
            );
        }
    }
}
