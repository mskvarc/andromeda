//! Auditing `attribute_history.unit_code_id` against the unit-code catalog.
//!
//! `attribute_history` is a hypertable, and a foreign key on a hypertable costs
//! a lookup per inserted row on the broker's hottest write path, so the column
//! carries none. Every write goes through this crate, which resolves the code
//! against the compiled-in catalog before binding it, so the referent is
//! established in Rust rather than by the database. This audit is the check that
//! the arrangement held: it names any stored identity with no catalog row, which
//! is what a hand-written row or a catalog regression would leave behind.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use sqlx::Row;

/// One stored unit-code identity that names no catalog registration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UnregisteredUnitCodeRows {
    /// The identity exactly as stored, which by definition is not one the
    /// catalog assigns and therefore is not a [`UnitCodeId`].
    ///
    /// [`UnitCodeId`]: andromeda_unit_catalog::unit_code_id::UnitCodeId
    pub stored_identity: i16,
    /// How many history rows carry it.
    pub rows: i64,
}

impl TemporalRepository {
    /// Report history rows whose `unit_code_id` has no `unit_codes` row.
    ///
    /// An empty result is the expected outcome. Any row returned means the
    /// hypertable holds a unit nothing can decode, so the affected attributes
    /// render without their unit until the catalog registers the identity again.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the audit statement fails
    /// and [`TemporalStoreError::Decode`] when a returned column does not carry
    /// the expected type.
    pub async fn audit_unregistered_unit_codes(&self) -> Result<Vec<UnregisteredUnitCodeRows>, TemporalStoreError> {
        let rows = sqlx::query(AUDIT_UNREGISTERED_UNIT_CODES_SQL).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(UnregisteredUnitCodeRows {
                    stored_identity: row
                        .try_get("unit_code_id")
                        .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.unit_code_id: {error}")))?,
                    rows: row
                        .try_get("row_count")
                        .map_err(|error| TemporalStoreError::Decode(format!("invalid unit-code audit count: {error}")))?,
                })
            })
            .collect()
    }
}

/// Audit statement replacing the foreign key the hypertable does not carry.
const AUDIT_UNREGISTERED_UNIT_CODES_SQL: &str = r#"
SELECT ah.unit_code_id, COUNT(*) AS row_count
FROM attribute_history ah
LEFT JOIN unit_codes uc ON uc.id = ah.unit_code_id
WHERE ah.unit_code_id IS NOT NULL AND uc.id IS NULL
GROUP BY ah.unit_code_id
ORDER BY ah.unit_code_id
"#;

#[cfg(test)]
mod tests {
    use crate::unit_code_audit::AUDIT_UNREGISTERED_UNIT_CODES_SQL;

    #[test]
    fn the_audit_reports_only_identities_the_catalog_is_missing() {
        assert!(AUDIT_UNREGISTERED_UNIT_CODES_SQL.contains("LEFT JOIN unit_codes uc ON uc.id = ah.unit_code_id"));
        assert!(AUDIT_UNREGISTERED_UNIT_CODES_SQL.contains("WHERE ah.unit_code_id IS NOT NULL AND uc.id IS NULL"));
    }
}
