//! `q` predicates compiled to `EXISTS` probes against the `attribute_current`
//! projection.

use crate::{
    error::QueryError,
    filter::{
        expression::Expr,
        path::AttrPath,
        predicate::{CmpOp, PatternPolarity, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
        sql::{
            attribute_names::ResolvedAttributeNames,
            fallback::{fallback_compare, fallback_exists, fallback_pattern},
            target::QFilterTarget,
        },
    },
    pattern::Pattern,
    sql::bind::BindBuilder,
};
use andromeda_domain::{attribute::name::AttributeName, temporal::AttributeNameId};
use sea_query::{Condition, Expr as SeaExpr, Value as SeaValue};

/// Compile a parsed `q` expression into a sea-query [`Condition`].
pub fn compile(expr: &Expr, target: QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, QueryError> {
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

/// Attribute names a `q` expression references at the top level.
///
/// Only these need dictionary resolution: sub-attribute and value-path
/// segments are navigated inside the JSONB payload, never through
/// `attribute_names`.
pub fn referenced_attribute_names(expr: &Expr, out: &mut Vec<AttributeName>) {
    match expr {
        Expr::And(terms) | Expr::Or(terms) => {
            for term in terms {
                referenced_attribute_names(term, out);
            }
        }
        Expr::Pred(pred) => {
            let path = match pred {
                Predicate::Exists(path) | Predicate::Compare { path, .. } | Predicate::MatchPattern { path, .. } | Predicate::NotMatchPattern { path, .. } => {
                    path
                }
            };
            // clone: the caller owns the resolved list, which outlives the expression
            out.push(path.attr.clone());
        }
    }
}

/// Compile one leaf predicate, routing it to the projection or the fallback.
fn compile_predicate(pred: &Predicate, target: QFilterTarget, names: &ResolvedAttributeNames) -> Result<Condition, QueryError> {
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

/// Compile a comparison against the typed columns of `attribute_current`.
fn compile_compare_on_projection(id: AttributeNameId, op: CmpOp, rhs: &CompareValue, target: QFilterTarget) -> Result<Condition, QueryError> {
    // The markers are local to one `cust_with_values` fragment; sea-query
    // renumbers them when the enclosing statement is rendered.
    let mut binds: BindBuilder<SeaValue> = BindBuilder::default();
    let attr = binds.bind(id.get());

    let value_sql = match rhs {
        CompareValue::Scalar(scalar) => scalar_condition(scalar, op, &mut binds),
        CompareValue::Range { min, max } => match op.set_membership().ok_or(QueryError::RangeOperatorUnsupported)? {
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
            match op.set_membership().ok_or(QueryError::ValueListOperatorUnsupported)? {
                SetMembership::Within => format!("({joined})"),
                SetMembership::Outside => format!("NOT ({joined})"),
            }
        }
    };

    Ok(Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.into_values())))
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
fn scalar_condition(scalar: &Scalar, op: CmpOp, binds: &mut BindBuilder<SeaValue>) -> String {
    let operator = op.sql_operator();
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
/// POSIX `~` is not accelerated by the trigram index (`docs/postgresql.md`),
/// but the probe is still narrowed by the `attribute_name_id` prefix of
/// `idx_ac_attr_str` before any regex is evaluated.
fn compile_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, target: QFilterTarget, names: &ResolvedAttributeNames) -> Condition {
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
    let pattern_placeholder = binds.bind(pattern.as_str());
    let value_sql = format!("\"ac\".\"value_string\" {} {}", polarity.operator(), pattern_placeholder);
    Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.into_values()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        filter::parser,
        sql::{
            bind::{BindValue, sea_values_to_bind_values},
            schema::Entities,
        },
    };
    use sea_query::{PostgresQueryBuilder, Query};

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
        let (sql, _) = render_with(&format!("\"{}\">1", "https://example.com/never-written"), &names(), QFilterTarget::Entities);
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
        let (sql, _) = render_with(&format!("{}>50", quoted(SPEED)), &names(), QFilterTarget::TemporalStage1);
        assert!(sql.contains("\"ac\".\"entity_id\" = \"tea\".\"entity_id\""), "{sql}");
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_primary_key() {
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"ac\".\"entity_id\" = \"entities\".\"id\""), "{sql}");
    }

    // -- attribute collection ------------------------------------------------

    #[test]
    fn referenced_names_collect_every_predicate_target() {
        let expr = parser::parse(&format!(
            "{}>1;\"https://example.com/b\"<2|\"https://example.com/c\"==\"x\"",
            quoted("https://example.com/a")
        ))
        .expect("parse");
        let mut names = Vec::new();
        referenced_attribute_names(&expr, &mut names);
        names.sort();
        assert_eq!(
            names.iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
            vec!["https://example.com/a", "https://example.com/b", "https://example.com/c"]
        );
    }

    #[test]
    fn referenced_names_ignore_sub_attribute_segments() {
        let expr = parser::parse("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"").expect("parse");
        let mut names = Vec::new();
        referenced_attribute_names(&expr, &mut names);
        assert_eq!(
            names.iter().map(AttributeName::as_str).collect::<Vec<&str>>(),
            vec!["https://example.com/temperature"]
        );
    }
}
