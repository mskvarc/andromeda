//! The ICU collations already created on one tenant's database.

use crate::schema_ddl::icu_collation::{STRICT_ICU_LOCALE_VALIDATION, create_icu_collation};
use andromeda_ngsi_ld_query::ordering::collation::IcuCollation;
use dashmap::DashSet;
use sqlx::postgres::PgPool;
use thiserror::Error;

/// The `SQLSTATE` PostgreSQL reports an unusable ICU locale under.
///
/// Both refusals a collation identifier can draw -- `ICU locale "…" has unknown
/// language "…"` from the locale validation, and `could not open collator for
/// locale "…"` from ICU itself -- are raised as `invalid_parameter_value`. The
/// parameter in question is the client's own `collation`, which is what makes
/// the failure a clause 5.7.2.4 `BadRequestData` rather than a broker fault.
const INVALID_PARAMETER_VALUE: &str = "22023";

/// The ICU collation objects one tenant's database is known to hold.
///
/// A collation object is per-database, and every store above one tenant's pool
/// shares that database, so the set is correctly scoped to the pool it is built
/// from: the `CREATE COLLATION` for a given identifier runs once per store that
/// orders under it rather than once per query. Two stores over the same tenant
/// each keep their own set, so an identifier both of them order by is created
/// once by each; `IF NOT EXISTS` makes the second a no-op.
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
            pool: pool.clone(), // clone: `PgPool` shares its inner pool by reference count.
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
    /// the one place that can ask it: the statement runs in its own transaction
    /// under [`STRICT_ICU_LOCALE_VALIDATION`], so an unknown locale is refused
    /// rather than silently creating an object that sorts by the root
    /// collation.
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
        self.create(collation).await.map_err(|source| {
            // clone: the error outlives the ordering that named the identifier.
            let named = collation.clone();
            if refused_the_locale(&source) {
                CollationProvisioningError::NotAnIcuCollation { collation: named, source }
            } else {
                CollationProvisioningError::Statement { collation: named, source }
            }
        })?;
        self.created.insert(collation.clone()); // clone: the cache outlives the ordering that named the identifier.
        Ok(())
    }

    /// Create the collation object in a transaction of its own.
    ///
    /// The transaction exists for the `SET LOCAL`: the strict validation level
    /// has to hold for the `CREATE COLLATION` and has to be gone before the
    /// connection returns to the pool.
    async fn create(&self, collation: &IcuCollation) -> Result<(), sqlx::Error> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(sqlx::AssertSqlSafe(STRICT_ICU_LOCALE_VALIDATION))
            .execute(&mut *transaction)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(create_icu_collation(collation)))
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await
    }
}

/// Whether the statement failed because ICU would not accept the locale.
///
/// Every other `sqlx::Error` is a connection, protocol or decoding failure of
/// the broker's own rather than a judgement on the client's identifier.
fn refused_the_locale(error: &sqlx::Error) -> bool {
    matches!(error, sqlx::Error::Database(database) if database.code().is_some_and(|code| code == INVALID_PARAMETER_VALUE))
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
