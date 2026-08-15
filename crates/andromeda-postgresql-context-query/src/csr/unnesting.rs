//! The `information` document unnesting shared by CSR discovery and coverage.
//!
//! A registration's `information` (clause 5.2.9) is a nested array of entity and
//! attribute selectors. Both the discovery statement (clause 5.9) and the
//! forwarding coverage decision (clause 5.10.2) ask questions of it that reduce
//! to the same shape — some `information` item constrains nothing, or one of its
//! `entities` entries matches — so that shape is written once here and each
//! caller supplies only the entry-level predicate it decides with.

use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_array_elements, jsonb_exists},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, Query, SelectStatement, SimpleExpr};

/// The `AS info` element of the unnested `information` array.
pub(crate) fn info() -> SimpleExpr {
    SeaExpr::col((Alias::new("info"), Alias::new("value"))).into()
}

/// The `AS ent` element of an information item's unnested `entities` array.
pub(crate) fn ent() -> SimpleExpr {
    SeaExpr::col((Alias::new("ent"), Alias::new("value"))).into()
}

/// `NOT jsonb_exists(target, 'key')` - whether `target` lacks `key`.
pub(crate) fn lacks_key(target: SimpleExpr, key: &str) -> SimpleExpr {
    Condition::all().add(jsonb_exists(target, text_literal(key))).not().into()
}

/// The `entities` entries of the current information item, as a scan a caller
/// narrows with its own `WHERE`.
pub(crate) fn entities_of_item() -> SelectStatement {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_array_elements(info().json_get(text_literal("entities"))), Alias::new("ent"));
    scan
}

/// Whether some `information` item constrains no entities, an empty entities
/// list, or an `entities` entry the `entities_match` scan admits.
///
/// The first two disjuncts are the "constrains everything" escape hatch clause
/// 5.10.2 requires: an item with no `entities`, or an empty one, covers every
/// entity, so a caller only reaches its own entry-level predicate for items that
/// do constrain.
pub(crate) fn information_covers(entities_match: SelectStatement) -> SimpleExpr {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_array_elements(SeaExpr::col(Csr::Information).into()), Alias::new("info"))
        .cond_where(
            Condition::any()
                .add(lacks_key(info(), "entities"))
                .add(SeaExpr::expr(info().json_get(text_literal("entities"))).eq(cast(text_literal("[]"), PostgresType::Jsonb)))
                .add(SeaExpr::exists(entities_match)),
        );
    SeaExpr::exists(scan)
}
