//! `q` predicates compiled to `EXISTS` probes against the
//! `current_attribute_values` projection.

use crate::{
    attribute_name_id::AttributeNameId,
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    operator::{compare, pattern as pattern_predicate},
    q_predicate::{
        fallback::{fallback_equality, fallback_exists, fallback_ordering, fallback_pattern},
        traversal::LinkedHop,
    },
};
use andromeda_ngsi_ld_lexicon::{ngsi_ld_date::NgsiLdDate, ngsi_ld_time::NgsiLdTime};
use andromeda_ngsi_ld_query::{
    filter::{
        expression::Expr,
        path::{AttrPath, QueryPath},
        predicate::{CmpOp, OrderingOp, PatternPolarity, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_postgresql_schema::current::CurrentAttributeValues;
use andromeda_postgresql_seaquery::literal::{false_literal, int_literal};
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, Iden, Query, SimpleExpr};

/// The alias every value probe reads the `current_attribute_values` scan under.
#[derive(Iden, Clone, Copy)]
#[iden = "cav"]
struct Cav;

/// A `"cav"."column"` reference into the probe's projection scan.
fn probe_column(column: CurrentAttributeValues) -> SimpleExpr {
    SeaExpr::col((Cav, column))
}

/// Compile a parsed `q` expression into a sea-query [`Condition`].
///
/// # Errors
///
/// Returns [`RenderError`] when a term of the expression cannot be rendered.
/// No shape clause 4.9 admits reaches that: an attribute or entity type name
/// the dictionary does not hold compiles to a constant `FALSE` rather than
/// failing, and every operand is bound as a parameter rather than converted, so
/// the result is `Ok` for every expression the `q` parser accepts.
pub fn compile(expr: &Expr, rows: &EntityRowRefs, names: &QueryNames) -> Result<Condition, RenderError> {
    match expr {
        Expr::And(terms) => terms.iter().try_fold(Condition::all(), |cond, term| Ok(cond.add(compile(term, rows, names)?))),
        Expr::Or(terms) => terms.iter().try_fold(Condition::any(), |cond, term| Ok(cond.add(compile(term, rows, names)?))),
        Expr::Pred(pred) => compile_predicate(pred, rows, names),
    }
}

/// Compile one leaf predicate, descending any linked-entity hops its axis
/// carries before compiling the comparison against the entity it lands on.
fn compile_predicate(pred: &Predicate, rows: &EntityRowRefs, names: &QueryNames) -> Result<Condition, RenderError> {
    compile_along_path(pred, pred.path(), rows, names, 0)
}

/// Compile a predicate against the entity reached by the remaining axis.
///
/// `depth` is the number of hops already descended; it suffixes the aliases so
/// a nested traversal's probes cannot shadow an enclosing one's.
fn compile_along_path(pred: &Predicate, path: &QueryPath, rows: &EntityRowRefs, names: &QueryNames, depth: usize) -> Result<Condition, RenderError> {
    match path {
        QueryPath::Local(local) => Ok(compile_local_predicate(pred, local, rows, names)),
        QueryPath::Linked {
            relationship,
            guard_types,
            inner,
        } => {
            let Some(relationship_id) = names.attributes.get(relationship) else {
                // No `attribute_names` row means no instance of the
                // Relationship was ever written, so no entity can be reached
                // through it.
                return Ok(unsatisfiable());
            };
            let hop = LinkedHop::at_depth(depth + 1);
            let inner_condition = compile_along_path(pred, inner, &hop.inner_rows(), names, depth + 1)?;
            Ok(hop.probe(relationship_id, guard_types, &names.types, rows, inner_condition))
        }
    }
}

/// Compile the comparison itself, routing it to the projection or the fallback.
fn compile_local_predicate(pred: &Predicate, path: &AttrPath, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    match pred {
        Predicate::Exists(_) => compile_exists(path, rows, names),
        Predicate::Equality { membership, rhs, .. } => {
            if is_projection_eligible(path) {
                match names.attributes.get(&path.attr) {
                    Some(id) => compile_equality_on_projection(id, *membership, rhs, rows),
                    None => unsatisfiable(),
                }
            } else {
                fallback_equality(path, *membership, rhs, rows)
            }
        }
        Predicate::Ordering { op, rhs, .. } => {
            if is_projection_eligible(path) {
                match names.attributes.get(&path.attr) {
                    Some(id) => compile_ordering_on_projection(id, *op, rhs, rows),
                    None => unsatisfiable(),
                }
            } else {
                fallback_ordering(path, *op, rhs, rows)
            }
        }
        Predicate::MatchPattern { pattern, .. } => compile_pattern(path, pattern, PatternPolarity::Match, rows, names),
        Predicate::NotMatchPattern { pattern, .. } => compile_pattern(path, pattern, PatternPolarity::NotMatch, rows, names),
    }
}

/// Whether a path addresses an attribute's own value, which is the only thing
/// `current_attribute_values` stores. Sub-attributes (`temperature.observedAt`)
/// and compound value paths (`address[city]`) live inside the JSONB payload.
const fn is_projection_eligible(path: &AttrPath) -> bool {
    path.sub_attrs.is_empty() && path.value_path.is_empty()
}

/// A predicate that no row can satisfy.
fn unsatisfiable() -> Condition {
    Condition::all().add(false_literal())
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
fn compile_exists(path: &AttrPath, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    if !is_projection_eligible(path) {
        return fallback_exists(path, rows);
    }

    let Some(id) = names.attributes.get(&path.attr) else {
        return unsatisfiable();
    };

    let mut probe = Query::select();
    probe
        .expr(int_literal(1))
        .from_as(CurrentAttributeValues::Table, Cav)
        .and_where(SeaExpr::expr(probe_column(CurrentAttributeValues::AttributeNameId)).eq(id.get()))
        .and_where(SeaExpr::expr(probe_column(CurrentAttributeValues::EntityKey)).eq(rows.entity_key_column()));
    Condition::all().add(SeaExpr::exists(probe))
}

// ---------------------------------------------------------------------------
// Comparisons against the projection
// ---------------------------------------------------------------------------

/// Wrap a value condition in the correlated `current_attribute_values` probe.
///
/// The probe is an `EXISTS` over the projection, narrowed to the attribute's
/// dictionary key and the value condition, and correlated on the enclosing
/// statement's entity key. The attribute-name test is added before the value so
/// its dictionary key binds ahead of the value's operands, and the correlation
/// is added last so it binds nothing after them.
fn exists_on_projection(id: AttributeNameId, value: Condition, rows: &EntityRowRefs) -> Condition {
    let mut probe = Query::select();
    probe
        .expr(int_literal(1))
        .from_as(CurrentAttributeValues::Table, Cav)
        .and_where(SeaExpr::expr(probe_column(CurrentAttributeValues::AttributeNameId)).eq(id.get()))
        .cond_where(value)
        .and_where(SeaExpr::expr(probe_column(CurrentAttributeValues::EntityKey)).eq(rows.entity_key_column()));
    Condition::all().add(SeaExpr::exists(probe))
}

/// Compile an equality comparison (`==`/`!=`) against the typed columns of
/// `current_attribute_values`.
fn compile_equality_on_projection(id: AttributeNameId, membership: SetMembership, rhs: &CompareValue, rows: &EntityRowRefs) -> Condition {
    let value = match rhs {
        CompareValue::Scalar(scalar) => Condition::all().add(scalar_condition(scalar, membership.as_cmp_op())),
        CompareValue::Range { min, max } => match membership {
            SetMembership::Within => Condition::all()
                .add(scalar_condition(min, CmpOp::GreaterEq))
                .add(scalar_condition(max, CmpOp::LessEq)),
            SetMembership::Outside => Condition::any()
                .add(scalar_condition(min, CmpOp::Less))
                .add(scalar_condition(max, CmpOp::Greater)),
        },
        CompareValue::ValueList(values) => {
            let arms = values
                .iter()
                .fold(Condition::any(), |cond, value| cond.add(scalar_condition(value, CmpOp::Equal)));
            match membership {
                SetMembership::Within => arms,
                SetMembership::Outside => arms.not(),
            }
        }
    };
    exists_on_projection(id, value, rows)
}

/// Compile an ordering comparison (`>`/`>=`/`<`/`<=`) against the typed columns
/// of `current_attribute_values`.
fn compile_ordering_on_projection(id: AttributeNameId, op: OrderingOp, rhs: &Scalar, rows: &EntityRowRefs) -> Condition {
    exists_on_projection(id, Condition::all().add(scalar_condition(rhs, op.as_cmp_op())), rows)
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
/// * numbers land in `value_number` (`numeric`) and bind directly as an exact
///   decimal, so every supported operand uses the typed numeric index,
/// * booleans in `value_bool`,
/// * date-times in `value_datetime`,
/// * strings in `value_string` for Properties and single-valued
///   `VocabProperties`, and — under the equality operators only — in `value_uri`
///   for Relationship objects, which is what [`relationship_object_is_comparable`]
///   decides,
/// * plain dates and times in `value_string`, because only full
///   `DateTime`-typed values are projected to `value_datetime`. Both are stored
///   in the one canonical, fixed-width form their clause 4.6.3 types define, so
///   byte-wise order over the column is chronological order and an ordering
///   comparison against the bound canonical text is a temporal comparison.
fn scalar_condition(scalar: &Scalar, op: CmpOp) -> SimpleExpr {
    match scalar {
        // SeaQuery owns every value in its AST, so this is the one necessary
        // clone between the borrowed query domain value and the NUMERIC bind.
        Scalar::Number(number) => compare(probe_column(CurrentAttributeValues::ValueNumber), op, SeaExpr::val(number.decimal().clone())),
        Scalar::Bool(value) => compare(probe_column(CurrentAttributeValues::ValueBool), op, SeaExpr::val(*value)),
        Scalar::Str(value) => {
            let string_test = compare(probe_column(CurrentAttributeValues::ValueString), op, SeaExpr::val(value.as_str()));
            match relationship_object_is_comparable(op) {
                RelationshipObjects::Comparable => {
                    let uri_test = compare(probe_column(CurrentAttributeValues::ValueUri), op, SeaExpr::val(value.as_str()));
                    string_test.or(uri_test)
                }
                RelationshipObjects::NonMatching => string_test,
            }
        }
        Scalar::DateTime(value) => compare(probe_column(CurrentAttributeValues::ValueDatetime), op, SeaExpr::val(*value)),
        Scalar::Date(value) => compare(
            probe_column(CurrentAttributeValues::ValueString),
            op,
            SeaExpr::val(NgsiLdDate::from(*value).to_string()),
        ),
        // The operand is rendered in the same canonical form the write path
        // stores, rather than in the clause 4.9 serialisation the client wrote:
        // `12:00:00+01:00` and `11:00:00Z` are one time of day, and only the
        // canonical text of both compares against the stored column.
        Scalar::Time(value) => match NgsiLdTime::normalise(*value) {
            Ok(canonical) => compare(probe_column(CurrentAttributeValues::ValueString), op, SeaExpr::val(canonical.to_string())),
            // A time of day with no canonical form is one no write path could
            // have stored, so nothing can compare equal to it.
            Err(_) => false_literal(),
        },
    }
}

/// Whether a Relationship's object is a comparison target under one operator.
///
/// `current_attribute_values.value_uri` holds Relationship objects and nothing
/// else, so probing it is exactly the decision of whether the predicate may
/// match a Relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelationshipObjects {
    /// The operator admits a Relationship's object as a comparison target.
    Comparable,
    /// Clause 4.9 requires a Relationship operand to non-match this operator.
    NonMatching,
}

/// Clause 4.9: a Relationship (and a `ListRelationship`) operand shall non-match
/// every operator other than equality and inequality.
///
/// A URI names a resource; it carries no order a `<` or `>` could mean, and the
/// pattern operators are defined over String targets. Probing `value_uri` under
/// an ordering operator therefore made `owner>"a"` select entities the standard
/// requires it to reject.
///
/// Known gap, left deliberately: a `ListRelationship`'s objects are projected into
/// `value_object` as a JSON array, not into `value_uri`, so equality against one
/// of its members is not answered by this probe at all. Clause 4.9 admits it, and
/// closing it needs a containment predicate over `value_object` that the
/// projection carries no index for; it is a separate change from this gating.
const fn relationship_object_is_comparable(op: CmpOp) -> RelationshipObjects {
    match op {
        CmpOp::Equal | CmpOp::Unequal => RelationshipObjects::Comparable,
        CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq => RelationshipObjects::NonMatching,
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
/// Only `value_string` is tested. Clause 4.9 requires a Relationship operand to
/// non-match every operator other than equality and inequality, and the pattern
/// operators are defined over String targets — so a Relationship's object, which
/// is the whole content of `value_uri`, is never a pattern target. Probing it
/// made `owner~="urn:Person:.*"` select entities the standard requires it to
/// reject.
///
/// The attribute-aware trigram index `idx_cav_attr_string_trgm` on
/// `(attribute_name_id int4_ops, value_string gin_trgm_ops)` can accelerate
/// POSIX `~` (since PostgreSQL 9.3) whenever the pattern yields extractable
/// trigrams — `urn:ngsi-ld:.*` does, an anchored `^A.*` does not — and it
/// intersects those trigrams with the attribute key, so a pattern common
/// across attributes yields candidates only from the one probed. When no
/// trigram is extractable the probe is still narrowed by the
/// `attribute_name_id` prefix of `idx_cav_attr_string` before any regex is
/// evaluated.
fn compile_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, rows: &EntityRowRefs, names: &QueryNames) -> Condition {
    if !is_projection_eligible(path) {
        return fallback_pattern(path, pattern, polarity, rows);
    }
    let Some(id) = names.attributes.get(&path.attr) else {
        return unsatisfiable();
    };

    let value = Condition::all().add(pattern_predicate(
        probe_column(CurrentAttributeValues::ValueString),
        polarity,
        SeaExpr::val(pattern.as_str()),
    ));
    exists_on_projection(id, value, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{dictionary::resolved_names::ResolvedNames, entity_row_refs::DocumentsRef, entity_type_id::EntityTypeId};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::parser;
    use andromeda_postgresql_schema::current::Entities;
    use sea_query::{Alias, PostgresQueryBuilder, Query, Value};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> QueryNames {
        QueryNames {
            attributes: attribute_names(),
            types: ResolvedNames::from_pairs([
                ("https://example.com/Building".parse().expect("valid type name"), EntityTypeId::from(31)),
                ("https://example.com/Store".parse().expect("valid type name"), EntityTypeId::from(32)),
            ]),
        }
    }

    fn attribute_names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([
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
    fn render(q: &str) -> (String, Vec<Value>) {
        render_with(q, &names(), &EntityRowRefs::unaliased())
    }

    fn render_with(q: &str, names: &QueryNames, rows: &EntityRowRefs) -> (String, Vec<Value>) {
        let expr = parser::parse(q).expect("parse q");
        let condition = compile(&expr, rows, names).expect("compile q");
        let (sql, values) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        (sql, values.0)
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
        assert!(matches!(binds[0], Value::Int(Some(7))), "attribute id binds as int4: {binds:?}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "50"),
            "numeric literal binds as NUMERIC: {binds:?}"
        );
    }

    #[test]
    fn a_number_comparison_binds_numeric_without_a_double_conversion() {
        let (sql, binds) = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
        assert!(sql.contains("\"cav\".\"value_number\" > $2"), "{sql}");
        assert!(!sql.contains("double precision"), "{sql}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "0.100000000000000005"),
            "{binds:?}"
        );
    }

    #[test]
    fn a_pattern_does_not_reach_relationship_objects() {
        // Clause 4.9 requires a Relationship operand to non-match every operator
        // other than equality and inequality, and the pattern operators are
        // defined over String targets. `value_uri` holds Relationship objects and
        // nothing else, so a pattern must not probe it.
        let (sql, _) = render("\"https://example.com/brandName\"~=\"Merc.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" ~"), "{sql}");
        assert!(!sql.contains("value_uri"), "a pattern must not test relationship objects: {sql}");
    }

    #[test]
    fn a_negated_pattern_does_not_reach_relationship_objects_either() {
        let (sql, _) = render("\"https://example.com/brandName\"!~=\"Merc.*\"");
        assert!(sql.contains("\"cav\".\"value_string\" !~"), "{sql}");
        assert!(!sql.contains("value_uri"), "{sql}");
    }

    #[test]
    fn equality_covers_property_values_and_relationship_objects() {
        for expression in [
            "\"https://example.com/brandName\"==\"Mercedes\"",
            "\"https://example.com/brandName\"!=\"Mercedes\"",
        ] {
            let (sql, _) = render(expression);
            assert!(sql.contains("\"cav\".\"value_string\" "), "{expression}: {sql}");
            assert!(
                sql.contains("\"cav\".\"value_uri\" "),
                "{expression}: relationship objects are legal equality targets: {sql}"
            );
        }
    }

    #[test]
    fn an_ordering_comparison_does_not_reach_relationship_objects() {
        // Clause 4.9: a Relationship operand shall non-match every operator other
        // than equality and inequality. A URI carries no order a `<` could mean.
        for expression in [
            "\"https://example.com/brandName\">\"Mercedes\"",
            "\"https://example.com/brandName\">=\"Mercedes\"",
            "\"https://example.com/brandName\"<\"Mercedes\"",
            "\"https://example.com/brandName\"<=\"Mercedes\"",
        ] {
            let (sql, _) = render(expression);
            assert!(sql.contains("\"cav\".\"value_string\" "), "{expression}: {sql}");
            assert!(!sql.contains("value_uri"), "{expression}: {sql}");
        }
    }

    #[test]
    fn a_string_range_bound_keeps_relationship_objects_out_of_its_ordering_halves() {
        // A range compiles to two ordering comparisons, so it inherits the
        // clause 4.9 gating rather than restating it.
        let (sql, _) = render("\"https://example.com/brandName\"==\"a\"..\"z\"");
        assert!(!sql.contains("value_uri"), "{sql}");
    }

    #[test]
    fn a_value_list_still_covers_relationship_objects_in_both_polarities() {
        // Every arm of a value list is an equality comparison (clause 4.9
        // Table 4.9-1), so each one may match a Relationship's object.
        for expression in [
            "\"https://example.com/color\"==\"red\",\"blue\"",
            "\"https://example.com/color\"!=\"red\",\"blue\"",
        ] {
            let (sql, _) = render(expression);
            assert_eq!(sql.matches("value_uri").count(), 2, "{expression}: one per list member: {sql}");
        }
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
        assert!(binds.iter().any(|b| matches!(b, Value::Bool(Some(true)))), "{binds:?}");
    }

    #[test]
    fn datetime_comparison_binds_native_timestamp() {
        let (sql, binds) = render("\"https://example.com/observed\">=2017-12-24T12:00:00Z");
        assert!(sql.contains("\"cav\".\"value_datetime\" >="), "{sql}");
        assert!(binds.iter().any(|b| matches!(b, Value::ChronoDateTimeUtc(_))), "{binds:?}");
    }

    #[test]
    fn date_scalar_compares_as_text_because_the_projection_stores_it_that_way() {
        let (sql, _) = render("\"https://example.com/d\"==2024-01-01");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(!sql.contains("value_datetime"), "only full date-times reach value_datetime: {sql}");
    }

    #[test]
    fn a_time_operand_binds_the_canonical_form_the_write_path_stores() {
        // Clause 4.6.3 admits any offset and any fractional width, and the write
        // path stores one canonical rendering of them. The probe has to bind that
        // same rendering or an offset-bearing operand matches nothing.
        let (_, binds) = render("\"https://example.com/d\"==\"13:00:00+01:00\"");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "12:00:00.000000Z")),
            "{binds:?}"
        );
    }

    #[test]
    fn a_time_ordering_comparison_binds_the_canonical_form_too() {
        // The stored column is byte-wise collated, so the comparison is
        // chronological exactly because both sides are the fixed-width form.
        let (sql, binds) = render("\"https://example.com/d\">\"09:30:00.5Z\"");
        assert!(sql.contains("\"cav\".\"value_string\" >"), "{sql}");
        assert!(
            binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.as_str() == "09:30:00.500000Z")),
            "{binds:?}"
        );
    }

    #[test]
    fn inclusive_range_becomes_a_bounded_pair_inside_one_probe() {
        let (sql, binds) = render("\"https://example.com/temperature\"==10..20");
        assert_eq!(sql.matches("EXISTS").count(), 1, "one probe, not two: {sql}");
        assert!(sql.contains("\"cav\".\"value_number\" >="), "{sql}");
        assert!(sql.contains("\"cav\".\"value_number\" <="), "{sql}");
        assert!(matches!(binds[0], Value::Int(Some(12))));
        assert!(matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "10"));
        assert!(matches!(&binds[2], Value::BigDecimal(Some(value)) if value.to_string() == "20"));
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
    fn integers_beyond_binary64_precision_use_the_numeric_projection() {
        let (sql, binds) = render(&format!("{}==9007199254740993", quoted(SPEED)));
        assert!(sql.contains("current_attribute_values"), "{sql}");
        assert!(!sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(
            matches!(&binds[1], Value::BigDecimal(Some(value)) if value.to_string() == "9007199254740993"),
            "{binds:?}"
        );
    }

    #[test]
    fn exact_fractions_use_the_projection() {
        let (sql, _) = render(&format!("{}>0.100000000000000005", quoted(SPEED)));
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
        let (sql, _) = render_with(&format!("\"{}\">1", "https://example.com/never-written"), &names(), &EntityRowRefs::unaliased());
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
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_id" = $1 AND "cav"."entity_key" = "entities"."entity_key")"#,
            )
        );
        assert!(matches!(binds[0], Value::Int(Some(7))), "{binds:?}");
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
            &EntityRowRefs::aliased(Alias::new("ce"), Alias::new("cs"), DocumentsRef::ByKeyLookup),
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
        assert!(!sql.contains("entity_search"), "a hop without a type guard reads no search row: {sql}");
        // The inner comparison is correlated against the linked entity's key,
        // which is the row the join on its URI landed on.
        assert!(sql.contains("\"cav\".\"entity_key\" = \"le1\".\"entity_key\""), "{sql}");
        assert!(sql.contains("\"cav\".\"value_string\" ="), "{sql}");
        assert!(
            matches!(binds[0], Value::Int(Some(19))),
            "the relationship's dictionary key binds first: {binds:?}"
        );
        assert!(matches!(binds[1], Value::Int(Some(20))), "then the inner attribute's: {binds:?}");
    }

    #[test]
    fn a_hop_guard_tests_the_linked_entitys_types_for_any_of_its_alternatives() {
        // Clause 4.9 lists the guard's types as alternatives, so one membership
        // test against `entities.type_ids` per resolved type, disjoined.
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
                r#"SELECT "id" FROM "entities" WHERE EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav_ln1" "#,
                r#"JOIN "entities" AS "le1" ON "le1"."id" = "cav_ln1"."value_uri" "#,
                r#"JOIN "entity_search" AS "ls1" ON "ls1"."entity_key" = "le1"."entity_key" "#,
                r#"WHERE "cav_ln1"."entity_key" = "entities"."entity_key" "#,
                r#"AND "cav_ln1"."attribute_name_id" = $1 "#,
                r#"AND (("ls1"."type_ids" @> (ARRAY[31])) OR ("ls1"."type_ids" @> (ARRAY[32]))) "#,
                r#"AND EXISTS(SELECT 1 FROM "current_attribute_values" AS "cav" "#,
                r#"WHERE "cav"."attribute_name_id" = $2 AND ("cav"."value_string" = $3 OR "cav"."value_uri" = $4) "#,
                r#"AND "cav"."entity_key" = "le1"."entity_key"))"#,
            )
        );
        assert!(
            !binds.iter().any(|bind| matches!(bind, Value::Int(Some(31 | 32)))),
            "the guard type keys are inlined for the planner, never bound: {binds:?}"
        );
        assert!(
            !binds
                .iter()
                .any(|bind| matches!(bind, Value::String(Some(text)) if text.contains("example.com/Building"))),
            "the type IRI itself never reaches the statement: {binds:?}"
        );
    }

    /// A guard type the dictionary never recorded was never carried by any
    /// entity, so that alternative is unsatisfiable while the resolved one
    /// still selects.
    #[test]
    fn an_unresolved_guard_type_renders_as_an_unsatisfiable_alternative() {
        let (sql, _) = render(&format!(
            "{}{{{},{}:{}==\"x\"}}",
            quoted(IS_PARKED_IN),
            quoted("https://example.com/Building"),
            quoted("https://example.com/Warehouse"),
            quoted(CATEGORY)
        ));
        assert!(sql.contains(r#"(("ls1"."type_ids" @> (ARRAY[31])) OR (FALSE))"#), "{sql}");
    }

    #[test]
    fn an_unguarded_hop_emits_no_type_test() {
        let (sql, _) = render(&format!("{}{{{}==\"commercial\"}}", quoted(IS_PARKED_IN), quoted(CATEGORY)));
        assert!(!sql.contains("\"type_ids\""), "no guard was written: {sql}");
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
