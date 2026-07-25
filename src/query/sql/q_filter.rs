//! Compiler from NGSI-LD `q` filter expressions (ETSI GS CIM 009 v1.9.1
//! clause 4.9) to SQL predicates, shared by the entity and temporal builders.
//!
//! # Why this exists
//!
//! Value predicates used to compile to
//! `jsonb_path_query_first(payload, '$[0]."<iri>"[0]."hasValue"[0]."@value"')`
//! with a text cast. That is a functional expression over a JSONB column: no
//! index can serve it, so every filtered query degraded to a sequential scan
//! over `entities`.
//!
//! Predicates now compile to an `EXISTS` probe against the `attribute_current`
//! projection, whose typed partial indexes are keyed
//! `(attribute_name_id, value_*)`. The planner scans the typed index, fetches
//! `entity_id` from the heap, and semi-joins back to the driving table — the
//! access path migration 0025 was shaped for.
//!
//! # Eligibility
//!
//! Not every predicate can use the projection. `attribute_current` stores one
//! typed value per attribute instance; it does not store sub-attributes or the
//! interior of compound values. So:
//!
//! | Predicate shape | Compiles to |
//! |---|---|
//! | comparison or pattern with no sub-attribute and no value path | `EXISTS` on `attribute_current` |
//! | bare existence | `EXISTS` on `temporal_entity_attributes`, tombstone-checked |
//! | anything carrying a sub-attribute or value path | JSONB jsonpath fallback |
//! | attribute IRI absent from `attribute_names` | constant `FALSE` |
//!
//! The fallback keeps the previous semantics for the shapes the projection
//! cannot answer. It is still unindexed; it is now the exception rather than
//! the rule.
//!
//! # Multi-instance semantics
//!
//! `EXISTS` matches when *any* row for the attribute satisfies the predicate.
//! That is what clause 4.9 requires (p. 86: "the target value shall be any
//! Value of such instances"). The previous `jsonb_path_query_first` shape only
//! ever examined the first instance, which was a spec bug.

use crate::{
    core::error::AndromedaError,
    domain::temporal::AttributeNameId,
    jsonld::vocab,
    query::q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
};
use chrono::{DateTime, Utc};
use sea_query::{Condition, Expr as SeaExpr, Value as SeaValue};
use std::collections::HashMap;

/// Attribute-name dictionary ids for the IRIs a compiled `q` filter mentions.
///
/// An IRI that is absent has no `attribute_names` row, and therefore no
/// `attribute_current` or `temporal_entity_attributes` row either, so its
/// predicates compile to constant `FALSE`.
///
/// There is deliberately no way to represent a *cached* miss. Resolution runs
/// per query execution against a cache that only ever stores hits, so an
/// attribute created after a filter was first compiled becomes visible on the
/// next execution instead of being pinned absent.
#[derive(Debug, Default, Clone)]
pub struct ResolvedAttributeNames {
    ids: HashMap<String, AttributeNameId>,
}

impl ResolvedAttributeNames {
    /// Build from resolved `(expanded IRI, dictionary id)` pairs.
    pub fn from_pairs(pairs: impl IntoIterator<Item = (String, AttributeNameId)>) -> ResolvedAttributeNames {
        ResolvedAttributeNames {
            ids: pairs.into_iter().collect(),
        }
    }

    /// Dictionary id for an expanded attribute IRI, if the broker knows it.
    pub fn get(&self, attr_iri: &str) -> Option<AttributeNameId> {
        self.ids.get(attr_iri).copied()
    }

    /// Whether any attribute IRI resolved. Used to decide whether a query has
    /// to wait on the projection's read-your-writes barrier at all.
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Which enclosing query a compiled `q` predicate is being attached to.
///
/// Determines the column an `attribute_current` probe correlates against and
/// the payload column the jsonpath fallback reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QFilterTarget {
    /// Entity queries, which select directly from `entities`.
    Entities,
    /// Temporal stage 1, which drives from the TEA catalogue and left-joins
    /// `entities` under the alias `e`.
    TemporalStage1,
}

impl QFilterTarget {
    /// SQL reference to the entity id an `EXISTS` probe correlates against.
    const fn entity_id_column(self) -> &'static str {
        match self {
            Self::Entities => "\"entities\".\"id\"",
            Self::TemporalStage1 => "\"tea\".\"entity_id\"",
        }
    }

    /// SQL reference to the JSONB payload column used by the fallback.
    const fn payload_column(self) -> &'static str {
        match self {
            Self::Entities => "\"payload\"",
            Self::TemporalStage1 => "\"e\".\"payload\"",
        }
    }
}

