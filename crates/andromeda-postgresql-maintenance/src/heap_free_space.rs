//! A heap's free space, its TOAST table's included, measured for a repack.
//!
//! Vacuum leaves the space of the rows it removes free inside the heap's
//! pages, where only new rows of the same table reuse it, so free space, not
//! dead tuples, is what a repack gives back. `pgstattuple_approx` estimates
//! it from the free space map for every page the visibility map marks
//! all-visible and reads only the rest, which keeps the probe bounded on a
//! settled heap. Entity bodies and notification payloads are stored out of
//! line, so the TOAST table, measured the same way, is often where the space
//! is.

use crate::{
    branch::Branch,
    candidate::heap_target::HeapTarget,
    catalog_counter::{CatalogCounter, non_negative, object_id},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{
    maintenance::{table_size, to_regclass},
    repack_catalog::{approximate_free_space, relation_and_toast},
    statement::Statement,
};
use sea_query::{Expr, Query};
use sqlx::{PgConnection, Row};

/// One heap's free space with its TOAST table's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FreeSpaceReading {
    /// The heap's OID in the tenant database.
    pub(crate) relid: u32,
    /// Bytes of the heap and its TOAST table.
    pub(crate) relation_bytes: u64,
    /// Estimated free bytes of the heap and its TOAST table.
    pub(crate) free_bytes: u64,
    /// The larger of the heap's and the TOAST table's dead share, as a whole
    /// percentage.
    pub(crate) dead_percent: u8,
}

/// One relation's estimate, as `pgstattuple_approx` reports it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Estimate {
    /// The relation's bytes.
    bytes: u64,
    /// Its estimated free bytes.
    free: u64,
    /// Its dead share, as a whole percentage.
    dead_percent: u8,
}

/// The free space of `heap` in `branch`, `None` when the heap does not
/// resolve.
pub(crate) async fn heap_free_space(connection: &mut PgConnection, branch: Branch, heap: HeapTarget) -> Result<Option<FreeSpaceReading>, MaintenanceError> {
    let Some(row) = relation_and_toast(&branch.qualified(heap.relation_name()))
        .to_query()
        .fetch_optional(&mut *connection)
        .await?
    else {
        return Ok(None);
    };
    let relid = object_id(row.try_get::<i64, _>(0)?, CatalogCounter::RelationOid)?;
    let toast = object_id(row.try_get::<i64, _>(1)?, CatalogCounter::RelationOid)?;
    let heap_estimate = estimate(connection, relid).await?;
    let toast_estimate = if toast == 0 { None } else { Some(estimate(connection, toast).await?) };
    Ok(Some(combined(relid, heap_estimate, toast_estimate)))
}

/// Bytes of `heap` and its TOAST table in `branch`, its indexes excluded,
/// `None` when the heap does not resolve.
pub(crate) async fn heap_table_bytes(connection: &mut PgConnection, branch: Branch, heap: HeapTarget) -> Result<Option<u64>, MaintenanceError> {
    let statement = Query::select()
        .expr(table_size(to_regclass(Expr::val(branch.qualified(heap.relation_name())))))
        .to_owned();
    let bytes = statement.to_query_scalar::<Option<i64>>().fetch_one(connection).await?;
    bytes.map(|bytes| non_negative(bytes, CatalogCounter::TableBytes)).transpose()
}

/// Estimate one relation by OID.
async fn estimate(connection: &mut PgConnection, relid: u32) -> Result<Estimate, MaintenanceError> {
    let row = approximate_free_space(relid).to_query().fetch_one(connection).await?;
    let dead = non_negative(i64::from(row.try_get::<i32, _>(2)?), CatalogCounter::DeadPercent)?;
    Ok(Estimate {
        bytes: non_negative(row.try_get::<i64, _>(0)?, CatalogCounter::TableBytes)?,
        free: non_negative(row.try_get::<i64, _>(1)?, CatalogCounter::FreeSpaceBytes)?,
        dead_percent: u8::try_from(Ord::min(dead, 100)).unwrap_or(100),
    })
}

/// The heap's estimate with its TOAST table's, when it has one.
fn combined(relid: u32, heap: Estimate, toast: Option<Estimate>) -> FreeSpaceReading {
    let toast = toast.unwrap_or(Estimate {
        bytes: 0,
        free: 0,
        dead_percent: 0,
    });
    FreeSpaceReading {
        relid,
        relation_bytes: heap.bytes.saturating_add(toast.bytes),
        free_bytes: heap.free.saturating_add(toast.free),
        dead_percent: Ord::max(heap.dead_percent, toast.dead_percent),
    }
}

#[cfg(test)]
mod tests {
    use crate::heap_free_space::{Estimate, FreeSpaceReading, combined};
    use pretty_assertions::assert_eq;

    const MIB: u64 = 1_048_576;

    #[test]
    fn the_toast_table_adds_its_bytes_its_free_space_and_its_dead_share() {
        let heap = Estimate {
            bytes: 100 * MIB,
            free: 20 * MIB,
            dead_percent: 3,
        };
        let toast = Estimate {
            bytes: 400 * MIB,
            free: 300 * MIB,
            dead_percent: 12,
        };
        assert_eq!(
            combined(16_384, heap, Some(toast)),
            FreeSpaceReading {
                relid: 16_384,
                relation_bytes: 500 * MIB,
                free_bytes: 320 * MIB,
                dead_percent: 12,
            }
        );
    }

    #[test]
    fn a_heap_without_a_toast_table_is_measured_alone() {
        let heap = Estimate {
            bytes: 100 * MIB,
            free: 60 * MIB,
            dead_percent: 1,
        };
        assert_eq!(
            combined(16_384, heap, None),
            FreeSpaceReading {
                relid: 16_384,
                relation_bytes: 100 * MIB,
                free_bytes: 60 * MIB,
                dead_percent: 1,
            }
        );
    }
}
