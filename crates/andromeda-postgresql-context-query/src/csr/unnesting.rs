//! The `information` document unnesting shared by CSR discovery and coverage.
//!
//! A registration's `information` (clause 5.2.9) is a nested array of entity and
//! attribute selectors. Both the discovery statement (clause 5.10.2) and the
//! forwarding coverage decision (clause 5.10.2) ask questions of it that reduce
//! to the same shape — some `information` item satisfies a predicate, and on
//! the entity axis an item constrains nothing or one of its `entities` entries
//! matches — so that shape is written once here and each caller supplies only
//! the entry-level predicate it decides with.

use andromeda_postgresql_schema::csr::Csr;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{jsonb_array_elements, jsonb_exists},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The `AS info` element of the unnested `information` array.
pub(crate) fn info() -> SimpleExpr {
    SeaExpr::col((Alias::new("info"), Alias::new("value")))
}

/// The `AS ent` element of an information item's unnested `entities` array.
pub(crate) fn ent() -> SimpleExpr {
    SeaExpr::col((Alias::new("ent"), Alias::new("value")))
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

/// Whether the current `information` item constrains no entities, an empty
/// entities list, or an `entities` entry the `entities_match` scan admits.
///
/// The first two disjuncts are the "constrains everything" escape hatch clause
/// 5.10.2 requires: an item with no `entities`, or an empty one, covers every
/// entity, so a caller only reaches its own entry-level predicate for items that
/// do constrain (clause 5.12: "If there is no EntityInfo specified in the
/// RegistrationInfo, the entity specification is considered matching").
pub(crate) fn entity_axis_covers(entities_match: SelectStatement) -> Condition {
    Condition::any()
        .add(lacks_key(info(), "entities"))
        .add(SeaExpr::expr(info().json_get(text_literal("entities"))).eq(cast(text_literal("[]"), PostgresType::Jsonb)))
        .add(SeaExpr::exists(entities_match))
}

/// Whether some `information` item satisfies `item_matches`, a condition
/// written against [`info`].
///
/// Clause 5.12 matches a specification against one `RegistrationInfo` at a
/// time: the entity axis and the attribute axis must hold for the *same* item,
/// which is why a caller conjoins its axes inside one unnesting rather than
/// unnesting once per axis.
pub(crate) fn information_item_matches(item_matches: Condition) -> SimpleExpr {
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_array_elements(SeaExpr::col(Csr::Information)), Alias::new("info"))
        .cond_where(item_matches);
    SeaExpr::exists(scan)
}

/// Whether some `information` item covers the entity axis: it constrains no
/// entities, or one of its `entities` entries is admitted by `entities_match`.
pub(crate) fn information_covers(entities_match: SelectStatement) -> SimpleExpr {
    information_item_matches(entity_axis_covers(entities_match))
}
