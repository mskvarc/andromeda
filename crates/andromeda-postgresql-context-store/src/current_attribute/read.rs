//! Reading an Entity's `current_attributes` rows back.
//!
//! Rows are read without an `ORDER BY`: assembly puts them in written order by
//! their ordinal, which costs less than sorting wide rows in the statement. A
//! row's string and URI each read as one column, whichever side of the length
//! split holds them.

use crate::{current_attribute::row::CurrentAttributeRow, entity::stored_entity_error::StoredEntityError, error::ContextStoreError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
use andromeda_postgresql_attribute_instance::{
    stored_members::{InstanceMembers, MemberColumns, StoredInstanceError},
    sub_attribute::{SubAttributeElement, SubAttributeRecord},
};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::entity_key::EntityKey;
use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dataset_key::DatasetKey, dictionary::resolved_names::ResolvedNames};
use andromeda_postgresql_schema::current::CurrentAttributes;
use andromeda_postgresql_seaquery::pg_array::{is_any_of, is_none_of};
use chrono::{DateTime, Utc};
use iri_rs::IriBuf;
use sea_query::{Expr, Func, Iden, Query, SelectStatement, SimpleExpr};
use sqlx::{FromRow, Row, postgres::PgRow};

/// The name a statement reads rows under, `ca`.
#[derive(Iden, Clone, Copy)]
#[iden = "ca"]
pub struct Rows;

/// The rows of an Entity a read takes, by attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowSelection {
    /// Every row.
    Every,
    /// The rows of these attributes; none when the list is empty.
    Named(Vec<AttributeNameKey>),
    /// The rows of every attribute but these.
    AllBut(Vec<AttributeNameKey>),
}

impl RowSelection {
    /// The rows holding the Attributes `returned` lets a response hold, by
    /// the keys `attributes` resolved. A name with no key is held by no row:
    /// it takes nothing when named and sets nothing apart when excluded.
    #[must_use]
    pub fn of(returned: &ReturnedAttributes<'_>, attributes: &ResolvedNames<AttributeName>) -> RowSelection {
        let keys = |iris: &[&IriBuf]| iris.iter().filter_map(|iri| attributes.get_str(iri.as_str())).collect();
        match returned {
            ReturnedAttributes::Every => RowSelection::Every,
            ReturnedAttributes::Named(iris) => RowSelection::Named(keys(iris)),
            ReturnedAttributes::AllBut(iris) => RowSelection::AllBut(keys(iris)),
        }
    }

    /// Whether the selection takes no row at all.
    #[must_use]
    pub const fn takes_none(&self) -> bool {
        match self {
            RowSelection::Named(keys) => keys.is_empty(),
            RowSelection::Every | RowSelection::AllBut(_) => false,
        }
    }

    /// The condition on `ca.attribute_name_key` the selection reads rows by,
    /// `None` when it takes every row.
    #[must_use]
    pub fn condition(&self) -> Option<SimpleExpr> {
        let key = || Expr::col((Rows, CurrentAttributes::AttributeNameKey));
        let bound = |keys: &[AttributeNameKey]| keys.iter().map(|key| key.get()).collect::<Vec<i32>>();
        match self {
            RowSelection::Every => None,
            RowSelection::AllBut(keys) if keys.is_empty() => None,
            RowSelection::Named(keys) => Some(is_any_of(key(), bound(keys))),
            RowSelection::AllBut(keys) => Some(is_none_of(key(), bound(keys))),
        }
    }
}

/// Add the columns a row is decoded from to `select`, read from [`Rows`]
/// under the names [`CurrentAttributeRecord`] reads them by.
pub fn select_row_columns(select: &mut SelectStatement) {
    for column in [
        CurrentAttributes::Ordinal,
        CurrentAttributes::CreatedAt,
        CurrentAttributes::ModifiedAt,
        CurrentAttributes::SubAttributes,
        CurrentAttributes::AttributeNameKey,
        CurrentAttributes::DatasetKey,
        CurrentAttributes::AttributeKind,
        CurrentAttributes::ValueKind,
        CurrentAttributes::ValueNumber,
        CurrentAttributes::ValueBoolean,
        CurrentAttributes::ValueDateTime,
        CurrentAttributes::ValueGeometry,
        CurrentAttributes::ValueGeometryMembers,
        CurrentAttributes::ValueObject,
        CurrentAttributes::ValueDatatypeKey,
        CurrentAttributes::ValueLanguage,
        CurrentAttributes::ValueTypeKey,
        CurrentAttributes::ObjectTypeKeys,
        CurrentAttributes::UnitCodeId,
        CurrentAttributes::ObservedAt,
        CurrentAttributes::ExpiresAt,
    ] {
        select.column((Rows, column));
    }
    for (short, long) in [
        (CurrentAttributes::ValueString, CurrentAttributes::ValueLongText),
        (CurrentAttributes::ValueUri, CurrentAttributes::ValueLongUri),
    ] {
        select.expr_as(Func::coalesce([Expr::col((Rows, short)), Expr::col((Rows, long))]), short);
    }
}

