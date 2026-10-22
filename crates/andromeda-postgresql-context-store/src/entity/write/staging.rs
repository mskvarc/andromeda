//! The staging shape every entity set-write reads its parallel arrays through.
//!
//! Each statement stages its rows with `UNNEST` under the alias `d`, then
//! projects them once, in a common table expression `proj`, into the typed
//! columns the entity relations store: the key array literals become
//! `integer[]`, the JSONB scope arrays `ltree[]` and `text[]`, the EWKB
//! geometries are validated. The relation statements that follow read `proj`
//! rather than repeating the projection, so a geometry is validated once per
//! row however many relations it lands in. The attribute instances are staged
//! apart, one row each, keyed by the entity identifier
//! ([`instances_cte`](crate::current_attribute::staging::instances_cte)).

use crate::entity::params::{PreparedEntityColumns, literal_texts};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::current::{Entities, EntitySearch};
use andromeda_postgresql_seaquery::{
    cast::cast,
    external_arguments::ExternalArguments,
    jsonb::jsonb_array_elements_text,
    pg_array::{array_of_subquery, nullable_array, text_array},
    pg_type::PostgresType,
    postgis::validated_geom_from_ewkb,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{
    CaseStatement,
    CommonTableExpression,
    Cond,
    Expr,
    ExprTrait,
    FunctionCall,
    Iden,
    IntoIden,
    JoinType,
    Keyword,
    Query,
    SelectStatement,
    SimpleExpr,
};

/// The staging alias, `d`, each set-write reads its parallel arrays under.
///
/// Spelled nowhere else in a statement, so the single `AS "d"` its table source
/// emits is the one `with_staged_alias` gives the column-list alias.
#[derive(Iden, Clone, Copy)]
#[iden = "d"]
pub(super) struct Staged;

/// The projection CTE, `proj`: one row per staged entity, typed as stored.
#[derive(Iden, Clone, Copy)]
#[iden = "proj"]
pub(super) struct Proj;

/// The insert CTE over `entities`, `ins`, whose returned keys drive the child
/// inserts.
#[derive(Iden, Clone, Copy)]
#[iden = "ins"]
pub(super) struct Ins;

/// The `entity_search` CTE, `srch`: the rows a set-write wrote there, returned
/// as the images ([`SearchImage`](crate::entity::write::search_image::SearchImage))
/// the memberships and the census are derived from.
#[derive(Iden, Clone, Copy)]
#[iden = "srch"]
pub(crate) struct Srch;

/// The `current_attributes` CTE, `attrs`: the attribute instance rows a
/// set-write lands.
#[derive(Iden, Clone, Copy)]
#[iden = "attrs"]
pub(super) struct Attrs;

/// The update's `current_attributes` delete CTE, `gone`: the stored instance
/// rows of an overwritten entity whose grain the written state no longer
/// holds.
#[derive(Iden, Clone, Copy)]
#[iden = "gone"]
pub(super) struct Gone;

/// The update's `current_attributes` delete CTE, `dropped`: the stored
/// instance rows whose grain an edit removes.
#[derive(Iden, Clone, Copy)]
#[iden = "dropped"]
pub(super) struct Dropped;

/// The update's `current_attributes` update CTE, `moved`: the stored instance
/// rows an edit gives a new ordinal and nothing else.
#[derive(Iden, Clone, Copy)]
#[iden = "moved"]
pub(super) struct Moved;

/// The update CTE over `entities`, `ents`: the update's compare-and-set
/// anchor, whose returned keys drive every other relation the update writes.
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
/// The key and instance arrays arrive as PostgreSQL array literals
/// (`entity_type_name_keys_literal`, `attribute_name_keys_literal`,
/// `attribute_instance_name_keys_literal`, `attribute_instance_kinds_literal`)
/// and are cast to `integer[]`/`smallint[]` in the projection; the scopes
/// arrive as JSONB (`scope_paths_json`, `verbatim_scope_paths_json`) and are
/// expanded to `ltree[]` and `text[]` there; the default `location` geometry
/// arrives as EWKB (`loc`) and is validated there. Not every statement uses
/// every column: the delete stages only `id` and the expectation, and only the
/// update stages the expectation and whether a row keeps its geometry
/// (`loc_kept`).
#[derive(Iden, Clone, Copy)]
pub(super) enum StagedColumn {
    /// Entity identifier.
    Id,
    /// Type dictionary-key array, as an array literal.
    EntityTypeNameKeysLiteral,
    /// Attribute-name dictionary-key array, as an array literal.
    AttributeNameKeysLiteral,
    /// Attribute-instance name dictionary-key array, as an array literal.
    AttributeInstanceNameKeysLiteral,
    /// Attribute-instance family code array, as an array literal aligned with
    /// the names.
    AttributeInstanceKindsLiteral,
    /// Scope-path array as JSONB, in `ltree` dot notation.
    ScopePathsJson,
    /// The same Scopes as JSONB, as written.
    VerbatimScopePathsJson,
    /// Default `GeoProperty` geometry as EWKB.
    Loc,
    /// The instant after which every `location` instance has expired.
    LocExpiresAt,
    /// Expiration timestamp.
    ExpiresAt,
    /// Earliest attribute-instance expiration timestamp.
    MinAttributeExpiresAt,
    /// The `modifiedAt` the caller read, or NULL for an unconditional write.
    ExpectedModifiedAt,
    /// Whether the row keeps its stored `location` geometry.
    LocKept,
}

/// The hoisted geometry column of the search row, with the staging columns
/// that carry it.
#[derive(Clone, Copy)]
pub(super) struct HoistedColumn {
    /// The `entity_search` column the geometry lands in.
    pub(super) stored: EntitySearch,
    /// The staging column carrying its EWKB.
    pub(super) ewkb: StagedColumn,
    /// The staging column saying whether an update keeps the stored geometry.
    pub(super) kept: StagedColumn,
}

/// The default `location` instance's geometry, the one geometry the search row
/// hoists: every other `GeoProperty` is matched through its attribute instance
/// rows, whose index is keyed by attribute name.
pub(super) const HOISTED_LOCATION: HoistedColumn = HoistedColumn {
    stored: EntitySearch::Location,
    ewkb: StagedColumn::Loc,
    kept: StagedColumn::LocKept,
};

/// A staging column reference, `d.<column>`.
pub(super) fn staged(column: StagedColumn) -> SimpleExpr {
    Expr::col((Staged, column))
}

/// A projected column reference, `proj.<column>`.
pub(super) fn projected(column: impl IntoIden) -> SimpleExpr {
    Expr::col((Proj, column.into_iden()))
}

/// `FROM ins JOIN proj ON proj.id = ins.id`, the source every child insert of
/// a create reads: the surrogate key the identity insert minted beside the
/// projected values of the same staged row.
pub(super) fn from_inserted() -> SelectStatement {
    Query::select()
        .from(Ins)
        .join(JoinType::Join, Proj, Expr::col((Proj, Entities::Id)).equals((Ins, Entities::Id)))
        .take()
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

/// The `ltree[]` projection of a staged JSONB array column:
/// `ARRAY (SELECT jsonb_array_elements_text(d.<col>)::element_type)`.
fn array_of(column: StagedColumn, element_type: PostgresType) -> SimpleExpr {
    let element: SimpleExpr = jsonb_array_elements_text(staged(column)).into();
    array_of_subquery(Query::select().expr(cast(element, element_type)).to_owned())
}

/// The eleven projected staging columns every insert and update stages, in
/// bind order.
const fn value_staging_columns() -> [(StagedColumn, PostgresType); 11] {
    [
        (StagedColumn::Id, PostgresType::TextArray),
        (StagedColumn::EntityTypeNameKeysLiteral, PostgresType::TextArray),
        (StagedColumn::AttributeNameKeysLiteral, PostgresType::TextArray),
        (StagedColumn::AttributeInstanceNameKeysLiteral, PostgresType::TextArray),
        (StagedColumn::AttributeInstanceKindsLiteral, PostgresType::TextArray),
        (StagedColumn::ScopePathsJson, PostgresType::JsonbArray),
        (StagedColumn::VerbatimScopePathsJson, PostgresType::JsonbArray),
        (StagedColumn::Loc, PostgresType::ByteaArray),
        (StagedColumn::LocExpiresAt, PostgresType::TimestamptzArray),
        (StagedColumn::ExpiresAt, PostgresType::TimestamptzArray),
        (StagedColumn::MinAttributeExpiresAt, PostgresType::TimestamptzArray),
    ]
}

/// The eleven projected staging columns of the insert, in bind order.
pub(super) fn insert_staging_columns() -> Vec<UnnestColumn> {
    value_staging_columns()
        .into_iter()
        .map(|(column, array_type)| UnnestColumn::new(column, array_type))
        .collect()
}

/// The thirteen projected staging columns of the update: the insert's eleven,
/// whether the hoisted geometry is kept, and the expectation the
/// compare-and-set holds.
pub(super) fn update_staging_columns() -> Vec<UnnestColumn> {
    let mut columns = insert_staging_columns();
    columns.push(UnnestColumn::new(HOISTED_LOCATION.kept, PostgresType::BooleanArray));
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

/// The eleven arrays the entity columns stage in `value_staging_columns`
/// order, as the expressions standing for them in the statement.
///
/// The scope arrays are `jsonb[]` values sea-query has no form for, so they
/// travel as external arguments pushed onto `external`. `locations` is the
/// hoisted geometry column the write stages: the EWKB of each row, or none.
pub(super) fn entity_arrays<'entities>(
    entities: &'entities PreparedEntityColumns,
    locations: Vec<Option<&[u8]>>,
    external: &mut ExternalArguments<'entities>,
) -> Vec<SimpleExpr> {
    let ids = entities.ids.iter().map(Identifier::as_str).collect::<Vec<_>>();
    vec![
        text_array(&ids),
        text_array(&literal_texts(&entities.entity_type_name_keys)),
        text_array(&literal_texts(&entities.attribute_name_keys)),
        text_array(&literal_texts(&entities.attribute_instance_name_keys)),
        text_array(&literal_texts(&entities.attribute_instance_kinds)),
        external.push(entities.scope_paths_bind()),
        external.push(entities.verbatim_scope_paths_bind()),
        nullable_array(locations.into_iter().map(|ewkb| ewkb.map(<[u8]>::to_vec))),
        nullable_array(entities.location_expires_ats.iter().copied()),
        nullable_array(entities.expires_ats.iter().copied()),
        nullable_array(entities.min_attribute_expires_ats.iter().copied()),
    ]
}

/// The `UNNEST(…)` staging source over `columns`, one array expression per
/// column in column order.
pub(super) fn staging_source(columns: &[UnnestColumn], arrays: Vec<SimpleExpr>) -> FunctionCall {
    unnest_call(columns.iter().zip(arrays))
}

/// The projection CTE `proj` over the staged `columns` and their `arrays`,
/// whose output columns carry the names of the relation columns they land in.
///
/// What only an update stages (whether the geometry is kept, and the
/// expectation) is projected for the update alone, so the insert and the
/// update share this one builder. A kept geometry stages no EWKB, so its
/// projection validates nothing.
pub(super) fn projection_cte(columns: &[UnnestColumn], arrays: Vec<SimpleExpr>, write: ProjectedWrite) -> CommonTableExpression {
    let mut select = Query::select();
    select
        .expr_as(staged(StagedColumn::Id), Entities::Id)
        .expr_as(
            cast(staged(StagedColumn::EntityTypeNameKeysLiteral), PostgresType::IntegerArray),
            EntitySearch::EntityTypeNameKeys,
        )
        .expr_as(
            cast(staged(StagedColumn::AttributeNameKeysLiteral), PostgresType::IntegerArray),
            EntitySearch::AttributeNameKeys,
        )
        .expr_as(
            cast(staged(StagedColumn::AttributeInstanceNameKeysLiteral), PostgresType::IntegerArray),
            EntitySearch::AttributeInstanceNameKeys,
        )
        .expr_as(
            cast(staged(StagedColumn::AttributeInstanceKindsLiteral), PostgresType::SmallintArray),
            EntitySearch::AttributeInstanceKinds,
        )
        .expr_as(array_of(StagedColumn::ScopePathsJson, PostgresType::Ltree), EntitySearch::ScopePaths)
        .expr_as(
            array_of(StagedColumn::VerbatimScopePathsJson, PostgresType::Text),
            EntitySearch::VerbatimScopePaths,
        )
        .expr_as(geometry(HOISTED_LOCATION.ewkb), HOISTED_LOCATION.stored)
        .expr_as(staged(StagedColumn::LocExpiresAt), EntitySearch::LocationExpiresAt)
        .expr_as(staged(StagedColumn::ExpiresAt), Entities::ExpiresAt)
        .expr_as(staged(StagedColumn::MinAttributeExpiresAt), Entities::MinAttributeExpiresAt);
    match write {
        ProjectedWrite::Update => {
            select.expr(staged(HOISTED_LOCATION.kept)).expr(staged(StagedColumn::ExpectedModifiedAt));
        }
        ProjectedWrite::Insert => {}
    }
    select.from_function(staging_source(columns, arrays), Staged);
    let select: SelectStatement = select.take();
    CommonTableExpression::new().table_name(Proj).query(select).to_owned()
}

/// `statement`, the projection source named `d(<columns>)` after its columns.
pub(super) fn with_staged_alias<S>(statement: UnnestAliased<S>, columns: &[UnnestColumn]) -> UnnestAliased<S> {
    statement.with_columns(Staged, columns)
}

/// Which set-write a projection serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProjectedWrite {
    /// The update: whether the geometry is kept, and the expectation, are
    /// staged and projected.
    Update,
    /// The insert: nothing to compare against and no stored row to keep from.
    Insert,
}

/// The precondition the update and the delete carry:
/// `<expected> IS NULL OR <entities>.modified_at = <expected>`, with
/// `expected` read from wherever the statement carries it (`d` for the delete,
/// `proj` for the update) and `entities` the name the statement reads the
/// identity row under.
///
/// A null expectation matches whatever is there; a non-null one matches only a
/// row still standing as the caller read it, which is what turns a
/// read-modify-write into a compare-and-set rather than a lost update. The
/// token compared is the clause 4.8 `modifiedAt` of the entity, which every
/// write stamps: it is what a caller read and what it states.
pub(super) fn expectation(expected: SimpleExpr, entities_alias: impl IntoIden) -> Cond {
    let entities_alias = entities_alias.into_iden();
    Cond::any()
        .add(expected.clone().is_null())
        .add(Expr::col((entities_alias, Entities::ModifiedAt)).eq(expected)) // clone: the expectation is read twice, once for its presence and once for its value
}