/// Compile a parsed `q` expression into a sea-query [`Condition`].
pub fn compile(expr: &Expr, target: QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, AndromedaError> {
    match expr {
        Expr::And(terms) => terms
            .iter()
            .try_fold(Condition::all(), |cond, term| Ok(cond.add(compile(term, target, names)?))),
        Expr::Or(terms) => terms
            .iter()
            .try_fold(Condition::any(), |cond, term| Ok(cond.add(compile(term, target, names)?))),
        Expr::Pred(pred) => compile_predicate(pred, target, names),
    }
}

/// Attribute IRIs a `q` expression references at the top level.
///
/// Only these need dictionary resolution: sub-attribute and value-path
/// segments are navigated inside the JSONB payload, never through
/// `attribute_names`.
pub fn referenced_attribute_iris(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => {
            for term in terms {
                referenced_attribute_iris(term, out);
            }
        }
        Expr::Pred(pred) => {
            let path = match pred {
                Predicate::Exists(path) | Predicate::Compare { path, .. } | Predicate::MatchPattern { path, .. } | Predicate::NotMatchPattern { path, .. } => {
                    path
                }
            };
            out.push(path.attr.clone()); // clone: caller owns the collected list
        }
    }
}

fn compile_predicate(pred: &Predicate, target: QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, AndromedaError> {
    match pred {
        Predicate::Exists(path) => Ok(compile_exists(path, target, names)),
        Predicate::Compare { path, op, rhs } => {
            if is_projection_eligible(path) && !exceeds_projection_precision(rhs) {
                match names.get(&path.attr) {
                    Some(id) => compile_compare_on_projection(id, *op, rhs, target),
                    None => Ok(unsatisfiable()),
                }
            } else {
                fallback_compare(path, *op, rhs, target)
            }
        }
        Predicate::MatchPattern { path, pattern } => Ok(compile_pattern(path, pattern, PatternPolarity::Match, target, names)),
        Predicate::NotMatchPattern { path, pattern } => Ok(compile_pattern(path, pattern, PatternPolarity::NotMatch, target, names)),
    }
}

/// Whether a path addresses an attribute's own value, which is the only thing
/// `attribute_current` stores. Sub-attributes (`temperature.observedAt`) and
/// compound value paths (`address[city]`) live inside the JSONB payload.
const fn is_projection_eligible(path: &AttrPath) -> bool {
    path.sub_attrs.is_empty() && path.value_path.is_empty()
}

/// Largest integer magnitude `f64` represents exactly.
const F64_EXACT_INTEGER_LIMIT: i128 = 1 << 53;

/// Whether a comparison target is outside what the projection can represent.
///
/// `attribute_current.value_number` is `double precision`, but JSONB stores
/// numbers as exact `numeric`, so the payload holds the precise value while the
/// projection holds a rounded one. Beyond 2^53 those disagree, and a probe
/// against the projection would answer from the rounded copy: `GET` on the
/// entity would show the exact value while the filter denied it existed.
///
/// Such predicates go to the JSONB fallback instead, which reads the payload and
/// compares as `numeric`. That is slower and unindexed, but it is the only
/// source that still holds the value the client wrote.
fn exceeds_projection_precision(rhs: &CompareValue) -> bool {
    fn scalar_exceeds(scalar: &Scalar) -> bool {
        let Scalar::Number(number) = scalar else {
            return false;
        };
        // Only integers can exceed f64's exact range while still being written
        // precisely in JSON; a value the client wrote as a float is already an
        // f64-shaped quantity.
        number
            .as_i64()
            .map(|value| i128::from(value).abs() > F64_EXACT_INTEGER_LIMIT)
            .or_else(|| number.as_u64().map(|value| i128::from(value) > F64_EXACT_INTEGER_LIMIT))
            .unwrap_or(false)
    }

    match rhs {
        CompareValue::Scalar(scalar) => scalar_exceeds(scalar),
        CompareValue::Range { min, max } => scalar_exceeds(min) || scalar_exceeds(max),
        CompareValue::ValueList(values) => values.iter().any(scalar_exceeds),
    }
}

/// A predicate that no row can satisfy.
fn unsatisfiable() -> Condition {
    Condition::all().add(SeaExpr::cust("FALSE"))
}

// ---------------------------------------------------------------------------
// Existence
// ---------------------------------------------------------------------------

/// Compile a bare existence predicate.
///
/// Existence resolves through the temporal catalogue rather than the payload.
/// The obvious alternative, `payload->0 ? '<iri>'`, cannot use the GIN index on
/// `payload`: GIN's `jsonb_ops` only serves `?` when the left operand is the
/// bare indexed column, and `payload->0` is an expression. It is a sequential
/// scan.
///
/// Both probes here hit indexes that already exist and are already maintained:
/// the outer one uses the leading `(entity_id, attribute_name_id)` prefix of the
/// TEA uniqueness constraint, the inner one `attribute_current`'s primary key.
///
/// The inner probe is required because TEA rows outlive the attributes they
/// describe — deleting an attribute writes a tombstone rather than dropping the
/// catalogue row — so catalogue presence alone would report deleted attributes
/// as present.
fn compile_exists(path: &AttrPath, target: QFilterTarget, names: &ResolvedAttributeNames) -> Condition {
    if !is_projection_eligible(path) {
        // A sub-path existence check asks whether a nested element is present,
        // which only the payload can answer. The previous compiler silently
        // dropped the sub-path here and tested the top-level attribute instead.
        let jsonpath = value_extraction_jsonpath(path, vocab::expanded::properties::VALUE.as_str());
        let sql = format!("jsonb_path_exists({}, '{}')", target.payload_column(), escape_jsonpath(&jsonpath));
        return Condition::all().add(SeaExpr::cust(sql));
    }

    let Some(id) = names.get(&path.attr) else {
        return unsatisfiable();
    };

    let mut binds = BindBuilder::default();
    let attr = binds.bind(id.get());
    let sql = format!(
        "EXISTS (SELECT 1 FROM \"temporal_entity_attributes\" AS \"tea_q\" \
         WHERE \"tea_q\".\"entity_id\" = {corr} AND \"tea_q\".\"attribute_name_id\" = {attr} \
         AND EXISTS (SELECT 1 FROM \"attribute_current\" AS \"ac\" \
         WHERE \"ac\".\"tea_id\" = \"tea_q\".\"id\" AND {guard}))",
        corr = target.entity_id_column(),
        attr = attr,
        guard = TOMBSTONE_GUARD,
    );
    Condition::all().add(SeaExpr::cust_with_values(sql, binds.values))
}

// ---------------------------------------------------------------------------
// Comparisons against the projection
// ---------------------------------------------------------------------------

/// Excludes deletion tombstones, which are stored as
/// `value_string = 'urn:ngsi-ld:null'`.
///
/// Applied to every projection probe, not only string ones: the
/// `chk_attribute_current_single_value` constraint guarantees exactly one typed
/// column is populated, so on a non-string row `value_string` is NULL and
/// `NULL IS DISTINCT FROM '…'` is true. One uniform guard is easier to keep
/// correct than a per-arm one.
const TOMBSTONE_GUARD: &str = "\"ac\".\"value_string\" IS DISTINCT FROM 'urn:ngsi-ld:null'";

fn compile_compare_on_projection(id: AttributeNameId, op: CmpOp, rhs: &CompareValue, target: QFilterTarget) -> Result<Condition, AndromedaError> {
    let mut binds = BindBuilder::default();
    let attr = binds.bind(id.get());

    let value_sql = match rhs {
        CompareValue::Scalar(scalar) => scalar_condition(scalar, op, &mut binds),
        CompareValue::Range { min, max } => match op {
            CmpOp::Equal => {
                let lower = scalar_condition(min, CmpOp::GreaterEq, &mut binds);
                let upper = scalar_condition(max, CmpOp::LessEq, &mut binds);
                format!("({lower} AND {upper})")
            }
            CmpOp::Unequal => {
                let below = scalar_condition(min, CmpOp::Less, &mut binds);
                let above = scalar_condition(max, CmpOp::Greater, &mut binds);
                format!("({below} OR {above})")
            }
            CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => {
                return Err(AndromedaError::query_validation("Range values only supported with == and != operators"));
            }
        },
        CompareValue::ValueList(values) => {
            let arms: Vec<String> = values.iter().map(|value| scalar_condition(value, CmpOp::Equal, &mut binds)).collect();
            let joined = arms.join(" OR ");
            match op {
                CmpOp::Equal => format!("({joined})"),
                CmpOp::Unequal => format!("NOT ({joined})"),
                CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => {
                    return Err(AndromedaError::query_validation("Value lists only supported with == and != operators"));
                }
            }
        }
    };

    Ok(Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.values)))
}

