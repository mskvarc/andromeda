//! The staging shape every entity set-write reads its parallel arrays through.
//!
//! Each statement stages its rows with `UNNEST` under the alias `d`, then
//! projects them once, in a common table expression `proj`, into the typed
//! columns the three entity relations store: the JSONB key arrays become
//! `integer[]` and `ltree[]`, the payload text becomes `jsonb`, the EWKB
//! geometries are validated. The relation statements that follow read `proj`
//! rather than repeating the projection, so a geometry is validated once per
//! row however many relations it lands in.

use andromeda_postgresql_schema::current::{Entities, EntityDocuments, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_array_elements_text,
    pg_array::array_subquery,
    pg_type::PostgresType,
    placeholder::external_placeholder,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestColumn, unnest_call},
};
use sea_query::{CaseStatement, CommonTableExpression, Cond, Expr, ExprTrait, FunctionCall, Iden, IntoIden, Keyword, Query, SelectStatement, SimpleExpr};

/// The staging alias, `d`, each set-write reads its parallel arrays under.
///
/// Spelled nowhere else in a statement, so the single `AS "d"` its table source
/// emits is the one `apply_column_alias` rewrites into the column-list alias.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
pub(super) struct Staged;

/// The projection CTE, `proj`: one row per staged entity, typed as stored.
#[derive(Iden, Clone, Copy)]
#[iden = "proj"]
pub(super) struct Proj;

/// The insert CTE over `entities`, `ins`, whose returned keys drive the two
/// child inserts.
#[derive(Iden, Clone, Copy)]
#[iden = "ins"]
pub(super) struct Ins;

/// The `entity_documents` CTE, `docs`: the update's compare-and-set anchor.
#[derive(Iden, Clone, Copy)]
#[iden = "docs"]
pub(super) struct Docs;

/// The `entity_search` CTE, `srch`.
#[derive(Iden, Clone, Copy)]
#[iden = "srch"]
pub(super) struct Srch;

/// The `entities` lifecycle CTE, `ents`.
#[derive(Iden, Clone, Copy)]
#[iden = "ents"]
pub(super) struct Ents;

/// The delete's candidate CTE, `doomed`: the rows about to go, with what the
/// deletion notification needs from them.
#[derive(Iden, Clone, Copy)]
#[iden = "doomed"]
pub(super) struct Doomed;

/// The staging columns of the entity set-writes, named as the `AS d(…)` alias
/// spells them rather than as the columns they land in.
///
/// The key and scope arrays arrive as JSONB (`entity_type_name_keys_json`,
/// `attribute_name_keys_json`, `scope_paths_json`) and are expanded to
/// `integer[]`/`ltree[]` in the projection; the geometries arrive as EWKB
/// (`loc`, `obs`, `ops`) and are validated there. Not every statement uses
/// every column: the delete stages only `id` and the expectation.
#[derive(Iden, Clone, Copy)]
pub(super) enum StagedColumn {
    /// Entity identifier.
    Id,
    /// Type dictionary-key array as JSONB.
    EntityTypeNameKeysJson,
    /// Attribute-name dictionary-key array as JSONB.
    AttributeNameKeysJson,
    /// Scope-path array as JSONB, in `ltree` dot notation.
    ScopePathsJson,
    /// Payload as JSON text, cast to `jsonb` in the projection.
    Payload,
    /// Default `GeoProperty` geometry as EWKB.
    Loc,
    /// `observationSpace` geometry as EWKB.
    Obs,
    /// `operationSpace` geometry as EWKB.
    Ops,
    /// The instant after which every `location` instance has expired.
    LocExpiresAt,
    /// Expiration timestamp.
    ExpiresAt,
    /// Earliest attribute-instance expiration timestamp.
    MinAttributeExpiresAt,
    /// The `modifiedAt` the caller read, or NULL for an unconditional write.
    ExpectedModifiedAt,
}

/// A staging column reference, `d.<column>`.
pub(super) fn staged(column: StagedColumn) -> SimpleExpr {
    Expr::col((Staged, column))
}

/// A projected column reference, `proj.<column>`.
pub(super) fn projected(column: impl IntoIden) -> SimpleExpr {
    Expr::col((Proj, column.into_iden()))
}

/// The validated-geometry projection of a staged EWKB column:
/// `CASE WHEN d.<col> IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB(d.<col>)) ELSE NULL END`.
///
/// The EWKB already carries its SRID, so no re-SRID is applied, and a NULL EWKB
/// projects to a NULL geometry rather than a call on a NULL argument.
fn geometry(column: StagedColumn) -> SimpleExpr {
    CaseStatement::new()
        .case(Expr::col((Staged, column)).is_not_null(), validated_geom_from_ewkb(staged(column)))
        .finally(Keyword::Null)
        .into()
}

/// The `integer[]`/`ltree[]` projection of a staged JSONB array column:
/// `ARRAY(SELECT jsonb_array_elements_text(d.<col>)::element_type)`.
fn array_of(column: StagedColumn, element_type: PostgresType) -> SimpleExpr {
    let element: SimpleExpr = jsonb_array_elements_text(staged(column)).into();
    array_subquery(&Query::select().expr(cast(element, element_type)).to_owned())
}

