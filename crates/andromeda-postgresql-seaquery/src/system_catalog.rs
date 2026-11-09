//! The PostgreSQL system catalog relations the broker and its test harness
//! read, named for sea-query.

use sea_query::Iden;

/// The `information_schema` schema.
#[derive(Iden, Clone, Copy)]
#[iden = "information_schema"]
pub struct InformationSchema;

/// `information_schema.columns`: one row per column of every relation.
#[derive(Iden, Clone, Copy)]
pub enum InformationSchemaColumns {
    /// The relation itself.
    #[iden = "columns"]
    Table,
    /// The schema of the column's relation.
    TableSchema,
    /// The column's relation.
    TableName,
    /// The column's name.
    ColumnName,
    /// How an identity column generates its value, NULL for any other column.
    IdentityGeneration,
    /// `ALWAYS` for a stored generated column, `NEVER` otherwise.
    IsGenerated,
    /// The column's 1-based position in its relation.
    OrdinalPosition,
}

/// `information_schema.tables`: one row per table and view.
#[derive(Iden, Clone, Copy)]
pub enum InformationSchemaTables {
    /// The relation itself.
    #[iden = "tables"]
    Table,
    /// The schema of the relation.
    TableSchema,
    /// The relation's name.
    TableName,
}

/// `information_schema.views`: one row per view.
#[derive(Iden, Clone, Copy)]
pub enum InformationSchemaViews {
    /// The relation itself.
    #[iden = "views"]
    Table,
    /// The schema of the view.
    TableSchema,
    /// The view's name.
    TableName,
}

/// `pg_class`: one row per relation.
#[derive(Iden, Clone, Copy)]
pub enum PgClass {
    /// The relation itself.
    #[iden = "pg_class"]
    Table,
    /// The relation's OID.
    Oid,
    /// The relation's name.
    Relname,
    /// The OID of the relation's schema.
    Relnamespace,
    /// The relation's kind: `r` for an ordinary table.
    Relkind,
}

/// `pg_constraint`: one row per table constraint.
#[derive(Iden, Clone, Copy)]
pub enum PgConstraint {
    /// The relation itself.
    #[iden = "pg_constraint"]
    Table,
    /// The OID of the relation the constraint is declared on.
    Conrelid,
    /// The OID of the relation a foreign key references.
    Confrelid,
    /// The constraint's kind: `f` for a foreign key.
    Contype,
}

/// `pg_namespace`: one row per schema.
#[derive(Iden, Clone, Copy)]
pub enum PgNamespace {
    /// The relation itself.
    #[iden = "pg_namespace"]
    Table,
    /// The schema's OID.
    Oid,
    /// The schema's name.
    Nspname,
}

/// `pg_database`: one row per database of the server.
#[derive(Iden, Clone, Copy)]
pub enum PgDatabase {
    /// The relation itself.
    #[iden = "pg_database"]
    Table,
    /// The database's name.
    Datname,
}

/// `pg_stat_activity`: one row per server process.
#[derive(Iden, Clone, Copy)]
pub enum PgStatActivity {
    /// The relation itself.
    #[iden = "pg_stat_activity"]
    Table,
    /// The process's kind: `client backend` for a client session.
    BackendType,
    /// The `application_name` the session set.
    ApplicationName,
    /// The database the session is connected to.
    Datname,
    /// The process id of the session's backend.
    Pid,
    /// The start of the session's current transaction, NULL while it is in
    /// none.
    XactStart,
    /// The kind of event the session waits for, `Lock` while it waits on a
    /// heavyweight lock, NULL while it waits for nothing.
    WaitEventType,
}

/// `pg_stat_user_tables`: one row per user table of the current database,
/// with its cumulative statistics.
#[derive(Iden, Clone, Copy)]
pub enum PgStatUserTables {
    /// The relation itself.
    #[iden = "pg_stat_user_tables"]
    Table,
    /// The schema holding the table.
    Schemaname,
    /// The table's name.
    Relname,
    /// The estimated live rows.
    NLiveTup,
    /// The estimated dead rows, those no vacuum has removed yet.
    NDeadTup,
}
