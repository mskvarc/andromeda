//! `q` predicates compiled to `EXISTS` probes against the
//! `current_attribute_values` projection.

use crate::{
    attribute_name_id::AttributeNameId,
    bind::BindBuilder,
    column_reference::{qualified, quoted},
    error::RenderError,
    operator::{comparison, pattern_match},
    q_predicate::{
        attribute_names::ResolvedAttributeNames,
        fallback::{fallback_equality, fallback_exists, fallback_ordering, fallback_pattern},
        target::QFilterTarget,
        traversal::LinkedHop,
    },
    schema::CurrentAttributeValues,
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
use sea_query::{Alias, Condition, Expr as SeaExpr, Value as SeaValue};

/// Alias of the projection scan every value probe reads.
const PROBE_ALIAS: &str = "cav";

/// Quoted `"cav"."column"` reference into the probe's projection scan.
fn probe_column(column: CurrentAttributeValues) -> String {
    qualified(&Alias::new(PROBE_ALIAS), &column)
}

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
/// `current_attribute_values` stores. Sub-attributes (`temperature.observedAt`)
/// and compound value paths (`address[city]`) live inside the JSONB payload.
const fn is_projection_eligible(path: &AttrPath) -> bool {
    path.sub_attrs.is_empty() && path.value_path.is_empty()
}

/// Largest integer magnitude `f64` represents exactly.
const F64_EXACT_INTEGER_LIMIT: i128 = 1 << 53;

/// Whether a comparison target is outside what a probe can carry to the
/// projection.
///
/// `current_attribute_values.value_number` is `numeric` and so holds exactly
/// what the client wrote, but the comparison operand travels as `double
/// precision` — the widest exact numeric type this crate can bind — and is cast
/// back to `numeric` in the statement. Beyond 2^53 that round trip rounds the
/// operand, and the probe would answer against a value the client never wrote:
/// `GET` on the entity would show the exact number while the filter denied it.
///
/// Such predicates go to the JSONB fallback instead, which reads the payload and
/// compares as `numeric` throughout. That is slower and unindexed, and it stops
/// being necessary the moment a `numeric` bind value exists.
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
/// A row in the projection *is* the attribute: the tombstone value kind belongs
/// to history and never appears in current state, so an attribute deleted by
/// clause 5.6.16 leaves no row behind and presence needs no further guard.
///
/// The probe reads the leading `(entity_key, attribute_name_id)` columns of the
/// clause 4.5.5 grain constraint, so it is an index-only lookup on the same
/// index the write path conflicts against.
///
/// The obvious alternative, `payload->0 ? '<iri>'`, cannot use the GIN index on
/// `payload`: GIN's `jsonb_ops` only serves `?` when the left operand is the
/// bare indexed column, and `payload->0` is an expression. It is a sequential
/// scan.
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
        "EXISTS (SELECT 1 FROM {projection} AS \"{PROBE_ALIAS}\" \
         WHERE {attr_column} = {attr} AND {key_column} = {corr})",
        projection = quoted(&CurrentAttributeValues::Table),
        attr_column = probe_column(CurrentAttributeValues::AttributeNameId),
        attr = attr,
        key_column = probe_column(CurrentAttributeValues::EntityKey),
        corr = target.entity_key_column(),
    );
    Condition::all().add(SeaExpr::cust_with_values(sql, binds.into_values()))
}

// ---------------------------------------------------------------------------
// Comparisons against the projection
// ---------------------------------------------------------------------------

/// Wrap a rendered value predicate — already accompanied by its binds — in the
/// correlated `current_attribute_values` probe.
fn projection_condition(attr: String, value_sql: String, binds: BindBuilder<SeaValue>, target: &QFilterTarget) -> Condition {
    Condition::all().add(SeaExpr::cust_with_values(exists_on_projection(&attr, &value_sql, target), binds.into_values()))
}