/// Wrap a value predicate in the correlated `attribute_current` probe.
fn exists_on_projection(attr_placeholder: &str, value_sql: &str, target: QFilterTarget) -> String {
    format!(
        "EXISTS (SELECT 1 FROM \"attribute_current\" AS \"ac\" \
         WHERE \"ac\".\"attribute_name_id\" = {attr} AND ({value}) AND {guard} \
         AND \"ac\".\"entity_id\" = {corr})",
        attr = attr_placeholder,
        value = value_sql,
        guard = TOMBSTONE_GUARD,
        corr = target.entity_id_column(),
    )
}

/// Compare one scalar against the typed column that holds values of its kind.
///
/// The mapping mirrors what `TemporalRecorder::extract_typed_value` writes:
///
/// * numbers land in `value_number` (`double precision`),
/// * booleans in `value_bool`,
/// * date-times in `value_datetime`,
/// * strings in `value_string` for Properties and single-valued
///   VocabProperties, and in `value_uri` for Relationship objects — clause 4.9
///   makes a Relationship's object a legal comparison target, so a string
///   comparison has to consider both,
/// * plain dates and times in `value_string`, because the recorder only
///   promotes full `DateTime`-typed values to `value_datetime`.
fn scalar_condition(scalar: &Scalar, op: CmpOp, binds: &mut BindBuilder) -> String {
    let operator = cmp_op_sql(op);
    match scalar {
        Scalar::Number(number) => match number.as_f64() {
            Some(value) => {
                let placeholder = binds.bind(value);
                format!("\"ac\".\"value_number\" {operator} {placeholder}")
            }
            // A `serde_json::Number` that will not fit an f64 cannot equal any
            // stored `double precision` value.
            None => "FALSE".to_string(),
        },
        Scalar::Bool(value) => {
            let placeholder = binds.bind(*value);
            format!("\"ac\".\"value_bool\" {operator} {placeholder}")
        }
        Scalar::Str(value) => {
            let string_placeholder = binds.bind(value.as_str());
            let uri_placeholder = binds.bind(value.as_str());
            format!("(\"ac\".\"value_string\" {operator} {string_placeholder} OR \"ac\".\"value_uri\" {operator} {uri_placeholder})")
        }
        Scalar::DateTime(raw) => match DateTime::parse_from_rfc3339(raw) {
            Ok(parsed) => {
                let placeholder = binds.bind(parsed.with_timezone(&Utc));
                format!("\"ac\".\"value_datetime\" {operator} {placeholder}")
            }
            // Not a real timestamp; fall back to the textual comparison the
            // previous compiler would have made.
            Err(_) => {
                let placeholder = binds.bind(raw.as_str());
                format!("\"ac\".\"value_string\" {operator} {placeholder}")
            }
        },
        Scalar::Date(raw) | Scalar::Time(raw) => {
            let placeholder = binds.bind(raw.as_str());
            format!("\"ac\".\"value_string\" {operator} {placeholder}")
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Whether a pattern predicate asserts a match or the absence of one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PatternPolarity {
    /// `~=`
    Match,
    /// `!~=`
    NotMatch,
}

impl PatternPolarity {
    const fn operator(self) -> &'static str {
        match self {
            Self::Match => "~",
            Self::NotMatch => "!~",
        }
    }
}

/// Compile `~=` / `!~=` (clause 4.9 `patternOp` / `notPatternOp`).
///
/// Both polarities compile *inside* the `EXISTS`, so both require the attribute
/// to be present. Clause 4.9 is explicit that the entity "shall contain the
/// target element" for `notPatternOp`; the previous compiler wrapped the whole
/// existence test in `NOT (...)`, which matched entities that simply lacked the
/// attribute.
///
/// POSIX `~` is not accelerated by the trigram index (`docs/postgresql.md`),
/// but the probe is still narrowed by the `attribute_name_id` prefix of
/// `idx_ac_attr_str` before any regex is evaluated.
fn compile_pattern(path: &AttrPath, pattern: &str, polarity: PatternPolarity, target: QFilterTarget, names: &ResolvedAttributeNames) -> Condition {
    if !is_projection_eligible(path) {
        return fallback_pattern(path, pattern, polarity, target);
    }
    let Some(id) = names.get(&path.attr) else {
        return unsatisfiable();
    };

    let mut binds = BindBuilder::default();
    let attr = binds.bind(id.get());
    let pattern_placeholder = binds.bind(pattern);
    let value_sql = format!("\"ac\".\"value_string\" {} {}", polarity.operator(), pattern_placeholder);
    Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.values))
}

