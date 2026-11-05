//! The statements the extension generated for one table.
//!
//! `repack.tables` reports, per table, every statement a run executes that
//! names the table's own columns, key and storage. The controller never
//! writes or edits one: each is opaque text the extension owns, run as
//! generated or passed back to the extension's replay function.

use crate::{
    branch::Branch,
    candidate::{heap_target::HeapTarget, repack_order::RepackOrder},
    error::MaintenanceError,
};
use andromeda_postgresql_seaquery::{
    repack::{RepackScript, RepackTables, ReplayStatements, index_definitions, order_by_index, table_script},
    statement::Statement,
};
use sea_query::Iden;
use sqlx::{PgConnection, Row, postgres::PgRow};

/// One replay statement the extension generated, passed back to it as text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GeneratedStatement(String);

/// The statements the extension's replay function runs for one table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReplayScript {
    /// The read of the next log rows.
    peek: GeneratedStatement,
    /// The replay of a logged insert.
    insert: GeneratedStatement,
    /// The replay of a logged delete.
    delete: GeneratedStatement,
    /// The replay of a logged update.
    update: GeneratedStatement,
    /// The removal of replayed log rows.
    pop: GeneratedStatement,
}

impl ReplayScript {
    /// The statements, borrowed for one call of the replay function.
    pub(crate) fn statements(&self) -> ReplayStatements<'_> {
        ReplayStatements {
            peek: &self.peek.0,
            insert: &self.insert.0,
            delete: &self.delete.0,
            update: &self.update.0,
            pop: &self.pop.0,
        }
    }
}

/// The tablespace a table lives in, which its copy is created in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TablespaceName(String);

impl TablespaceName {
    /// The name, as the copy's creation binds it.
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

/// Everything a run executes that the extension generated for one table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RepackTableScript {
    /// The table's OID.
    pub(crate) relid: u32,
    /// The composite type of the key the log identifies rows by.
    pub(crate) create_pktype: RepackScript,
    /// The log table.
    pub(crate) create_log: RepackScript,
    /// The trigger logging every write.
    pub(crate) create_trigger: RepackScript,
    /// The trigger enabled for every session, replicas included.
    pub(crate) enable_trigger: RepackScript,
    /// The tablespace the copy is created in.
    pub(crate) tablespace: TablespaceName,
    /// The copy of every row, in the order the target's [`RepackOrder`]
    /// names.
    pub(crate) copy: RepackScript,
    /// The copy's column storage, where it differs from the column types'.
    pub(crate) alter_col_storage: Option<RepackScript>,
    /// The copy's columns dropped where the table has dropped columns.
    pub(crate) drop_columns: Option<RepackScript>,
    /// The emptying of the log before the copy reads.
    pub(crate) delete_log: RepackScript,
    /// The statements the log is replayed with.
    pub(crate) replay: ReplayScript,
    /// Each valid index's definition, rewritten to build it on the copy.
    pub(crate) indexes: Vec<RepackScript>,
}

/// Why a table has no script a run could execute.
#[derive(Clone, Copy, Debug, Eq, PartialEq, strum::IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum ScriptAbsence {
    /// The extension does not list the table: not an ordinary, permanent
    /// table, or it does not resolve.
    NotListed,
    /// The table has neither a primary key nor a not-null unique key, which
    /// the log identifies rows by.
    NoKey,
    /// The index the copy is ordered by is missing.
    NoOrderingIndex,
}