/// Compile an equality comparison (`==`/`!=`) against the typed columns of
/// `current_attribute_values`.
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
/// of `current_attribute_values`.
fn compile_ordering_on_projection(id: AttributeNameId, op: OrderingOp, rhs: &Scalar, target: &QFilterTarget) -> Condition {
    let mut binds: BindBuilder<SeaValue> = BindBuilder::default();
    let attr = binds.bind(id.get());
    let value_sql = scalar_condition(rhs, op.as_cmp_op(), &mut binds);
    projection_condition(attr, value_sql, binds, target)
}

/// Wrap a value predicate in the correlated `current_attribute_values` probe.
fn exists_on_projection(attr_placeholder: &str, value_sql: &str, target: &QFilterTarget) -> String {
    format!(
        "EXISTS (SELECT 1 FROM {projection} AS \"{PROBE_ALIAS}\" \
         WHERE {attr_column} = {attr} AND ({value}) \
         AND {key_column} = {corr})",
        projection = quoted(&CurrentAttributeValues::Table),
        attr_column = probe_column(CurrentAttributeValues::AttributeNameId),
        attr = attr_placeholder,
        value = value_sql,
        key_column = probe_column(CurrentAttributeValues::EntityKey),
        corr = target.entity_key_column(),
    )
}

/// Compare one scalar against the typed column that holds values of its kind.
///
/// Every arm names exactly one typed column and compares it to a bound value, so
/// each comparison is the shape the partial index over that column is predicated
/// on: `value_number > $1` implies `value_number IS NOT NULL`, which is what
/// makes `(attribute_name_id, value_number) WHERE value_number IS NOT NULL`
/// applicable. Nothing tests `value_kind`, deliberately — the projection's
/// check constraint already ties the kind to the populated column, and an added
/// `value_kind = k` term would turn the NULL a wrong-kind row yields into
/// `FALSE`, which under the negated forms (`!=` over a value list) flips a
/// non-match into a match.
///
/// The mapping mirrors what the write path projects:
///
/// * numbers land in `value_number` (`numeric`); the operand is bound as
///   `double precision` and cast back, which is exact for every value
///   [`scalar_exceeds_precision`] admits and keeps the comparison
///   `numeric`-to-`numeric` so the typed index still applies,
/// * booleans in `value_bool`,
/// * date-times in `value_datetime`,
/// * strings in `value_string` for Properties and single-valued
///   VocabProperties, and in `value_uri` for Relationship objects — clause 4.9
///   makes a Relationship's object a legal comparison target, so a string
///   comparison has to consider both,
/// * plain dates and times in `value_string`, because only full
///   `DateTime`-typed values are projected to `value_datetime`.
fn scalar_condition(scalar: &Scalar, op: CmpOp, binds: &mut BindBuilder<SeaValue>) -> String {
    let operator = comparison(op);
    match scalar {
        Scalar::Number(number) => match number.as_f64() {
            Some(value) => {
                let placeholder = binds.bind(value);
                let column = probe_column(CurrentAttributeValues::ValueNumber);
                format!("{column} {operator} {placeholder}::numeric")
            }
            // A `serde_json::Number` that fits no f64 cannot be carried to the
            // projection at all, and one that large is out of range for
            // `scalar_exceeds_precision` to have admitted a comparison anyway.
            None => "FALSE".to_string(),
        },
        Scalar::Bool(value) => {
            let placeholder = binds.bind(*value);
            let column = probe_column(CurrentAttributeValues::ValueBool);
            format!("{column} {operator} {placeholder}")
        }
        Scalar::Str(value) => {
            let string_placeholder = binds.bind(value.as_str());
            let uri_placeholder = binds.bind(value.as_str());
            let string_column = probe_column(CurrentAttributeValues::ValueString);
            let uri_column = probe_column(CurrentAttributeValues::ValueUri);
            format!("({string_column} {operator} {string_placeholder} OR {uri_column} {operator} {uri_placeholder})")
        }
        Scalar::DateTime(value) => {
            let placeholder = binds.bind(*value);
            let column = probe_column(CurrentAttributeValues::ValueDatetime);
            format!("{column} {operator} {placeholder}")
        }
        Scalar::Date(_) | Scalar::Time(_) => {
            let placeholder = binds.bind(scalar.literal_text());
            let column = probe_column(CurrentAttributeValues::ValueString);
            format!("{column} {operator} {placeholder}")
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
/// The `gin_trgm_ops` trigram index `idx_cav_string_trgm` can accelerate POSIX
/// `~` (since PostgreSQL 9.3) whenever the pattern yields extractable trigrams
/// — `urn:ngsi-ld:.*` does, an anchored `^A.*` does not. Either way the probe
/// is narrowed by the `attribute_name_id` prefix of `idx_cav_attr_string`
/// before any regex is evaluated. The `value_uri` disjunct has no trigram index
/// of its own; it is evaluated over the rows that prefix already selected.
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
    let string_column = probe_column(CurrentAttributeValues::ValueString);
    let uri_column = probe_column(CurrentAttributeValues::ValueUri);
    let value_sql = format!("({string_column} {operator} {string_placeholder} OR {uri_column} {operator} {uri_placeholder})");
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
        assert!(sql.contains("\"current_attribute_values\""), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" >"), "{sql}");
        assert!(matches!(binds[0], BindValue::I32(7)), "attribute id binds as int4: {binds:?}");
        assert!(
            matches!(binds[1], BindValue::F64(value) if (value - 50.0).abs() < f64::EPSILON),
            "numeric literal binds as float8: {binds:?}"
        );
    }

    #[test]
    fn a_number_comparison_casts_its_operand_so_the_column_stays_numeric() {
        // `value_number` is `numeric`. Without the cast the operand's
        // `double precision` type wins operator resolution and the column is
        // coerced instead, which both rounds it and makes the typed partial
        // index inapplicable.
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"value_number\" > $2::numeric"), "{sql}");
    }

    #[test]
    fn a_pattern_covers_property_values_and_relationship_objects() {
        // Clause 4.9: a URI is a String, so `~=` over a Relationship's object
        // matches; the pattern must test `value_uri` as the comparison does.
        let (sql, _) = render("\"https://example.com/brandName\"~=\"Merc.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" ~"), "{sql}");
        assert!(sql.contains("\"cav\".\"value_uri\" ~"), "a pattern must also test relationship objects: {sql}");
    }

    #[test]
    fn string_comparison_covers_property_values_and_relationship_objects() {
        let (sql, _) = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(sql.contains("\"cav\".\"value_uri\" ="), "relationship objects are legal targets: {sql}");
    }

    #[test]
    fn no_probe_guards_against_a_stored_deletion_marker() {
        // A deleted attribute is an absent row in the projection, so there is
        // nothing to exclude and no sentinel value a client could collide with.
        let (sql, _) = render("\"https://example.com/brandName\"==\"Mercedes\"");
        assert!(!sql.contains("IS DISTINCT FROM"), "{sql}");
        assert!(!sql.contains("urn:ngsi-ld:null"), "{sql}");
    }

    #[test]
    fn no_probe_filters_on_the_value_kind_code() {
        // The projection's check constraint already pins the kind to the
        // populated column, and a `value_kind = k` term would turn the NULL a
        // wrong-kind row yields into FALSE, which a negated predicate then
        // reads as a match.
        let (sql, _) = render("\"https://example.com/color\"!=\"red\",\"blue\"");
        assert!(!sql.contains("value_kind"), "{sql}");
    }

    #[test]
    fn boolean_comparison_binds_native_boolean() {
        let (sql, binds) = render("\"https://example.com/active\"==true");
        assert!(sql.contains("\"cav\".\"value_bool\" ="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, BindValue::Bool(true))), "{binds:?}");
    }

    #[test]
    fn datetime_comparison_binds_native_timestamp() {
        let (sql, binds) = render("\"https://example.com/observed\">=2017-12-24T12:00:00Z");
        assert!(sql.contains("\"cav\".\"value_datetime\" >="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, BindValue::Timestamp(_))), "{binds:?}");
    }

    #[test]
    fn date_scalar_compares_as_text_because_the_projection_stores_it_that_way() {
        let (sql, _) = render("\"https://example.com/d\"==2024-01-01");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(!sql.contains("value_datetime"), "only full date-times reach value_datetime: {sql}");
    }

    #[test]
    fn inclusive_range_becomes_a_bounded_pair_inside_one_probe() {
        let (sql, binds) = render("\"https://example.com/temperature\"==10..20");
        assert_eq!(sql.matches("EXISTS").count(), 1, "one probe, not two: {sql}");
        assert!(sql.contains("\"cav\".\"value_number\" >="), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" <="), "{sql}");
        assert!(matches!(binds[0], BindValue::I32(12)));
        assert!(matches!(binds[1], BindValue::F64(value) if (value - 10.0).abs() < f64::EPSILON));
        assert!(matches!(binds[2], BindValue::F64(value) if (value - 20.0).abs() < f64::EPSILON));
    }

    #[test]
    fn excluded_range_requires_the_attribute_to_exist() {
        let (sql, _) = render("\"https://example.com/temperature\"!=10..20");
        assert!(sql.contains("EXISTS"), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" <"), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" >"), "{sql}");
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
        assert!(sql.contains("current_attribute_values"), "2^53-1 is exact in f64: {sql}");
    }

    #[test]
    fn floats_are_never_treated_as_lossy() {
        // A value written as a float is already an f64-shaped quantity; only
        // large integers can lose precision on the way to the probe.
        let (sql, _) = render(&format!("{}>1.5", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"value_number\" >"), "{sql}");
    }

    // -- patterns ------------------------------------------------------------

    #[test]
    fn pattern_match_uses_posix_regex_on_the_string_column() {
        let (sql, _) = render("\"https://example.com/name\"~=\"^A.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" ~"), "{sql}");
    }

    #[test]
    fn negated_pattern_sits_inside_the_probe_so_missing_attributes_do_not_match() {
        let (sql, _) = render("\"https://example.com/name\"!~=\"^A.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" !~"), "{sql}");
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
        assert!(!sql.contains("current_attribute_values"), "no probe is emitted when no row can exist: {sql}");
    }

    // -- existence -----------------------------------------------------------

    #[test]
    fn bare_existence_is_the_presence_of_a_projection_row() {
        // A deleted attribute leaves no row, so presence needs no second probe
        // and no tombstone test.
        let (sql, binds) = render(&quoted(SPEED));
        pretty_assertions::assert_eq!(
            sql,
            concat!(
                r#"SELECT "id" FROM "entities" WHERE EXISTS (SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_id" = $1 AND "cav"."entity_key" = "entities"."entity_key")"#,
            )
        );
        assert!(matches!(binds[0], BindValue::I32(7)), "{binds:?}");
    }

    #[test]
    fn unknown_attribute_existence_compiles_to_a_constant_false() {
        let (sql, _) = render("\"https://example.com/never-written\"");
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("current_attribute_values"), "{sql}");
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
    fn a_correlated_statement_probes_against_the_entity_key_of_its_own_alias() {
        let (sql, _) = render_with(
            &format!("{}>50", quoted(SPEED)),
            &names(),
            &QFilterTarget::correlated((Alias::new("ce"), Alias::new("entity_key")), (Alias::new("ce"), Alias::new("payload"))),
        );
        assert!(sql.contains("\"cav\".\"entity_key\" = \"ce\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn entity_queries_correlate_on_the_entities_surrogate_key() {
        let (sql, _) = render(&format!("{}>50", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"entity_key\" = \"entities\".\"entity_key\""), "{sql}");
        assert!(!sql.contains("\"entities\".\"id\""), "the URI is not the correlation column: {sql}");
    }

    // -- attribute collection ------------------------------------------------

    // -- linked-entity traversal (clause 4.9) --------------------------------

    const IS_PARKED_IN: &str = "https://example.com/isParkedIn";
    const CATEGORY: &str = "https://example.com/category";

    #[test]
    fn a_single_hop_probes_the_relationship_and_joins_the_entity_it_names() {
        let (sql, binds) = render(&format!("{}{{{}==\"commercial\"}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"current_attribute_values\" AS \"cav_ln1\""), "{sql}");
        assert!(
            sql.contains("JOIN \"entities\" AS \"le1\" ON \"le1\".\"id\" = \"cav_ln1\".\"value_uri\""),
            "the hop resolves the object through the projection, not the payload: {sql}"
        );
        assert!(sql.contains("\"cav_ln1\".\"entity_key\" = \"entities\".\"entity_key\""), "{sql}");
        // The inner comparison is correlated against the linked entity's key,
        // which is the row the join on its URI landed on.
        assert!(sql.contains("\"cav\".\"entity_key\" = \"le1\".\"entity_key\""), "{sql}");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(
            matches!(binds[0], BindValue::I32(19)),
            "the relationship's dictionary key binds first: {binds:?}"
        );
        assert!(matches!(binds[1], BindValue::I32(20)), "then the inner attribute's: {binds:?}");
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
                r#"SELECT "id" FROM "entities" WHERE EXISTS (SELECT 1 FROM "current_attribute_values" AS "cav_ln1" "#,
                r#"JOIN "entities" AS "le1" ON "le1"."id" = "cav_ln1"."value_uri" "#,
                r#"WHERE "cav_ln1"."entity_key" = "entities"."entity_key" "#,
                r#"AND "cav_ln1"."attribute_name_id" = $1 "#,
                r#"AND (("le1"."types" @> ARRAY(SELECT $2::TEXT)) OR ("le1"."types" @> ARRAY(SELECT $3::TEXT))) "#,
                r#"AND (EXISTS (SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_id" = $4 AND (("cav"."value_string" = $5 OR "cav"."value_uri" = $6)) "#,
                r#"AND "cav"."entity_key" = "le1"."entity_key")))"#,
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
        for alias in ["cav_ln1", "le1", "cav_ln2", "le2"] {
            assert!(sql.contains(&format!("AS \"{alias}\"")), "{alias} missing from {sql}");
        }
        assert!(
            sql.contains("\"cav_ln2\".\"entity_key\" = \"le1\".\"entity_key\""),
            "the second hop starts at the first's target: {sql}"
        );
        assert!(sql.contains("\"cav\".\"entity_key\" = \"le2\".\"entity_key\""), "{sql}");
    }

    #[test]
    fn a_bare_existence_through_a_hop_probes_the_projection_of_the_linked_entity() {
        let (sql, _) = render(&format!("{}{{{}}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(sql.contains("\"current_attribute_values\" AS \"cav_ln1\""), "{sql}");
        assert!(
            sql.contains("\"cav\".\"entity_key\" = \"le1\".\"entity_key\""),
            "existence resolves against the entity the hop landed on: {sql}"
        );
    }

    #[test]
    fn an_unresolved_relationship_name_makes_the_traversal_unsatisfiable() {
        // No `attribute_names` row means no instance of the Relationship was
        // ever written, so no entity is reachable through it.
        let (sql, _) = render(&format!("{}{{{}==\"x\"}}", quoted("https://example.com/never-written"), quoted(CATEGORY)));
        assert!(sql.contains("FALSE"), "{sql}");
        assert!(!sql.contains("cav_ln1"), "no probe is emitted when no row can exist: {sql}");
    }
}