// ---------------------------------------------------------------------------
// JSONB fallback
// ---------------------------------------------------------------------------

fn fallback_compare(path: &AttrPath, op: CmpOp, rhs: &CompareValue, target: QFilterTarget) -> Result<Condition, AndromedaError> {
    let payload = target.payload_column();
    let value_path = value_extraction_jsonpath(path, vocab::expanded::properties::VALUE.as_str());
    let object_path = value_extraction_jsonpath(path, vocab::expanded::properties::OBJECT.as_str());
    let extract_value = format!("jsonb_path_query_first({}, '{}')", payload, escape_jsonpath(&value_path));
    let extract_object = format!("jsonb_path_query_first({}, '{}')", payload, escape_jsonpath(&object_path));

    match rhs {
        CompareValue::Scalar(scalar) => {
            let (cast, value) = scalar_to_text_bind(scalar);
            let sql = format!(
                "(COALESCE(({extract_value} #>> '{{}}'), ({extract_object} #>> '{{}}')){cast} {op} $1{cast})",
                op = cmp_op_sql(op)
            );
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [value])))
        }
        CompareValue::Range { min, max } => {
            let (cast, min_value) = scalar_to_text_bind(min);
            let (_, max_value) = scalar_to_text_bind(max);
            match op {
                CmpOp::Equal => {
                    let sql = format!("(({extract_value} #>> '{{}}')){cast} >= $1{cast} AND (({extract_value} #>> '{{}}')){cast} <= $2{cast}");
                    Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [min_value, max_value])))
                }
                CmpOp::Unequal => {
                    let sql = format!("(({extract_value} #>> '{{}}')){cast} < $1{cast} OR (({extract_value} #>> '{{}}')){cast} > $2{cast}");
                    Ok(Condition::all().add(SeaExpr::cust_with_values(sql, [min_value, max_value])))
                }
                CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => {
                    Err(AndromedaError::query_validation("Range values only supported with == and != operators"))
                }
            }
        }
        CompareValue::ValueList(values) => {
            let mut arms = Vec::with_capacity(values.len());
            let mut binds = Vec::with_capacity(values.len());
            for (index, value) in values.iter().enumerate() {
                let (cast, bound) = scalar_to_text_bind(value);
                arms.push(format!("(({extract_value} #>> '{{}}')){cast} = ${}{cast}", index + 1));
                binds.push(bound);
            }
            let joined = arms.join(" OR ");
            let sql = match op {
                CmpOp::Equal => format!("({joined})"),
                CmpOp::Unequal => format!("NOT ({joined})"),
                CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => {
                    return Err(AndromedaError::query_validation("Value lists only supported with == and != operators"));
                }
            };
            Ok(Condition::all().add(SeaExpr::cust_with_values(sql, binds)))
        }
    }
}