/// Read the script of `heap` in `branch`, whose OID is `relid`, copying in
/// the heap's [`RepackOrder`].
pub(crate) async fn read_table_script(
    connection: &mut PgConnection,
    branch: Branch,
    heap: HeapTarget,
    relid: u32,
) -> Result<Result<RepackTableScript, ScriptAbsence>, MaintenanceError> {
    let Some(row) = table_script(relid).to_query().fetch_optional(&mut *connection).await? else {
        return Ok(Err(ScriptAbsence::NotListed));
    };
    if row.try_get::<Option<i64>, _>(column(RepackTables::Pkid).as_str())?.is_none() {
        return Ok(Err(ScriptAbsence::NoKey));
    }
    let copy_data = text(&row, RepackTables::CopyData)?;
    let copy = match heap.repack_order() {
        RepackOrder::Physical => RepackScript::generated(copy_data),
        RepackOrder::ClusteredBy(index) => {
            let order = order_by_index(&branch.qualified(index.relation_name()), relid)
                .to_query_scalar::<Option<String>>()
                .fetch_one(&mut *connection)
                .await?;
            let Some(order) = order else {
                return Ok(Err(ScriptAbsence::NoOrderingIndex));
            };
            RepackScript::ordered_copy(&copy_data, &order)
        }
    };
    let indexes = index_definitions(relid)
        .to_query_scalar::<String>()
        .fetch_all(&mut *connection)
        .await?
        .into_iter()
        .map(RepackScript::generated)
        .collect();
    Ok(Ok(RepackTableScript {
        relid,
        create_pktype: script(&row, RepackTables::CreatePktype)?,
        create_log: script(&row, RepackTables::CreateLog)?,
        create_trigger: script(&row, RepackTables::CreateTrigger)?,
        enable_trigger: script(&row, RepackTables::EnableTrigger)?,
        tablespace: TablespaceName(text(&row, RepackTables::TablespaceOrig)?),
        copy,
        alter_col_storage: optional_script(&row, RepackTables::AlterColStorage)?,
        drop_columns: optional_script(&row, RepackTables::DropColumns)?,
        delete_log: script(&row, RepackTables::DeleteLog)?,
        replay: ReplayScript {
            peek: generated(&row, RepackTables::SqlPeek)?,
            insert: generated(&row, RepackTables::SqlInsert)?,
            delete: generated(&row, RepackTables::SqlDelete)?,
            update: generated(&row, RepackTables::SqlUpdate)?,
            pop: generated(&row, RepackTables::SqlPop)?,
        },
        indexes,
    }))
}

/// The name a column of the view is read under.
fn column(column: RepackTables) -> String {
    column.to_string()
}

/// One text column of the script row.
fn text(row: &PgRow, name: RepackTables) -> Result<String, MaintenanceError> {
    Ok(row.try_get::<String, _>(column(name).as_str())?)
}

/// One statement column of the script row, run as generated.
fn script(row: &PgRow, name: RepackTables) -> Result<RepackScript, MaintenanceError> {
    text(row, name).map(RepackScript::generated)
}

/// One statement column the extension leaves NULL when the table needs none.
fn optional_script(row: &PgRow, name: RepackTables) -> Result<Option<RepackScript>, MaintenanceError> {
    Ok(row.try_get::<Option<String>, _>(column(name).as_str())?.map(RepackScript::generated))
}

/// One replay statement column, passed back as text.
fn generated(row: &PgRow, name: RepackTables) -> Result<GeneratedStatement, MaintenanceError> {
    text(row, name).map(GeneratedStatement)
}

#[cfg(test)]
mod tests {
    use crate::repack::table_script::{GeneratedStatement, ReplayScript, ScriptAbsence};
    use andromeda_postgresql_seaquery::repack::ReplayStatements;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_replay_statements_are_passed_back_in_the_order_the_extension_takes_them() {
        let replay = ReplayScript {
            peek: GeneratedStatement("peek".to_owned()),
            insert: GeneratedStatement("insert".to_owned()),
            delete: GeneratedStatement("delete".to_owned()),
            update: GeneratedStatement("update".to_owned()),
            pop: GeneratedStatement("pop".to_owned()),
        };
        let ReplayStatements {
            peek,
            insert,
            delete,
            update,
            pop,
        } = replay.statements();
        assert_eq!([peek, insert, delete, update, pop], ["peek", "insert", "delete", "update", "pop"]);
    }

    #[test]
    fn each_absence_is_named_for_the_log() {
        let names: [&'static str; 3] = [ScriptAbsence::NotListed, ScriptAbsence::NoKey, ScriptAbsence::NoOrderingIndex].map(Into::into);
        assert_eq!(names, ["not-listed", "no-key", "no-ordering-index"]);
    }
}
