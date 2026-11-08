//! The ICU collations already created on one tenant's database.

use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;
use andromeda_postgresql_pool::sql_state::{SqlState, violates_unique_constraint};
use andromeda_postgresql_seaquery::{
    collation::{CreateIcuCollation, PostgresCollation},
    command::Command,
};
use dashmap::DashSet;
use sqlx::postgres::PgPool;
use thiserror::Error;

/// The catalog index that makes a collation name unique within its schema.
///
/// `IF NOT EXISTS` checks the catalog before it inserts, so two concurrent
/// creates of one name both pass the check and the later insert is refused on
/// this index once the earlier commits. That refusal means the object exists.
const COLLATION_NAME_INDEX: &str = "pg_collation_name_enc_nsp_index";

/// The ICU collation objects one tenant's database is known to hold.
///
/// A collation object is per-database, and every store above one tenant's pool
/// shares that database, so the set is correctly scoped to the pool it is built
/// from: the `CREATE COLLATION` for a given identifier runs once per store that
/// orders under it rather than once per query. Two stores over the same tenant
/// each keep their own set, so an identifier both of them order by is created
/// once by each; `IF NOT EXISTS` makes the second a no-op, and a create that
/// loses a concurrent race on [`COLLATION_NAME_INDEX`] counts as done.
///
/// Membership is only ever added after a successful create, so a failed
/// provisioning is retried on the next request rather than remembered as done.
#[derive(Debug)]
pub struct ProvisionedCollations {
    /// The tenant pool the collation objects live on.
    pool: PgPool,
    /// The identifiers a `CREATE COLLATION` has already succeeded for.
    created: DashSet<IcuCollation>,
}

impl ProvisionedCollations {
    /// The collations of the database `pool` connects to.
    #[must_use]
    pub fn of_pool(pool: &PgPool) -> ProvisionedCollations {
        ProvisionedCollations {
            pool: pool.clone(),
            created: DashSet::new(),
        }
    }

    /// Ensure `collation` names an object this database holds.
    ///
    /// Clause 4.23.1 lets a request name any ICU collation, and PostgreSQL
    /// pre-imports only the `<locale>-x-icu` forms; ordering under one that has
    /// no object fails the statement with `collation … does not exist`, so a
    /// conformant request would answer 500.
    ///
    /// The identifier was accepted by [`IcuCollation::parse`] as a well-formed
    /// BCP47 tag whose Unicode keywords are the ones IETF RFC 6067 registers.
    /// Whether ICU holds a collation for the locale itself is asked here, in
    /// the one place that can ask it: the collation is created under strict
    /// locale validation, so an unknown locale is refused rather than silently
    /// creating an object that sorts by the root collation, a successful
    /// response in an order the client never asked for. Clauses 5.7.2.4 and
    /// 5.7.4.4 make that refusal the client's: "if a preferred collation
    /// setting is present and it does not conform to a valid ICU collation
    /// (see IETF RFC 6067) then an error of type `BadRequestData` shall be
    /// raised". A create refused because a concurrent create of the same name
    /// committed first has provisioned the object all the same.
    ///
    /// # Errors
    ///
    /// Returns [`CollationProvisioningError::NotAnIcuCollation`] when ICU
    /// refuses the identifier, which clauses 5.7.2.4 and 5.7.4.4 make a
    /// `BadRequestData`, and [`CollationProvisioningError::Statement`] when the
    /// statement fails for any other reason.
    pub async fn ensure(&self, collation: &IcuCollation) -> Result<(), CollationProvisioningError> {
        if self.created.contains(collation) {
            return Ok(());
        }
        if let Err(source) = self.create(collation).await {
            match failure_of(&source) {
                CreateFailure::CreatedConcurrently => {}
                CreateFailure::LocaleRefused => {
                    return Err(CollationProvisioningError::NotAnIcuCollation {
                        // clone: the error outlives the borrowed collation it names.
                        collation: collation.clone(),
                        source,
                    });
                }
                CreateFailure::Statement => {
                    return Err(CollationProvisioningError::Statement {
                        // clone: the error outlives the borrowed collation it names.
                        collation: collation.clone(),
                        source,
                    });
                }
            }
        }
        self.created.insert(collation.clone());
        Ok(())
    }