fn fallback_pattern(path: &AttrPath, pattern: &str, polarity: PatternPolarity, target: QFilterTarget) -> Condition {
    let payload = target.payload_column();
    let jsonpath = escape_jsonpath(&value_extraction_jsonpath(path, vocab::expanded::properties::VALUE.as_str()));
    // Element presence is asserted in both polarities (clause 4.9); only the
    // regex operator flips.
    let sql = format!(
        "(jsonb_path_exists({payload}, '{jsonpath}') AND (jsonb_path_query_first({payload}, '{jsonpath}')#>>'{{}}')::text {} $1)",
        polarity.operator()
    );
    Condition::all().add(SeaExpr::cust_with_values(sql, [SeaValue::from(pattern)]))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collects sea-query bind values and hands back the `$N` marker for each.
///
/// The markers are local to one `cust_with_values` fragment; sea-query
/// renumbers them when the enclosing statement is rendered.
#[derive(Debug, Default)]
struct BindBuilder {
    values: Vec<SeaValue>,
}

impl BindBuilder {
    fn bind(&mut self, value: impl Into<SeaValue>) -> String {
        self.values.push(value.into());
        format!("${}", self.values.len())
    }
}

const fn cmp_op_sql(op: CmpOp) -> &'static str {
    match op {
        CmpOp::Equal => "=",
        CmpOp::Unequal => "!=",
        CmpOp::Greater => ">",
        CmpOp::GreaterEq => ">=",
        CmpOp::Less => "<",
        CmpOp::LessEq => "<=",
    }
}

/// Cast suffix and text rendering used by the JSONB fallback, which compares
/// everything as text extracted with `#>>`.
fn scalar_to_text_bind(scalar: &Scalar) -> (&'static str, String) {
    match scalar {
        Scalar::Number(n) => ("::numeric", n.to_string()),
        Scalar::Bool(b) => ("::boolean", b.to_string()),
        Scalar::Str(s) => ("", s.clone()), // clone: sea-query bind takes an owned String
        Scalar::DateTime(s) | Scalar::Date(s) | Scalar::Time(s) => ("", s.clone()), // clone: as above
    }
}

