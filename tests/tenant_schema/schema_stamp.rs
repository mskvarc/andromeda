//! The `schema_metadata` stamp of the tenant, read and rewritten by the
//! verifications that drift it and put it back.

use andromeda_postgresql_schema::tenancy::SchemaMetadata;
use andromeda_postgresql_seaquery::{clock::now, literal::true_literal, statement::Statement};
use sea_query::{DeleteStatement, Expr, InsertStatement, Query, SelectStatement, UpdateStatement};
use sqlx::{PgPool, Row};

/// The raw `schema_metadata` row a pool currently carries, if any.
pub async fn read_stamp_row(pool: &PgPool) -> Option<(i32, String)> {
    stamp_statement()
        .to_query()
        .fetch_optional(pool)
        .await
        .expect("the stamp must be readable")
        .map(|row| {
            (
                row.try_get::<i32, _>("schema_version").expect("the schema_version column decodes"),
                row.try_get::<String, _>("migrations_sha256").expect("the migrations_sha256 column decodes"),
            )
        })
}

/// The stamp row of the tenant.
pub fn stamp_statement() -> SelectStatement {
    Query::select()
        .columns([SchemaMetadata::SchemaVersion, SchemaMetadata::MigrationsSha256])
        .from(SchemaMetadata::Table)
        .and_where(Expr::col(SchemaMetadata::Singleton))
        .to_owned()
}

/// The removal of the stamp.
pub fn clear_stamp() -> DeleteStatement {
    Query::delete().from_table(SchemaMetadata::Table).to_owned()
}

/// The insert of a stamp of `version` and `digest`, written now.
pub fn write_stamp(version: i32, digest: String) -> InsertStatement {
    Query::insert()
        .into_table(SchemaMetadata::Table)
        .columns([
            SchemaMetadata::AppliedAt,
            SchemaMetadata::SchemaVersion,
            SchemaMetadata::Singleton,
            SchemaMetadata::MigrationsSha256,
        ])
        .values([now(), Expr::val(version), true_literal(), Expr::val(digest)])
        .expect("one value per column")
        .to_owned()
}

/// The update of the stamp's version to `version`, its digest as it was.
pub fn restamp_version(version: i32) -> UpdateStatement {
    Query::update()
        .table(SchemaMetadata::Table)
        .value(SchemaMetadata::SchemaVersion, version)
        .and_where(Expr::col(SchemaMetadata::Singleton))
        .to_owned()
}

/// The update of the stamp's version and digest to `version` and `digest`.
pub fn restamp(version: i32, digest: String) -> UpdateStatement {
    Query::update()
        .table(SchemaMetadata::Table)
        .value(SchemaMetadata::SchemaVersion, version)
        .value(SchemaMetadata::MigrationsSha256, digest)
        .and_where(Expr::col(SchemaMetadata::Singleton))
        .to_owned()
}