/// The rows of the Entities `keys` names that `selection` takes, each with its
/// Entity's key:
///
/// ```sql
/// SELECT ca.entity_key, <row columns> FROM current_attributes ca
/// WHERE ca.entity_key = ANY($1) [AND ca.attribute_name_key = ANY($2) | <> ALL($2)]
/// ```
#[must_use]
pub fn rows_of(target: &ContextTarget, keys: &[EntityKey], selection: &RowSelection) -> SelectStatement {
    let mut select = Query::select();
    select.column((Rows, CurrentAttributes::EntityKey));
    select_row_columns(&mut select);
    select
        .from_as(table_of(target, CurrentAttributes::Table), Rows)
        .and_where(is_any_of(Expr::col((Rows, CurrentAttributes::EntityKey)), keys.iter().map(|key| key.get())));
    if let Some(condition) = selection.condition() {
        select.and_where(condition);
    }
    select
}

/// One row's columns, as [`select_row_columns`] reads them.
#[derive(Debug, FromRow)]
pub struct CurrentAttributeRecord {
    /// `ordinal`.
    pub ordinal: i32,
    /// `created_at`.
    pub created_at: DateTime<Utc>,
    /// `modified_at`.
    pub modified_at: DateTime<Utc>,
    /// `sub_attributes`, null when the instance has none.
    pub sub_attributes: Option<Vec<SubAttributeRecord>>,
    /// `dataset_key`, the grain key of the instance's `datasetId`.
    pub dataset_key: String,
    /// The other members.
    #[sqlx(flatten)]
    pub members: MemberColumns,
}

impl TryFrom<CurrentAttributeRecord> for CurrentAttributeRow {
    type Error = StoredInstanceError;

    fn try_from(record: CurrentAttributeRecord) -> Result<CurrentAttributeRow, StoredInstanceError> {
        Ok(CurrentAttributeRow {
            ordinal: record.ordinal,
            members: InstanceMembers::read(DatasetKey::stored(&record.dataset_key).dataset_id()?, record.members)?,
            created_at: record.created_at,
            modified_at: record.modified_at,
            sub_attributes: record
                .sub_attributes
                .unwrap_or_default()
                .into_iter()
                .map(SubAttributeElement::try_from)
                .collect::<Result<_, _>>()?,
        })
    }
}

/// The row a statement read through [`select_row_columns`], or `None` for the
/// nulls a left join gives an Entity without rows: `ordinal` is never null on
/// a stored row.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when a column does not decode, or when the
/// row or one of its elements breaks its invariants.
pub fn row_of(row: &PgRow) -> Result<Option<CurrentAttributeRow>, ContextStoreError> {
    if row.try_get::<Option<i32>, _>(CurrentAttributes::Ordinal.unquoted())?.is_none() {
        return Ok(None);
    }
    let record = CurrentAttributeRecord::from_row(row)?;
    Ok(Some(
        CurrentAttributeRow::try_from(record).map_err(|error| StoredEntityError::Row(Box::new(error)))?,
    ))
}

