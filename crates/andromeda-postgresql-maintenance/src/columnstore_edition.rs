//! Whether the tenant's `TimescaleDB` edition carries the columnstore.
//!
//! Only the TSL edition converts chunks, so on the Apache-2.0 edition no chunk
//! is ever overdue for conversion or partial, and neither is looked for.

use crate::error::MaintenanceError;
use andromeda_postgresql_seaquery::{columnstore_chunk::columnstore_licensed, statement::Statement};
use sea_query::Query;
use sqlx::PgConnection;

/// Whether the edition carries the hypercore columnstore.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ColumnstoreEdition {
    /// The TSL edition, licensed `timescale`.
    Licensed,
    /// The Apache-2.0 edition, or no `TimescaleDB`.
    Unlicensed,
}

/// Read the edition from the `timescaledb.license` setting.
pub(crate) async fn columnstore_edition(connection: &mut PgConnection) -> Result<ColumnstoreEdition, MaintenanceError> {
    let licensed = Query::select()
        .expr(columnstore_licensed())
        .to_owned()
        .to_query_scalar::<bool>()
        .fetch_one(connection)
        .await?;
    Ok(if licensed {
        ColumnstoreEdition::Licensed
    } else {
        ColumnstoreEdition::Unlicensed
    })
}
