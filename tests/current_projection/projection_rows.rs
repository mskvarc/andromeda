//! Direct reads of the current-attribute projection for one Entity.

use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    name_dictionary::AttributeNames,
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    postgis::st_as_text,
    statement::Statement,
};
use cefact_units::UnitCode;
use sea_query::{Alias, Expr, ExprTrait, Func, NullOrdering, Order, Query, SelectStatement};
use sqlx::{FromRow, PgPool};

/// The clause 4.5.5 grain of one projection row, with its typed value rendered
/// as text so a `NUMERIC` column needs no arbitrary-precision decoder on the
/// test side and still compares exactly.
#[derive(Debug, FromRow, PartialEq, Eq)]
pub struct ProjectionRow {
    /// Expanded attribute IRI taken from the `attribute_names` dictionary.
    pub attribute_name: String,
    /// `datasetId` of the instance, absent for the default instance.
    pub dataset_id: Option<String>,
    /// Migration-owned attribute-kind code (1 Property … 8 `ListRelationship`).
    pub attribute_kind: i16,
    /// Migration-owned value-kind code (1 Number … 7 Uri).
    pub value_kind: i16,
    /// The populated typed value column rendered as text.
    pub value_text: Option<String>,
}

/// The current Entity rows with identifier `entity_id`, before any column is
/// selected.
fn entities_where_id(entity_id: &str) -> SelectStatement {
    Query::select()
        .from(Entities::Table)
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(entity_id))
        .to_owned()
}

/// A projection column of the current attribute rows.
fn projected(column: CurrentAttributes) -> Expr {
    Expr::col((CurrentAttributes::Table, column))
}

/// The projection rows owned by the Entity `entity_id`, joined with the
/// dictionary name of each row's attribute, before any column is selected.
fn attribute_rows_of(entity_id: &str) -> SelectStatement {
    entities_where_id(entity_id)
        .inner_join(
            CurrentAttributes::Table,
            projected(CurrentAttributes::EntityKey).equals((Entities::Table, Entities::EntityKey)),
        )
        .inner_join(
            AttributeNames::Table,
            Expr::col((AttributeNames::Table, AttributeNames::AttributeNameKey)).equals((CurrentAttributes::Table, CurrentAttributes::AttributeNameKey)),
        )
        .to_owned()
}

/// The count of the current Entity rows with identifier `entity_id`.
pub fn entities_with_id(entity_id: &str) -> SelectStatement {
    entities_where_id(entity_id).expr(count_all()).to_owned()
}

/// The statement projecting one Entity's grains in a stable order.
fn rows_for_entity_statement(entity_id: &str) -> SelectStatement {
    let text = |column: CurrentAttributes| cast(projected(column), PostgresType::Text);
    attribute_rows_of(entity_id)
        .expr_as(Expr::col((AttributeNames::Table, AttributeNames::Name)), Alias::new("attribute_name"))
        .column((CurrentAttributes::Table, CurrentAttributes::DatasetId))
        .column((CurrentAttributes::Table, CurrentAttributes::AttributeKind))
        .column((CurrentAttributes::Table, CurrentAttributes::ValueKind))
        .expr_as(
            Func::coalesce([
                text(CurrentAttributes::ValueNumber),
                projected(CurrentAttributes::ValueString),
                projected(CurrentAttributes::ValueLongText),
                text(CurrentAttributes::ValueBoolean),
                text(CurrentAttributes::ValueDateTime),
                projected(CurrentAttributes::ValueUri),
                projected(CurrentAttributes::ValueLongUri),
                text(CurrentAttributes::ValueObject),
                st_as_text(projected(CurrentAttributes::ValueGeometry)),
            ]),
            Alias::new("value_text"),
        )
        .order_by((AttributeNames::Table, AttributeNames::Name), Order::Asc)
        .order_by_with_nulls((CurrentAttributes::Table, CurrentAttributes::DatasetId), Order::Asc, NullOrdering::First)
        .to_owned()
}

/// Read every projection row owned by one Entity identifier.
pub async fn rows_for_entity(pool: &PgPool, entity_id: &str) -> Vec<ProjectionRow> {
    rows_for_entity_statement(entity_id)
        .to_query_as()
        .fetch_all(pool)
        .await
        .expect("projection rows must be readable")
}