    /// Create the collation object, named and collating by its identifier, in
    /// a transaction of its own.
    ///
    /// The transaction exists for the strict validation level, which has to
    /// hold for the creation and has to be gone before the connection returns
    /// to the pool.
    async fn create(&self, collation: &IcuCollation) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        CreateIcuCollation::new(PostgresCollation::new(collation.as_str()), collation.as_str())
            .to_query()
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await
    }
}

/// What a failed `CREATE COLLATION` means for the identifier.
#[derive(Debug, PartialEq, Eq)]
enum CreateFailure {
    /// ICU would not accept the locale: the client's identifier is at fault.
    LocaleRefused,
    /// A concurrent create of the same name committed first, so the object
    /// exists.
    CreatedConcurrently,
    /// A connection, protocol or statement failure of the broker's own.
    Statement,
}

/// The meaning of a failed `CREATE COLLATION`.
///
/// Both refusals a collation identifier can draw (`ICU locale "…" has unknown
/// language "…"` from the locale validation, and `could not open collator for
/// locale "…"` from ICU itself) are raised as `invalid_parameter_value`. The
/// parameter in question is the client's own `collation`, which is what makes
/// the failure a clause 5.7.2.4 `BadRequestData` rather than a broker fault.
fn failure_of(error: &sqlx::Error) -> CreateFailure {
    if violates_unique_constraint(error, COLLATION_NAME_INDEX) {
        return CreateFailure::CreatedConcurrently;
    }
    if SqlState::of(error) == Some(SqlState::InvalidParameterValue) {
        CreateFailure::LocaleRefused
    } else {
        CreateFailure::Statement
    }
}

/// An ICU collation object could not be created on the tenant's database.
#[derive(Debug, Error)]
pub enum CollationProvisioningError {
    /// ICU does not hold a collation for the locale the identifier names, so
    /// the identifier does not conform to a valid ICU collation.
    #[error("'{collation}' does not name a valid ICU collation: {source}")]
    NotAnIcuCollation {
        /// The identifier ICU refused.
        collation: IcuCollation,
        /// The refusal underneath.
        #[source]
        source: sqlx::Error,
    },
    /// The `CREATE COLLATION` failed for a reason other than the identifier.
    #[error("the ICU collation '{collation}' could not be provisioned")]
    Statement {
        /// The identifier whose object could not be created.
        collation: IcuCollation,
        /// The statement failure underneath.
        #[source]
        source: sqlx::Error,
    },
}

#[cfg(test)]
mod tests {
    use crate::provisioned_collations::{CreateFailure, failure_of};
    use andromeda_postgresql_pool::fake_database_error::FakeDatabaseError;
    use pretty_assertions::assert_eq;

    /// A database refusal with SQLSTATE `code` on `constraint`.
    fn refusal(code: &str, constraint: Option<&str>) -> sqlx::Error {
        let error = FakeDatabaseError::new("refused", code);
        match constraint {
            Some(constraint) => error.with_constraint(constraint).into_sqlx(),
            None => error.into_sqlx(),
        }
    }

    /// Clauses 5.7.2.4 and 5.7.4.4: a collation ICU does not hold is the
    /// client's `BadRequestData`.
    #[test]
    fn an_invalid_parameter_value_is_a_refused_locale() {
        assert_eq!(failure_of(&refusal("22023", None)), CreateFailure::LocaleRefused);
    }

    #[test]
    fn a_unique_violation_on_the_collation_name_index_is_a_concurrent_create() {
        assert_eq!(
            failure_of(&refusal("23505", Some("pg_collation_name_enc_nsp_index"))),
            CreateFailure::CreatedConcurrently
        );
    }

    #[test]
    fn a_unique_violation_on_another_index_is_a_statement_failure() {
        assert_eq!(failure_of(&refusal("23505", Some("pg_type_typname_nsp_index"))), CreateFailure::Statement);
        assert_eq!(failure_of(&refusal("23505", None)), CreateFailure::Statement);
    }

    #[test]
    fn any_other_refusal_is_a_statement_failure() {
        assert_eq!(failure_of(&refusal("57P01", None)), CreateFailure::Statement);
        assert_eq!(failure_of(&sqlx::Error::PoolTimedOut), CreateFailure::Statement);
    }
}