/// Build the jsonpath that reaches an attribute's value inside the expanded
/// payload: `$[0]."<attr>"[0]."<sub>"…[0]."<terminal>"[0]."@value"."<key>"…`.
fn value_extraction_jsonpath(path: &AttrPath, terminal_iri: &str) -> String {
    let mut jsonpath = format!("$[0].\"{}\"", escape_jsonpath_key(&path.attr));
    for sub in &path.sub_attrs {
        jsonpath.push_str(&format!("[0].\"{}\"", escape_jsonpath_key(sub)));
    }
    jsonpath.push_str(&format!("[0].\"{}\"[0]", escape_jsonpath_key(terminal_iri)));
    let value_key = if terminal_iri == vocab::expanded::properties::OBJECT.as_str() {
        "@id"
    } else {
        "@value"
    };
    jsonpath.push_str(&format!(".\"{value_key}\""));
    for key in &path.value_path {
        jsonpath.push_str(&format!(".\"{}\"", escape_jsonpath_key(key)));
    }
    jsonpath
}

fn escape_jsonpath_key(key: &str) -> String {
    key.replace('\\', "\\\\").replace('"', "\\\"")
}

/// Escape a jsonpath for embedding in a single-quoted SQL literal.
///
/// Applied unconditionally. The temporal builder previously omitted it, so an
/// attribute IRI containing an apostrophe terminated the literal early.
fn escape_jsonpath(jsonpath: &str) -> String {
    jsonpath.replace('\'', "''")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;
    use crate::query::{
        q::{ast::AttrPath, parser},
        sql::BindValue,
    };
    use sea_query::{PostgresQueryBuilder, Query};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([
            (SPEED.to_string(), AttributeNameId::from(7)),
            ("https://example.com/brandName".to_string(), AttributeNameId::from(8)),
            ("https://example.com/active".to_string(), AttributeNameId::from(9)),
            ("https://example.com/observed".to_string(), AttributeNameId::from(10)),
            ("https://example.com/d".to_string(), AttributeNameId::from(11)),
            ("https://example.com/temperature".to_string(), AttributeNameId::from(12)),
            ("https://example.com/color".to_string(), AttributeNameId::from(13)),
            ("https://example.com/name".to_string(), AttributeNameId::from(14)),
            ("https://example.com/address".to_string(), AttributeNameId::from(15)),
            ("https://example.com/a".to_string(), AttributeNameId::from(16)),
            ("https://example.com/b".to_string(), AttributeNameId::from(17)),
            ("https://example.com/c".to_string(), AttributeNameId::from(18)),
        ])
    }

    /// Render a compiled condition the way a real query would, so the assertions
    /// see the final placeholder numbering.
    fn render(q: &str) -> (String, Vec<BindValue<'static>>) {
        render_with(q, &names(), QFilterTarget::Entities)
    }

    fn render_with(q: &str, names: &ResolvedAttributeNames, target: QFilterTarget) -> (String, Vec<BindValue<'static>>) {
        let expr = parser::parse(q).expect("parse q");
        let condition = compile(&expr, target, names).expect("compile q");
        let (sql, values) = Query::select()
            .column(crate::query::sql::schema::Entities::Id)
            .from(crate::query::sql::schema::Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        (sql, crate::query::sql::sea_values_to_bind_values(values).expect("bind values"))
    }

    fn quoted(iri: &str) -> String {
        format!("\"{iri}\"")
    }

    // -- projection-backed comparisons (clause 4.9) --------------------------

    #[test]
    fn number_comparison_probes_typed_number_column() {
        let (sql, binds) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"attribute_current\""), "{sql}");
        assert!(sql.contains("\"ac\".\"value_number\" >"), "{sql}");
        assert!(matches!(binds[0], BindValue::I16(7)), "attribute id binds as smallint: {binds:?}");
        assert!(
            matches!(binds[1], BindValue::F64(value) if (value - 50.0).abs() < f64::EPSILON),
            "numeric literal binds as float8: {binds:?}"
        );
    }

    #[test]
    fn string_comparison_covers_property_values_and_relationship_objects() {
        let (sql, _) = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert!(sql.contains("\"ac\".\"value_string\" ="), "{sql}");
        assert!(sql.contains("\"ac\".\"value_uri\" ="), "relationship objects are legal targets: {sql}");
        assert!(sql.contains("IS DISTINCT FROM 'urn:ngsi-ld:null'"), "tombstone guard missing: {sql}");
    }

    #[test]
    fn boolean_comparison_binds_native_boolean() {
        let (sql, binds) = render("\"https://example.com/active\"==true");
        assert!(sql.contains("\"ac\".\"value_bool\" ="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, BindValue::Bool(true))), "{binds:?}");
    }

    #[test]
    fn datetime_comparison_binds_native_timestamp() {
        let (sql, binds) = render("\"https://example.com/observed\">=2017-12-24T12:00:00Z");
        assert!(sql.contains("\"ac\".\"value_datetime\" >="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, BindValue::Timestamp(_))), "{binds:?}");
    }

    #[test]
    fn date_scalar_compares_as_text_because_the_recorder_stores_it_that_way() {
        let (sql, _) = render("\"https://example.com/d\"==2024-01-01");
        assert!(sql.contains("\"ac\".\"value_string\" ="), "{sql}");
        assert!(!sql.contains("value_datetime"), "only full date-times reach value_datetime: {sql}");
    }

    #[test]
    fn inclusive_range_becomes_a_bounded_pair_inside_one_probe() {
        let (sql, binds) = render("\"https://example.com/temperature\"==10..20");
        assert_eq!(sql.matches("EXISTS").count(), 1, "one probe, not two: {sql}");
        assert!(sql.contains("\"ac\".\"value_number\" >="), "{sql}");
        assert!(sql.contains("\"ac\".\"value_number\" <="), "{sql}");
        assert!(matches!(binds[0], BindValue::I16(12)));
        assert!(matches!(binds[1], BindValue::F64(value) if (value - 10.0).abs() < f64::EPSILON));
        assert!(matches!(binds[2], BindValue::F64(value) if (value - 20.0).abs() < f64::EPSILON));
    }

    #[test]
    fn excluded_range_requires_the_attribute_to_exist() {
        let (sql, _) = render("\"https://example.com/temperature\"!=10..20");
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"ac\".\"value_number\" <"), "{sql}");
        assert!(sql.contains("\"ac\".\"value_number\" >"), "{sql}");
    }

    #[test]
    fn value_list_becomes_a_disjunction_inside_one_probe() {
        let (sql, _) = render("\"https://example.com/color\"==\"red\",\"blue\"");
        assert_eq!(sql.matches("EXISTS").count(), 1, "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }

    #[test]
    fn negated_value_list_negates_the_disjunction_not_the_existence() {
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert!(sql.contains("NOT ("), "{sql}");
        assert!(sql.contains("EXISTS"), "the attribute must still exist: {sql}");
        assert!(!sql.contains("NOT (EXISTS"), "{sql}");
    }

    // -- patterns ------------------------------------------------------------

    #[test]
    fn pattern_match_uses_posix_regex_on_the_string_column() {
        let (sql, _) = render("\"https://example.com/name\"~=\"^A.*\"");
        assert!(sql.contains("\"ac\".\"value_string\" ~"), "{sql}");
        assert!(sql.contains("IS DISTINCT FROM 'urn:ngsi-ld:null'"), "{sql}");
    }

    #[test]
    fn negated_pattern_sits_inside_the_probe_so_missing_attributes_do_not_match() {
        let (sql, _) = render("\"https://example.com/name\"!~=\"^A.*\"");
        assert!(sql.contains("\"ac\".\"value_string\" !~"), "{sql}");
        assert!(
            !sql.contains("NOT (EXISTS") && !sql.contains("NOT EXISTS"),
            "clause 4.9 requires the target element to be present: {sql}"
        );
    }

    // -- unknown attributes --------------------------------------------------

    #[test]
    fn unknown_attribute_iri_compiles_to_a_constant_false() {
        let (sql, _) = render_with(&format!("\"{}\">1", "https://example.com/never-written"), &names(), QFilterTarget::Entities);
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("attribute_current"), "no probe is emitted when no row can exist: {sql}");
    }

    // -- fallback ------------------------------------------------------------

    #[test]
    fn integers_beyond_f64_precision_fall_back_to_the_payload() {
        // The projection stores `double precision`; JSONB stores exact numeric.
        // Probing the projection here would answer from the rounded copy, so the
        // entity would be visible via GET but denied by the filter.
        let (sql, _) = render(&format!("{}==12345678901234567", quoted(SPEED)));
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    #[test]
    fn integers_within_f64_precision_still_use_the_projection() {
        let (sql, _) = render(&format!("{}==9007199254740991", quoted(SPEED)));
        assert!(sql.contains("attribute_current"), "2^53-1 is exact in f64: {sql}");
    }

    #[test]
    fn a_range_bound_beyond_f64_precision_falls_back() {
        let (sql, _) = render(&format!("{}==1..12345678901234567", quoted(SPEED)));
        assert!(sql.contains("jsonb_path_query_first"), "one lossy bound taints the range: {sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    #[test]
    fn floats_are_never_treated_as_lossy() {
        // A value written as a float is already an f64-shaped quantity; only
        // large integers can lose precision moving into the projection.
        let (sql, _) = render(&format!("{}>1.5", quoted(SPEED)));
        assert!(sql.contains("\"ac\".\"value_number\" >"), "{sql}");
    }

    #[test]
    fn sub_attribute_predicate_falls_back_to_jsonpath() {
        let (sql, _) = render("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(!sql.contains("attribute_current"), "sub-attributes are not projected: {sql}");
    }

    #[test]
    fn compound_value_path_falls_back_to_jsonpath_with_terminal_key() {
        let (sql, _) = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(sql.contains(".\\\"city\\\"") || sql.contains(".\"city\""), "{sql}");
        assert!(!sql.contains("attribute_current"), "{sql}");
    }

    #[test]
    fn fallback_escapes_apostrophes_in_attribute_iris() {
        let path = AttrPath {
            attr: "https://example.com/o'brien".to_string(),
            sub_attrs: vec!["https://example.com/sub".to_string()],
            value_path: Vec::new(),
        };
        let expr = Expr::Pred(Predicate::Compare {
            path,
            op: CmpOp::Equal,
            rhs: CompareValue::Scalar(Scalar::Str("x".to_string())),
        });
        let condition = compile(&expr, QFilterTarget::Entities, &names()).expect("compile");
        let (sql, _) = Query::select()
            .column(crate::query::sql::schema::Entities::Id)
            .from(crate::query::sql::schema::Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        assert!(sql.contains("o''brien"), "apostrophe must be doubled inside the SQL literal: {sql}");
    }

    // -- existence -----------------------------------------------------------

    #[test]
    fn bare_existence_probes_the_temporal_catalogue_not_the_payload() {
        let (sql, binds) = render(&quoted(SPEED));
        assert!(sql.contains("\"temporal_entity_attributes\" AS \"tea_q\""), "{sql}");
        assert!(sql.contains("\"attribute_current\""), "presence must exclude tombstones: {sql}");
        assert!(!sql.contains("\"payload\"->0 ?"), "the payload probe cannot use the GIN index: {sql}");
        assert!(matches!(binds[0], BindValue::I16(7)), "{binds:?}");
    }

    #[test]
    fn existence_with_a_sub_path_tests_the_whole_path() {
        let (sql, _) = render("\"https://example.com/address\".\"https://example.com/city\"");
        assert!(sql.contains("jsonb_path_exists"), "{sql}");
        assert!(sql.contains("city"), "the sub-path must not be silently dropped: {sql}");
    }

    #[test]
    fn unknown_attribute_existence_compiles_to_a_constant_false() {
        let (sql, _) = render("\"https://example.com/never-written\"");
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("temporal_entity_attributes"), "{sql}");
    }

    // -- boolean structure ---------------------------------------------------

    #[test]
    fn and_or_nesting_preserves_grouping() {
        let (sql, _) = render(&format!(
            "({}>1;\"https://example.com/b\"<2)|\"https://example.com/c\"==\"x\"",
            quoted("https://example.com/a")
        ));
        assert_eq!(sql.matches("EXISTS").count(), 3, "one probe per predicate: {sql}");
        assert!(sql.contains(" AND "), "{sql}");
        assert!(sql.contains(" OR "), "{sql}");
    }

    // -- correlation ---------------------------------------------------------

    #[test]
    fn temporal_stage_one_correlates_on_the_catalogue_entity_id() {
        let expr = parser::parse(&format!("{}>50", quoted(SPEED))).expect("parse");
        let condition = compile(&expr, QFilterTarget::TemporalStage1, &names()).expect("compile");
        let (sql, _) = Query::select()
            .column(crate::query::sql::schema::Entities::Id)
            .from(crate::query::sql::schema::Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        assert!(sql.contains("\"ac\".\"entity_id\" = \"tea\".\"entity_id\""), "{sql}");
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_primary_key() {
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"ac\".\"entity_id\" = \"entities\".\"id\""), "{sql}");
    }

    // -- attribute collection ------------------------------------------------

    #[test]
    fn referenced_iris_collects_every_predicate_target() {
        let expr = parser::parse(&format!(
            "{}>1;\"https://example.com/b\"<2|\"https://example.com/c\"==\"x\"",
            quoted("https://example.com/a")
        ))
        .expect("parse");
        let mut iris = Vec::new();
        referenced_attribute_iris(&expr, &mut iris);
        iris.sort();
        assert_eq!(
            iris,
            vec![
                "https://example.com/a".to_string(),
                "https://example.com/b".to_string(),
                "https://example.com/c".to_string()
            ]
        );
    }

    #[test]
    fn referenced_iris_ignores_sub_attribute_segments() {
        let expr = parser::parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"").expect("parse");
        let mut iris = Vec::new();
        referenced_attribute_iris(&expr, &mut iris);
        assert_eq!(iris, vec!["https://example.com/temperature".to_string()]);
    }
}
