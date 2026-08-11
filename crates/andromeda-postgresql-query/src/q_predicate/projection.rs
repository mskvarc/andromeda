//! `q` predicates compiled to `EXISTS` probes against the `attribute_current`
//! projection.

use crate::{
    attribute_name_id::AttributeNameId,
    bind::BindBuilder,
    error::RenderError,
    operator::{comparison, pattern_match},
    q_predicate::{
        attribute_names::ResolvedAttributeNames,
        fallback::{fallback_equality, fallback_exists, fallback_ordering, fallback_pattern},
        target::QFilterTarget,
        traversal::LinkedHop,
    },
};
use andromeda_ngsi_ld_query::{
    filter::{
        expression::Expr,
        path::{AttrPath, QueryPath},
        predicate::{CmpOp, OrderingOp, PatternPolarity, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use sea_query::{Condition, Expr as SeaExpr, Value as SeaValue};

/// Compile a parsed `q` expression into a sea-query [`Condition`].
pub fn compile(expr: &Expr, target: &QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
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

/// Compile one leaf predicate, descending any linked-entity hops its axis
/// carries before compiling the comparison against the entity it lands on.
fn compile_predicate(pred: &Predicate, target: &QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
    compile_along_path(pred, pred.path(), target, names, 0)
}

/// Compile a predicate against the entity reached by the remaining axis.
///
/// `depth` is the number of hops already descended; it suffixes the aliases so
/// a nested traversal's probes cannot shadow an enclosing one's.
fn compile_along_path(
    pred: &Predicate,
    path: &QueryPath,
    target: &QFilterTarget,
    names: &ResolvedAttributeNames,
    depth: usize,
) -> Result<Condition, RenderError> {
    match path {
        QueryPath::Local(local) => compile_local_predicate(pred, local, target, names),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let Some(relationship_id) = names.get(relationship) else {
                // No `attribute_names` row means no instance of the
                // Relationship was ever written, so no entity can be reached
                // through it.
                return Ok(unsatisfiable());
            };
            let hop = LinkedHop::at_depth(depth + 1);
            let inner_condition = compile_along_path(pred, inner, &hop.inner_target(), names, depth + 1)?;
            Ok(hop.probe(relationship_id, guard_types, target, inner_condition))
        }
    }
}

/// Compile the comparison itself, routing it to the projection or the fallback.
fn compile_local_predicate(pred: &Predicate, path: &AttrPath, target: &QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, RenderError> {
    match pred {
        Predicate::Exists(_) => Ok(compile_exists(path, target, names)),
        Predicate::Equality { membership, rhs, .. } => {
            if is_projection_eligible(path) && !compare_value_exceeds_precision(rhs) {
                match names.get(&path.attr) {
                    Some(id) => Ok(compile_equality_on_projection(id, *membership, rhs, target)),
                    None => Ok(unsatisfiable()),
                }
            } else {
                Ok(fallback_equality(path, *membership, rhs, target))
            }
        }
        Predicate::Ordering { op, rhs, .. } => {
            if is_projection_eligible(path) && !scalar_exceeds_precision(rhs) {
                match names.get(&path.attr) {
                    Some(id) => Ok(compile_ordering_on_projection(id, *op, rhs, target)),
                    None => Ok(unsatisfiable()),
                }
            } else {
                Ok(fallback_ordering(path, *op, rhs, target))
            }
        }
        Predicate::MatchPattern { pattern, .. } => Ok(compile_pattern(path, pattern, PatternPolarity::Match, target, names)),
        Predicate::NotMatchPattern { pattern, .. } => Ok(compile_pattern(path, pattern, PatternPolarity::NotMatch, target, names)),
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
fn scalar_exceeds_precision(scalar: &Scalar) -> bool {
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

/// Whether any operand of an equality comparison is outside the projection's
/// precision.
fn compare_value_exceeds_precision(rhs: &CompareValue) -> bool {
    match rhs {
        CompareValue::Scalar(scalar) => scalar_exceeds_precision(scalar),
        CompareValue::Range { min, max } => scalar_exceeds_precision(min) || scalar_exceeds_precision(max),
        CompareValue::ValueList(values) => values.iter().any(scalar_exceeds_precision),
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
fn compile_exists(path: &AttrPath, target: &QFilterTarget, names: &ResolvedAttributeNames) -> Condition {
    if !is_projection_eligible(path) {
        return fallback_exists(path, target);
    }

    let Some(id) = names.get(&path.attr) else {
        return unsatisfiable();
    };

    // The markers are local to one `cust_with_values` fragment; sea-query
    // renumbers them when the enclosing statement is rendered.
    let mut binds: BindBuilder<SeaValue> = BindBuilder::default();
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
    Condition::all().add(SeaExpr::cust_with_values(sql, binds.into_values()))
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

/// Wrap a rendered value predicate — already accompanied by its binds — in the
/// correlated `attribute_current` probe.
fn projection_condition(attr: String, value_sql: String, binds: BindBuilder<SeaValue>, target: &QFilterTarget) -> Condition {
    Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.into_values()))
}

/// Compile an equality comparison (`==`/`!=`) against the typed columns of
/// `attribute_current`.
fn compile_equality_on_projection(id: AttributeNameId, membership: SetMembership, rhs: &CompareValue, target: &QFilterTarget) -> Condition {
    // The markers are local to one `cust_with_values` fragment; sea-query
    // renumbers them when the enclosing statement is rendered.
    let mut binds: BindBuilder<SeaValue> = BindBuilder::default();
    let attr = binds.bind(id.get());

    let value_sql = match rhs {
        CompareValue::Scalar(scalar) => scalar_condition(scalar, membership.as_cmp_op(), &mut binds),
        CompareValue::Range { min, max } => match membership {
            SetMembership::Within => {
                let lower = scalar_condition(min, CmpOp::GreaterEq, &mut binds);
                let upper = scalar_condition(max, CmpOp::LessEq, &mut binds);
                format!("({lower} AND {upper})")
            }
            SetMembership::Outside => {
                let below = scalar_condition(min, CmpOp::Less, &mut binds);
                let above = scalar_condition(max, CmpOp::Greater, &mut binds);
                format!("({below} OR {above})")
            }
        },
        CompareValue::ValueList(values) => {
            let arms: Vec<String> = values.iter().map(|value| scalar_condition(value, CmpOp::Equal, &mut binds)).collect();
            let joined = arms.join(" OR ");
            match membership {
                SetMembership::Within => format!("({joined})"),
                SetMembership::Outside => format!("NOT ({joined})"),
            }
        }
    };

    projection_condition(attr, value_sql, binds, target)
}

/// Compile an ordering comparison (`>`/`>=`/`<`/`<=`) against the typed columns
/// of `attribute_current`.
fn compile_ordering_on_projection(id: AttributeNameId, op: OrderingOp, rhs: &Scalar, target: &QFilterTarget) -> Condition {
    let mut binds: BindBuilder<SeaValue> = BindBuilder::default();
    let attr = binds.bind(id.get());
    let value_sql = scalar_condition(rhs, op.as_cmp_op(), &mut binds);
    projection_condition(attr, value_sql, binds, target)
}

/// Wrap a value predicate in the correlated `attribute_current` probe.
fn exists_on_projection(attr_placeholder: &str, value_sql: &str, target: &QFilterTarget) -> String {
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
/// The mapping mirrors what the temporal write path's `extract_attribute_value` writes:
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
fn scalar_condition(scalar: &Scalar, op: CmpOp, binds: &mut BindBuilder<SeaValue>) -> String {
    let operator = comparison(op);
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
        Scalar::DateTime(value) => {
            let placeholder = binds.bind(*value);
            format!("\"ac\".\"value_datetime\" {operator} {placeholder}")
        }
        Scalar::Date(_) | Scalar::Time(_) => {
            let placeholder = binds.bind(scalar.literal_text());
            format!("\"ac\".\"value_string\" {operator} {placeholder}")
        }
    }
}

// ---------------------------------------------------------------------------
// Pattern matching
// ---------------------------------------------------------------------------

/// Compile `~=` / `!~=` (clause 4.9 `patternOp` / `notPatternOp`).
///
/// Both polarities compile *inside* the `EXISTS`, so both require the attribute
/// to be present. Clause 4.9 is explicit that the entity "shall contain the
/// target element" for `notPatternOp`; an earlier compiler wrapped the whole
/// existence test in `NOT (...)`, which matched entities that simply lacked the
/// attribute.
///
/// A `gin_trgm_ops` trigram index can accelerate POSIX `~` (since PostgreSQL
/// 9.3) whenever the pattern yields extractable trigrams — `urn:ngsi-ld:.*`
/// does, an anchored `^A.*` does not. Either way the probe is narrowed by the
/// `attribute_name_id` prefix of `idx_ac_attr_str` before any regex is evaluated.
fn compile_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, target: &QFilterTarget, names: &ResolvedAttributeNames) -> Condition {
    if !is_projection_eligible(path) {
        return fallback_pattern(path, pattern, polarity, target);
    }
    let Some(id) = names.get(&path.attr) else {
        return unsatisfiable();
    };

    // The markers are local to one `cust_with_values` fragment; sea-query
    // renumbers them when the enclosing statement is rendered.
    let mut binds: BindBuilder<SeaValue> = BindBuilder::default();
    let attr = binds.bind(id.get());
    let operator = pattern_match(polarity);
    let string_placeholder = binds.bind(pattern.as_str());
    let uri_placeholder = binds.bind(pattern.as_str());
    // A pattern must also test a Relationship's object, held in `value_uri`,
    // exactly as the comparison path does (clause 4.9): a URI is a string, so
    // `owner~="urn:Person:.*"` matches. Only one column is non-null per
    // instance, so the OR holds for `~=` and `!~=` alike.
    let value_sql = format!("(\"ac\".\"value_string\" {operator} {string_placeholder} OR \"ac\".\"value_uri\" {operator} {uri_placeholder})");
    Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.into_values()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bind::{BindValue, sea_values_to_bind_values},
        schema::Entities,
    };
    use andromeda_ngsi_ld_query::filter::parser;
    use sea_query::{Alias, PostgresQueryBuilder, Query};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> ResolvedAttributeNames {
        ResolvedAttributeNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameId::from(7)),
            ("https://example.com/brandName".parse().expect("valid attribute name"), AttributeNameId::from(8)),
            ("https://example.com/active".parse().expect("valid attribute name"), AttributeNameId::from(9)),
            ("https://example.com/observed".parse().expect("valid attribute name"), AttributeNameId::from(10)),
            ("https://example.com/d".parse().expect("valid attribute name"), AttributeNameId::from(11)),
            (
                "https://example.com/temperature".parse().expect("valid attribute name"),
                AttributeNameId::from(12),
            ),
            ("https://example.com/color".parse().expect("valid attribute name"), AttributeNameId::from(13)),
            ("https://example.com/name".parse().expect("valid attribute name"), AttributeNameId::from(14)),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameId::from(15)),
            ("https://example.com/a".parse().expect("valid attribute name"), AttributeNameId::from(16)),
            ("https://example.com/b".parse().expect("valid attribute name"), AttributeNameId::from(17)),
            ("https://example.com/c".parse().expect("valid attribute name"), AttributeNameId::from(18)),
            (
                "https://example.com/isParkedIn".parse().expect("valid attribute name"),
                AttributeNameId::from(19),
            ),
            ("https://example.com/category".parse().expect("valid attribute name"), AttributeNameId::from(20)),
            ("https://example.com/outer".parse().expect("valid attribute name"), AttributeNameId::from(21)),
            ("https://example.com/inner".parse().expect("valid attribute name"), AttributeNameId::from(22)),
            ("https://example.com/zone".parse().expect("valid attribute name"), AttributeNameId::from(23)),
        ])
    }

    /// Render a compiled condition the way a real query would, so the assertions
    /// see the final placeholder numbering.
    fn render(q: &str) -> (String, Vec<BindValue<'static>>) {
        render_with(q, &names(), &QFilterTarget::entities())
    }

    fn render_with(q: &str, names: &ResolvedAttributeNames, target: &QFilterTarget) -> (String, Vec<BindValue<'static>>) {
        let expr = parser::parse(q).expect("parse q");
        let condition = compile(&expr, target, names).expect("compile q");
        let (sql, values) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        (sql, sea_values_to_bind_values(values).expect("bind values"))
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
    fn a_pattern_covers_property_values_and_relationship_objects() {
        // Clause 4.9: a URI is a String, so `~=` over a Relationship's object
        // matches; the pattern must test `value_uri` as the comparison does.
        let (sql, _) = render("\"https://example.com/brandName\"~=\"Merc.*\"");
        assert!(sql.contains("\"ac\".\"value_string\" ~"), "{sql}");
        assert!(sql.contains("\"ac\".\"value_uri\" ~"), "a pattern must also test relationship objects: {sql}");
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

    #[test]
    fn integers_within_f64_precision_still_use_the_projection() {
        let (sql, _) = render(&format!("{}==9007199254740991", quoted(SPEED)));
        assert!(sql.contains("attribute_current"), "2^53-1 is exact in f64: {sql}");
    }

    #[test]
    fn floats_are_never_treated_as_lossy() {
        // A value written as a float is already an f64-shaped quantity; only
        // large integers can lose precision moving into the projection.
        let (sql, _) = render(&format!("{}>1.5", quoted(SPEED)));
        assert!(sql.contains("\"ac\".\"value_number\" >"), "{sql}");
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
        let (sql, _) = render_with(&format!("\"{}\">1", "https://example.com/never-written"), &names(), &QFilterTarget::entities());
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("attribute_current"), "no probe is emitted when no row can exist: {sql}");
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
        let (sql, _) = render_with(
            &format!("{}>50", quoted(SPEED)),
            &names(),
            &QFilterTarget::correlated((Alias::new("tea"), Alias::new("entity_id")), (Alias::new("e"), Alias::new("payload"))),
        );
        assert!(sql.contains("\"ac\".\"entity_id\" = \"tea\".\"entity_id\""), "{sql}");
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_primary_key() {
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"ac\".\"entity_id\" = \"entities\".\"id\""), "{sql}");
    }

    // -- attribute collection ------------------------------------------------

    // -- linked-entity traversal (clause 4.9) --------------------------------

    const IS_PARKED_IN: &str = "https://example.com/isParkedIn";
    const CATEGORY: &str = "https://example.com/category";

    #[test]
    fn a_single_hop_probes_the_relationship_and_joins_the_entity_it_names() {
        let (sql, binds) = render(&format!("{}{{{}==\"commercial\"}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"attribute_current\" AS \"ac_ln1\""), "{sql}");
        assert!(
            sql.contains("JOIN \"entities\" AS \"le1\" ON \"le1\".\"id\" = \"ac_ln1\".\"value_uri\""),
            "the hop resolves the object through the projection, not the payload: {sql}"
        );
        assert!(sql.contains("\"ac_ln1\".\"entity_id\" = \"entities\".\"id\""), "{sql}");
        assert!(
            sql.contains("\"ac_ln1\".\"value_uri\" IS DISTINCT FROM 'urn:ngsi-ld:null'"),
            "a tombstoned relationship names no entity: {sql}"
        );
        // The inner comparison is correlated against the linked entity.
        assert!(sql.contains("\"ac\".\"entity_id\" = \"le1\".\"id\""), "{sql}");
        assert!(sql.contains("\"ac\".\"value_string\" ="), "{sql}");
        assert!(
            matches!(binds[0], BindValue::I16(19)),
            "the relationship's dictionary key binds first: {binds:?}"
        );
        assert!(matches!(binds[1], BindValue::I16(20)), "then the inner attribute's: {binds:?}");
    }

    #[test]
    fn a_hop_guard_tests_the_linked_entitys_types_for_any_of_its_alternatives() {
        // Clause 4.9 lists the guard's types as alternatives, so one membership
        // test against `entities.types` per type, disjoined.
        let (sql, binds) = render(&format!(
            "{}{{{},{}:{}==\"x\"}}",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Store"),
            quoted(CATEGORY)
        ));
        // The whole statement is pinned, not a substring: the guard renders as a
        // bare `a OR b`, and without parentheses around it the second disjunct
        // escapes the correlation, the attribute-name test and the tombstone
        // guard — the probe would then match whenever any entity in the tenant
        // carried the second type. A `contains` assertion cannot see that.
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS (SELECT 1 FROM "attribute_current" AS "ac_ln1" "#,
                r#"JOIN "entities" AS "le1" ON "le1"."id" = "ac_ln1"."value_uri" "#,
                r#"WHERE "ac_ln1"."entity_id" = "entities"."id" "#,
                r#"AND "ac_ln1"."attribute_name_id" = $1 "#,
                r#"AND "ac_ln1"."value_uri" IS DISTINCT FROM 'urn:ngsi-ld:null' "#,
                r#"AND (($2 = ANY("le1"."types")) OR ($3 = ANY("le1"."types"))) "#,
                r#"AND (EXISTS (SELECT 1 FROM "attribute_current" AS "ac" "#,
                r#"WHERE "ac"."attribute_name_id" = $4 AND (("ac"."value_string" = $5 OR "ac"."value_uri" = $6)) "#,
                r#"AND "ac"."value_string" IS DISTINCT FROM 'urn:ngsi-ld:null' "#,
                r#"AND "ac"."entity_id" = "le1"."id")))"#,
            )
        );
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, BindValue::String(text) if text == "https://example.com/Building")),
            "{binds:?}"
        );
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, BindValue::String(text) if text == "https://example.com/Store")),
            "{binds:?}"
        );
    }

    #[test]
    fn an_unguarded_hop_emits_no_type_test() {
        let (sql, _) = render(&format!("{}{{{}==\"commercial\"}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(!sql.contains("\"types\""), "no guard was written: {sql}");
    }

    #[test]
    fn nested_hops_number_their_aliases_by_depth() {
        let (sql, _) = render(&format!(
            "{}{{{}{{{}==\"north\"}}}}",
            quoted("https://example.com/outer"),
            quoted("https://example.com/inner"),
            quoted("https://example.com/zone")
        ));
        for alias in ["ac_ln1", "le1", "ac_ln2", "le2"] {
            assert!(sql.contains(&format!("AS \"{alias}\"")), "{alias} missing from {sql}");
        }
        assert!(
            sql.contains("\"ac_ln2\".\"entity_id\" = \"le1\".\"id\""),
            "the second hop starts at the first's target: {sql}"
        );
        assert!(sql.contains("\"ac\".\"entity_id\" = \"le2\".\"id\""), "{sql}");
    }

    #[test]
    fn a_bare_existence_through_a_hop_probes_the_catalogue_of_the_linked_entity() {
        let (sql, _) = render(&format!("{}{{{}}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"attribute_current\" AS \"ac_ln1\""), "{sql}");
        assert!(
            sql.contains("\"tea_q\".\"entity_id\" = \"le1\".\"id\""),
            "existence resolves through the catalogue of the entity the hop landed on: {sql}"
        );
    }

    #[test]
    fn an_unresolved_relationship_name_makes_the_traversal_unsatisfiable() {
        // No `attribute_names` row means no instance of the Relationship was
        // ever written, so no entity is reachable through it.
        let (sql, _) = render(&format!("{}{{{}==\"x\"}}", quoted("https://example.com/never-written"), quoted(CATEGORY)));
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("ac_ln1"), "no probe is emitted when no row can exist: {sql}");
    }
}