#[cfg(test)]
mod tests {
    use crate::current_attribute::read::{RowSelection, rows_of};
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::projection::returned_attributes::ReturnedAttributes;
    use andromeda_postgresql_codes::entity_key::EntityKey;
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::resolved_names::ResolvedNames};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value};

    fn keys() -> Vec<EntityKey> {
        [3_i64, 1, 2]
            .into_iter()
            .map(|key| EntityKey::try_from(key).expect("a surrogate key"))
            .collect()
    }

    fn names(keys: &[i32]) -> Vec<AttributeNameKey> {
        keys.iter().copied().map(AttributeNameKey::from).collect()
    }

    /// The page's order is restored by assembly, so the rows of a page are
    /// read by one membership test on the grain index's leading column and
    /// never sorted.
    #[test]
    fn a_page_reads_instances_without_a_sort() {
        let (sql, values) = rows_of(&ContextTarget::Live, &keys(), &RowSelection::Every).build(PostgresQueryBuilder);
        assert!(sql.starts_with(r#"SELECT "ca"."entity_key", "ca"."ordinal""#), "{sql}");
        assert!(sql.ends_with(r#"FROM "current_attributes" AS "ca" WHERE "ca"."entity_key" = ANY($1)"#), "{sql}");
        assert!(!sql.contains("ORDER BY"), "{sql}");
        assert_eq!(values.0, vec![Value::from(vec![3_i64, 1, 2])]);
    }

    /// The length split is invisible to a read: each pair reads as one column.
    #[test]
    fn a_row_reads_its_string_and_uri_whichever_column_holds_them() {
        let sql = rows_of(&ContextTarget::Live, &keys(), &RowSelection::Every).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"COALESCE("ca"."value_string", "ca"."value_long_text") AS "value_string""#),
            "{sql}"
        );
        assert!(sql.contains(r#"COALESCE("ca"."value_uri", "ca"."value_long_uri") AS "value_uri""#), "{sql}");
    }

    /// Clause 4.21: `attrs=a,b` returns `a` and `b` alone, so only their rows
    /// are read.
    #[test]
    fn attrs_pushes_the_names_into_stage_two() {
        let (sql, values) = rows_of(&ContextTarget::Live, &keys(), &RowSelection::Named(names(&[4, 7]))).build(PostgresQueryBuilder);
        assert!(
            sql.ends_with(r#"WHERE "ca"."entity_key" = ANY($1) AND "ca"."attribute_name_key" = ANY($2)"#),
            "{sql}"
        );
        assert_eq!(values.0[1], Value::from(vec![4_i32, 7]));
    }

    /// Clause 4.21: `omit=a` returns every attribute but `a`.
    #[test]
    fn omit_pushes_the_exclusion_into_stage_two() {
        let (sql, values) = rows_of(&ContextTarget::Live, &keys(), &RowSelection::AllBut(names(&[4]))).build(PostgresQueryBuilder);
        assert!(sql.ends_with(r#"AND "ca"."attribute_name_key" <> $2"#), "{sql}");
        assert_eq!(values.0[1], Value::from(4_i32));
    }

    /// An omission naming no stored attribute leaves every row read.
    #[test]
    fn an_exclusion_of_nothing_reads_every_row() {
        assert_eq!(RowSelection::AllBut(Vec::new()).condition(), None);
        assert!(!RowSelection::AllBut(Vec::new()).takes_none());
        assert!(RowSelection::Named(Vec::new()).takes_none());
    }

    fn iri(name: &str) -> IriBuf {
        IriBuf::new(format!("https://example.com/{name}")).expect("valid IRI")
    }

    fn known() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs(
            [("a", 4), ("b", 7)]
                .into_iter()
                .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("a name"), AttributeNameKey::from(key))),
        )
    }

    #[test]
    fn named_attributes_take_the_rows_of_those_the_dictionary_holds() {
        let (a, b, never) = (iri("a"), iri("b"), iri("never-written"));
        assert_eq!(
            RowSelection::of(&ReturnedAttributes::Named(vec![&a, &b, &never]), &known()),
            RowSelection::Named(names(&[4, 7]))
        );
    }

    /// Clause 4.21: `pick=id,type` returns no attribute, so no row is read.
    #[test]
    fn no_named_attribute_takes_no_row() {
        assert!(RowSelection::of(&ReturnedAttributes::Named(Vec::new()), &known()).takes_none());
    }

    #[test]
    fn excluded_attributes_take_every_row_but_theirs() {
        let (a, never) = (iri("a"), iri("never-written"));
        assert_eq!(
            RowSelection::of(&ReturnedAttributes::AllBut(vec![&a, &never]), &known()),
            RowSelection::AllBut(names(&[4]))
        );
    }

    #[test]
    fn every_attribute_takes_every_row() {
        assert_eq!(RowSelection::of(&ReturnedAttributes::Every, &known()), RowSelection::Every);
    }

    #[test]
    fn a_branch_reads_its_rows_from_its_own_schema() {
        use andromeda_context_space::{snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
        let key = SnapshotKey::try_from(5).expect("positive key");
        let target = ContextTarget::Snapshot(SnapshotTarget::staging(key, "urn:ngsi-ld:Snapshot:s".parse().expect("valid snapshot uri")));
        let sql = rows_of(&target, &keys(), &RowSelection::Every).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"FROM "snap_5_stg"."current_attributes" AS "ca""#), "{sql}");
    }
}
