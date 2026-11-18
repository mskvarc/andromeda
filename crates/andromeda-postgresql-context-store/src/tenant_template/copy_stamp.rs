//! The schema stamp of a database copied from the tenant template.

use crate::{error::ContextStoreError, extension_update::begin_without_timescaledb, schema_contract::schema_metadata::renew_stamp_instant};
use andromeda_postgresql_pool::schema_change_lock::schema_change_connect_options;
use sqlx::{Connection, PgConnection};
use url::Url;

/// Give the schema stamp of the copy `database_url` names an instant of its
/// own, over a connection of its own.
///
/// The update runs with `TimescaleDB` unloaded, so the session never loads the
/// extension at the template's version, which the copy's migration run may
/// then replace.
///
/// # Errors
///
/// Returns [`ContextStoreError::Database`] when the connection cannot be
/// opened or closed or the transaction fails, and
/// [`ContextStoreError::SchemaContract`] when the stamp cannot be written.
pub(crate) async fn renew_copy_stamp(database_url: &Url) -> Result<(), ContextStoreError> {
    let options = schema_change_connect_options(database_url.as_str())?;
    let mut connection = PgConnection::connect_with(&options).await?;
    let renewed = renew_on(&mut connection).await;
    connection.close().await?;
    renewed
}

/// Renew the stamp in a transaction of its own on `connection`.
async fn renew_on(connection: &mut PgConnection) -> Result<(), ContextStoreError> {
    let mut transaction = begin_without_timescaledb(&mut *connection).await?;
    renew_stamp_instant(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(())
}