/// The eleven staging columns every insert and update stages, in bind order.
const fn value_staging_columns() -> [(StagedColumn, PostgresType); 11] {
    [
        (StagedColumn::Id, PostgresType::TextArray),
        (StagedColumn::EntityTypeNameKeysJson, PostgresType::JsonbArray),
        (StagedColumn::AttributeNameKeysJson, PostgresType::JsonbArray),
        (StagedColumn::ScopePathsJson, PostgresType::JsonbArray),
        (StagedColumn::Payload, PostgresType::TextArray),
        (StagedColumn::Loc, PostgresType::ByteaArray),
        (StagedColumn::Obs, PostgresType::ByteaArray),
        (StagedColumn::Ops, PostgresType::ByteaArray),
        (StagedColumn::LocExpiresAt, PostgresType::TimestamptzArray),
        (StagedColumn::ExpiresAt, PostgresType::TimestamptzArray),
        (StagedColumn::MinAttributeExpiresAt, PostgresType::TimestamptzArray),
    ]
}

/// The eleven staging columns of the insert, in bind order.
pub(super) fn insert_staging_columns() -> Vec<UnnestColumn> {
    value_staging_columns()
        .into_iter()
        .map(|(column, array_type)| UnnestColumn::new(column, array_type))
        .collect()
}

/// The twelve staging columns of the update: the insert's eleven plus the
/// expectation the compare-and-set reads.
pub(super) fn update_staging_columns() -> Vec<UnnestColumn> {
    let mut columns = insert_staging_columns();
    columns.push(UnnestColumn::new(StagedColumn::ExpectedModifiedAt, PostgresType::TimestamptzArray));
    columns
}

/// The two staging columns of the delete: the identifier and the expectation.
pub(super) fn delete_staging_columns() -> Vec<UnnestColumn> {
    vec![
        UnnestColumn::new(StagedColumn::Id, PostgresType::TextArray),
        UnnestColumn::new(StagedColumn::ExpectedModifiedAt, PostgresType::TimestamptzArray),
    ]
}

/// The `UNNEST($1::t[], …)` staging source over `columns`, bound positionally.
pub(super) fn staging_source(columns: &[UnnestColumn]) -> FunctionCall {
    unnest_call(columns.iter().enumerate().map(|(index, column)| (column, external_placeholder(index + 1))))
}

/// The projection CTE `proj` over the staged `columns`, whose output columns
/// carry the names of the relation columns they land in.
///
/// Whether the expectation is projected follows from whether it was staged, so
/// the insert and the update share this one builder.
pub(super) fn projection_cte(columns: &[UnnestColumn], expectation: ExpectationStaged) -> CommonTableExpression {
    let mut select = Query::select();
    select
        .expr_as(staged(StagedColumn::Id), Entities::Id)
        .expr_as(
            array_of(StagedColumn::EntityTypeNameKeysJson, PostgresType::Integer),
            EntitySearch::EntityTypeNameKeys,
        )
        .expr_as(
            array_of(StagedColumn::AttributeNameKeysJson, PostgresType::Integer),
            EntitySearch::AttributeNameKeys,
        )
        .expr_as(array_of(StagedColumn::ScopePathsJson, PostgresType::Ltree), EntitySearch::ScopePaths)
        .expr_as(cast(staged(StagedColumn::Payload), PostgresType::Jsonb), EntityDocuments::Payload)
        .expr_as(geometry(StagedColumn::Loc), EntitySearch::Location)
        .expr_as(geometry(StagedColumn::Obs), EntitySearch::ObservationSpace)
        .expr_as(geometry(StagedColumn::Ops), EntitySearch::OperationSpace)
        .expr_as(staged(StagedColumn::LocExpiresAt), EntitySearch::LocationExpiresAt)
        .expr_as(staged(StagedColumn::ExpiresAt), Entities::ExpiresAt)
        .expr_as(staged(StagedColumn::MinAttributeExpiresAt), Entities::MinAttributeExpiresAt);
    match expectation {
        ExpectationStaged::Yes => {
            select.expr(staged(StagedColumn::ExpectedModifiedAt));
        }
        ExpectationStaged::No => {}
    }
    select.from_function(staging_source(columns), Staged);
    let select: SelectStatement = select.take();
    CommonTableExpression::new().table_name(Proj).query(select).to_owned()
}

/// Whether a statement stages the caller's expectation beside its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExpectationStaged {
    /// The update: the expectation is staged and projected.
    Yes,
    /// The insert: nothing to compare against.
    No,
}

/// The precondition the update and the delete carry:
/// `<expected> IS NULL OR entity_documents.modified_at = <expected>`,
/// with `expected` read from wherever the statement carries it (`d` for the
/// delete, `proj` for the update).
///
/// A null expectation matches whatever is there; a non-null one matches only a
/// row still standing as the caller read it, which is what turns a
/// read-modify-write into a compare-and-set rather than a lost update. The
/// token compared is `entity_documents.modified_at`: the version of the payload
/// is what a caller read and what it states.
pub(super) fn expectation(expected: SimpleExpr, documents_alias: impl IntoIden) -> Cond {
    let documents_alias = documents_alias.into_iden();
    Cond::any()
        .add(expected.clone().is_null())
        .add(Expr::col((documents_alias, EntityDocuments::ModifiedAt)).eq(expected)) // clone: the expectation is read twice, once for its presence and once for its value
}