/// Read the rows of one attribute of one Entity, matched by the local name at
/// the end of its expanded IRI.
pub async fn rows_for_attribute(pool: &PgPool, entity_id: &str, local_name: &str) -> Vec<ProjectionRow> {
    rows_for_entity(pool, entity_id)
        .await
        .into_iter()
        .filter(|row| row.attribute_name.ends_with(local_name))
        .collect()
}

/// Read the surrogate key of one Entity, absent when no current Entity exists.
pub async fn entity_key(pool: &PgPool, entity_id: &str) -> Option<i64> {
    entities_where_id(entity_id)
        .column(Entities::EntityKey)
        .to_query_scalar()
        .fetch_optional(pool)
        .await
        .expect("entity lookup must succeed")
}

/// Count the projection rows carrying one surrogate key, including rows whose
/// Entity has already been deleted.
pub async fn rows_for_key(pool: &PgPool, key: i64) -> i64 {
    Query::select()
        .expr(count_all())
        .from(CurrentAttributes::Table)
        .and_where(Expr::col(CurrentAttributes::EntityKey).eq(key))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("projection row count must be readable")
}

/// Whether an independent temporal Entity exists for one identifier.
pub async fn temporal_entity_exists(pool: &PgPool, entity_id: &str) -> bool {
    let count: i64 = Query::select()
        .expr(count_all())
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(entity_id))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("temporal entity count must be readable");
    count > 0
}

/// The default-instance rows of the attribute whose expanded IRI ends in
/// `local_name`, for one Entity, before any column is selected.
fn default_instance_of(entity_id: &str, local_name: &str) -> SelectStatement {
    attribute_rows_of(entity_id)
        .and_where(projected(CurrentAttributes::DatasetId).is_null())
        .and_where(Expr::col((AttributeNames::Table, AttributeNames::Name)).like(format!("%{local_name}")))
        .to_owned()
}

/// Read which of the two String columns holds the default instance of one
/// attribute, matched by the local name at the end of its expanded IRI, as
/// `(value_string, value_long_text)`. Exactly one side is present for a String
/// value: the indexed column up to the B-tree bound, the overflow column past it.
pub async fn string_placement(pool: &PgPool, entity_id: &str, local_name: &str) -> (Option<String>, Option<String>) {
    default_instance_of(entity_id, local_name)
        .column((CurrentAttributes::Table, CurrentAttributes::ValueString))
        .column((CurrentAttributes::Table, CurrentAttributes::ValueLongText))
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the default instance of the attribute must be projected")
}

/// Read which of the two URI columns holds the default instance of one
/// attribute, matched by the local name at the end of its expanded IRI, as
/// `(value_uri, value_long_uri)`. Exactly one side is present for a
/// Relationship object: the traversed column up to the identifier bound, the
/// overflow column past it.
pub async fn uri_placement(pool: &PgPool, entity_id: &str, local_name: &str) -> (Option<String>, Option<String>) {
    default_instance_of(entity_id, local_name)
        .column((CurrentAttributes::Table, CurrentAttributes::ValueUri))
        .column((CurrentAttributes::Table, CurrentAttributes::ValueLongUri))
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the default instance of the attribute must be projected")
}

/// Resolve the dictionary identifier of an expanded attribute IRI ending in the
/// supplied local name for one Entity.
pub async fn attribute_name_key(pool: &PgPool, entity_id: &str, local_name: &str) -> i32 {
    attribute_rows_of(entity_id)
        .column((CurrentAttributes::Table, CurrentAttributes::AttributeNameKey))
        .and_where(Expr::col((AttributeNames::Table, AttributeNames::Name)).like(format!("%{local_name}")))
        .limit(1)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the attribute must be present in the dictionary")
}

/// Read the `unitCode` projected for one instance of one attribute, matched by
/// the local name at the end of its expanded IRI, absent when the projected
/// instance carries none.
pub async fn unit_code_of(pool: &PgPool, entity_id: &str, local_name: &str, dataset_id: Option<&str>) -> Option<String> {
    let stored: Option<i16> = attribute_rows_of(entity_id)
        .column((CurrentAttributes::Table, CurrentAttributes::UnitCodeId))
        .and_where(Expr::col((AttributeNames::Table, AttributeNames::Name)).like(format!("%{local_name}")))
        .and_where(projected(CurrentAttributes::DatasetId).not_distinct_from(Expr::val(dataset_id)))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the instance of the attribute must be projected");
    stored.map(|identity| {
        UnitCode::from_smallint(identity)
            .expect("the stored unit identity names a listed code")
            .as_str()
            .to_owned()
    })
}
