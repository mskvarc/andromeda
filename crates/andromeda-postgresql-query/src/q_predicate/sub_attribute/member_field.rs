//! The members of an attribute instance a `q` path ends on.
//!
//! Clause 4.9 lets the last name of a path address a non-reified member of
//! the instance before it (`temperature.observedAt`), and a path is read the
//! same way here as in the evaluator that matches a Subscription in memory:
//! every member [`InstanceMetadata`] lists, each with the target value it
//! gives there. An expanded document writes each member as a bare value node
//! beside the sub-attributes. A row and an element hold them as typed fields
//! instead, so a path ending on one compares the field of the instance it
//! hangs off rather than unnesting a further level.

use crate::{
    operator::{PatternIndexes, compare, does_not_hold, pattern as pattern_test},
    q_predicate::{instance_row::InstanceRow, string_ordering::ordered_string},
};
use andromeda_ngsi_ld_query::{
    filter::{
        predicate::{CmpOp, PatternPolarity, Predicate, SetMembership},
        scalar::{CompareValue, Scalar},
        string_order::string_order,
    },
    pattern::Pattern,
    queried_entity::instance_metadata::InstanceMetadata,
};
use andromeda_postgresql_codes::stored_unit_code::stored_unit_code;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    name_dictionary::EntityTypeNames,
    temporal::AttributeHistory,
};
use andromeda_postgresql_seaquery::{
    literal::{false_literal, int_literal},
    pg_array::{any, array_element, array_subscript, cardinality, is_any_of, is_none_of},
    scalar_subquery::scalar_subquery,
};
use cefact_units::UnitCode;
use sea_query::{Alias, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Query, SimpleExpr};
use std::slice;

/// The alias the dictionary row naming one type is read under.
const NAMED_TYPE: &str = "mt";

/// Whether `member` of the instance `holder` satisfies `pred`.
///
/// The elements of a history row record no `createdAt` or `modifiedAt`
/// (clause 5.2.5 system times a sub-attribute is stored without), so a path
/// ending on either below the top level matches nothing in history.
#[must_use]
pub fn member_condition(member: InstanceMetadata, holder: InstanceRow, pred: &Predicate) -> Condition {
    StoredMember::of(member, holder).condition(pred)
}

/// How a stored instance holds one member, as the expression reading it.
enum StoredMember {
    /// A clause 4.8 or clause 4.22 instant, a `DateTime` target value.
    Instant(SimpleExpr),
    /// The `cefact-units` identity of a `unitCode`, a String (clause 4.5.2)
    /// compared as the text of the code.
    UnitCode(SimpleExpr),
    /// One String: a `datasetId`, or the expanded IRI of a `valueType`.
    Text(SimpleExpr),
    /// The dictionary keys of the types an `objectType` lists: one type is a
    /// String target value, several the array clause 4.9 compares by
    /// membership.
    TypeKeys(SimpleExpr),
    /// A member no stored instance holds: `instanceId`, which table 5.2.5-2
    /// has only in the temporal representation.
    Unheld,
}

impl StoredMember {
    /// How the instance `holder` holds `member`.
    fn of(member: InstanceMetadata, holder: InstanceRow) -> StoredMember {
        match member {
            InstanceMetadata::ObservedAt => {
                StoredMember::Instant(holder.field(CurrentAttributes::ObservedAt, AttributeHistory::ObservedAt, SubAttribute::ObservedAt))
            }
            InstanceMetadata::CreatedAt => {
                StoredMember::Instant(holder.field(CurrentAttributes::CreatedAt, AttributeHistory::CreatedAt, SubAttribute::CreatedAt))
            }
            InstanceMetadata::ModifiedAt => {
                StoredMember::Instant(holder.field(CurrentAttributes::ModifiedAt, AttributeHistory::ModifiedAt, SubAttribute::ModifiedAt))
            }
            InstanceMetadata::ExpiresAt => {
                StoredMember::Instant(holder.field(CurrentAttributes::ExpiresAt, AttributeHistory::ExpiresAt, SubAttribute::ExpiresAt))
            }
            InstanceMetadata::UnitCode => {
                StoredMember::UnitCode(holder.field(CurrentAttributes::UnitCodeId, AttributeHistory::UnitCodeId, SubAttribute::UnitCodeId))
            }
            InstanceMetadata::DatasetId => StoredMember::Text(holder.dataset_id()),
            InstanceMetadata::ValueType => StoredMember::Text(type_name(holder.value_type_key())),
            InstanceMetadata::ObjectType => StoredMember::TypeKeys(holder.field(
                CurrentAttributes::ObjectTypeKeys,
                AttributeHistory::ObjectTypeKeys,
                SubAttribute::ObjectTypeKeys,
            )),
            InstanceMetadata::InstanceId => StoredMember::Unheld,
        }
    }

    /// Whether the member satisfies `pred`.
    ///
    /// An operand of a kind the member is not compares with no member: clause
    /// 4.9 makes it non-matching and, under `unequal`, unequal. A stored
    /// instant keeps no written text, so a pattern matches none of them.
    fn condition(&self, pred: &Predicate) -> Condition {
        if matches!(self, StoredMember::Unheld) {
            return Condition::all().add(false_literal());
        }
        match pred {
            Predicate::Exists(_) => Condition::all().add(self.present()),
            Predicate::Equality { membership, rhs, .. } => {
                let equal = self.equal(rhs);
                match membership {
                    SetMembership::Within => equal,
                    SetMembership::Outside => Condition::all().add(self.present()).add(does_not_hold(equal)),
                }
            }
            Predicate::Ordering { op, rhs, .. } => Condition::all().add(self.compared(op.as_cmp_op(), rhs)),
            Predicate::MatchPattern { pattern, .. } => Condition::all().add(self.matched(pattern, PatternPolarity::Match)),
            Predicate::NotMatchPattern { pattern, .. } => Condition::all().add(self.matched(pattern, PatternPolarity::NotMatch)),
        }
    }

    /// Whether the instance holds the member.
    fn present(&self) -> SimpleExpr {
        match self {
            StoredMember::Instant(field) | StoredMember::UnitCode(field) | StoredMember::Text(field) | StoredMember::TypeKeys(field) => {
                field.clone().is_not_null() // clone: the field is read by every comparison of the term
            }
            StoredMember::Unheld => false_literal(),
        }
    }

    /// Whether the member equals the operand of an equality, within its
    /// range, or one of its values.
    ///
    /// Clause 4.9: an operand "Is included in the target value, if the latter
    /// is an array", so an `objectType` is equal to any type it lists.
    fn equal(&self, rhs: &CompareValue) -> Condition {
        match (self, rhs) {
            (StoredMember::TypeKeys(keys), CompareValue::Scalar(scalar)) => Condition::all().add(some_type_named(keys, slice::from_ref(scalar))),
            (StoredMember::TypeKeys(keys), CompareValue::ValueList(values)) => Condition::all().add(some_type_named(keys, values)),
            (StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::Unheld, CompareValue::Scalar(scalar)) => {
                Condition::all().add(self.compared(CmpOp::Equal, scalar))
            }
            (StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::Unheld, CompareValue::ValueList(values)) => {
                values.iter().fold(Condition::any(), |any, value| any.add(self.compared(CmpOp::Equal, value)))
            }
            (
                StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::TypeKeys(_) | StoredMember::Unheld,
                CompareValue::Range { min, max },
            ) => Condition::all()
                .add(self.compared(CmpOp::GreaterEq, min))
                .add(self.compared(CmpOp::LessEq, max)),
        }
    }

    /// One comparison of the member with an operand, constant `FALSE` when the
    /// operand is of another datatype.
    ///
    /// An `objectType` listing several types is an array, which no ordering
    /// holds for, so only a single type compares.
    fn compared(&self, op: CmpOp, scalar: &Scalar) -> SimpleExpr {
        match (self, scalar) {
            (StoredMember::Instant(field), Scalar::DateTime(instant)) => compare(field.clone(), op, SeaExpr::val(*instant)),
            (StoredMember::UnitCode(field), Scalar::Str(operand)) => listed_code_where(field, |code| op.holds(string_order(code, operand, op))),
            (StoredMember::Text(text), Scalar::Str(operand) | Scalar::Uri(operand)) => text_compared(text.clone(), op, scalar, operand),
            (StoredMember::TypeKeys(keys), Scalar::Str(operand) | Scalar::Uri(operand)) => text_compared(single_type_name(keys), op, scalar, operand),
            (StoredMember::Instant(_), Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::Date(_) | Scalar::Time(_))
            | (StoredMember::UnitCode(_), Scalar::Number(_) | Scalar::Bool(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_))
            | (
                StoredMember::Text(_) | StoredMember::TypeKeys(_),
                Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_),
            )
            | (
                StoredMember::Unheld,
                Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_),
            ) => false_literal(),
        }
    }

    /// A pattern term of `polarity` on the member.
    ///
    /// Clause 4.9 tests a pattern against a String only, so an instant and an
    /// `objectType` listing several types match no pattern either way.
    fn matched(&self, pattern: &Pattern, polarity: PatternPolarity) -> SimpleExpr {
        match self {
            StoredMember::UnitCode(field) => listed_code_where(field, |code| match polarity {
                PatternPolarity::Match => pattern.is_match(code),
                PatternPolarity::NotMatch => !pattern.is_match(code),
            }),
            StoredMember::Text(text) => pattern_test(text.clone(), polarity, pattern, PatternIndexes::None),
            StoredMember::TypeKeys(keys) => pattern_test(single_type_name(keys), polarity, pattern, PatternIndexes::None),
            StoredMember::Instant(_) | StoredMember::Unheld => false_literal(),
        }
    }
}

/// `text` compared with the String or `URI` operand `operand` spells, a String
/// ordering collating as a stored String value's does.
fn text_compared(text: SimpleExpr, op: CmpOp, scalar: &Scalar, operand: &str) -> SimpleExpr {
    compare(ordered_string(text, scalar, op), op, SeaExpr::val(operand))
}

/// The expanded IRI the `entity_type_names` key `key` names, NULL for a NULL
/// key: the String a `valueType` held as `key` is.
#[must_use]
pub fn type_name(key: SimpleExpr) -> SimpleExpr {
    let alias = Alias::new(NAMED_TYPE);
    scalar_subquery(
        Query::select()
            .column((alias.clone(), EntityTypeNames::Name))
            .from_as(EntityTypeNames::Table, alias.clone())
            .and_where(SeaExpr::col((alias, EntityTypeNames::EntityTypeNameKey)).eq(key))
            .to_owned(),
    )
}

/// The expanded IRI of the one type `keys` lists, NULL when it lists none or
/// several: the String an `objectType` held as `keys` is, when it is one.
#[must_use]
pub fn single_type_name(keys: &SimpleExpr) -> SimpleExpr {
    // clone: the keys are read by the count and by the subscript
    CaseStatement::new()
        .case(cardinality(keys.clone()).eq(int_literal(1)), type_name(array_element(keys.clone(), 1)))
        .into()
}

/// Whether some type `keys` lists is spelled by one of `operands`; a number,
/// a boolean or a time spells no type.
fn some_type_named(keys: &SimpleExpr, operands: &[Scalar]) -> SimpleExpr {
    // clone: the bound array owns its elements
    let spellings: Vec<String> = operands
        .iter()
        .filter_map(|scalar| match scalar {
            Scalar::Str(operand) | Scalar::Uri(operand) => Some(operand.clone()),
            Scalar::Number(_) | Scalar::Bool(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_) => None,
        })
        .collect();
    if spellings.is_empty() {
        return false_literal();
    }
    let alias = Alias::new(NAMED_TYPE);
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_as(EntityTypeNames::Table, alias.clone())
        .and_where(SeaExpr::col((alias.clone(), EntityTypeNames::EntityTypeNameKey)).eq(any(keys.clone()))) // clone: sea-query owns every operand
        .and_where(is_any_of(SeaExpr::col((alias, EntityTypeNames::Name)), spellings));
    SeaExpr::exists(scan)
}

/// Whether `field`, the stored identity of a `unitCode`, names a listed code
/// whose text `holds` accepts.
///
/// The column keeps the identity of the code rather than its text, so the
/// term is decided over the text of every code the library lists, under the
/// comparison and the pattern engine an in-memory evaluation applies, and the
/// column is tested against the identities that pass, or against those that
/// fail when they are fewer. An absent field matches neither way.
fn listed_code_where(field: &SimpleExpr, holds: impl Fn(&str) -> bool) -> SimpleExpr {
    let (passing, failing): (Vec<UnitCode>, Vec<UnitCode>) = UnitCode::ALL.iter().partition(|code| holds(code.as_str()));
    let field = field.clone(); // clone: the field is read by every comparison of the term
    if failing.is_empty() {
        field.is_not_null()
    } else if passing.len() <= failing.len() {
        is_any_of(field, passing.into_iter().map(stored_unit_code))
    } else {
        is_none_of(field, failing.into_iter().map(stored_unit_code))
    }
}

/// The text of the code `field`, the stored identity of a `unitCode`, names:
/// `NULL` for an absent field.
///
/// The library numbers its codes densely from 1 in the order it lists them, so
/// the list of their texts, bound as one array, holds each code's text at its
/// identity. No stored row carries an identity this build does not list (a
/// read of one fails to decode), and an identity past the end of the array
/// reads `NULL`.
#[must_use]
pub fn unit_code_text(field: SimpleExpr) -> SimpleExpr {
    let texts: Vec<String> = UnitCode::ALL.iter().map(|code| code.as_str().to_owned()).collect();
    array_subscript(SeaExpr::val(texts), field)
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::{ElementLevel, InstanceRow},
        sub_attribute::member_field::{member_condition, unit_code_text},
    };
    use andromeda_ngsi_ld_lexicon::unit_code_resolution::resolve_unit_code;
    use andromeda_ngsi_ld_query::{
        filter::{expression::Expr, parser::parse},
        queried_entity::instance_metadata::InstanceMetadata,
    };
    use andromeda_postgresql_codes::stored_unit_code::stored_unit_code;
    use andromeda_postgresql_schema::{
        current::{CurrentAttributes, SubAttribute},
        temporal::AttributeHistory,
    };
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, Value};

    /// The SQL of the `q` term `"https://example.com/t".<member><term>` over
    /// `holder`.
    fn member_sql(member: InstanceMetadata, term: &str, holder: InstanceRow) -> String {
        let Expr::Pred(pred) = parse(&format!("\"https://example.com/t\".\"{}\"{term}", member.iri().as_str())).expect("parse q") else {
            panic!("a single predicate");
        };
        Query::select()
            .cond_where(member_condition(member, holder, &pred))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn an_instant_member_compares_its_field_with_a_date_time() {
        let sql = member_sql(InstanceMetadata::ObservedAt, ">2024-01-01T00:00:00Z", InstanceRow::Element(ElementLevel::at(1)));
        assert!(sql.contains(r#""l1"."observed_at" > '2024-01-01 00:00:00.000000 +00:00'"#), "{sql}");
    }

    /// Clause 4.5.5: the default instance has no `datasetId`, which a row
    /// keys by the empty string and an element records as NULL.
    #[test]
    fn a_dataset_id_reads_the_grain_key_of_a_row_and_the_field_of_an_element() {
        let operand = "==\"urn:ngsi-ld:Dataset:d1\"";
        let current = member_sql(InstanceMetadata::DatasetId, operand, InstanceRow::Current);
        assert!(current.contains(r#"nullif("ca"."dataset_key", '') = 'urn:ngsi-ld:Dataset:d1'"#), "{current}");
        let history = member_sql(InstanceMetadata::DatasetId, operand, InstanceRow::History);
        assert!(history.contains(r#"nullif("qh"."dataset_key", '') = 'urn:ngsi-ld:Dataset:d1'"#), "{history}");
        let element = member_sql(InstanceMetadata::DatasetId, operand, InstanceRow::Element(ElementLevel::at(0)));
        assert!(element.contains(r#""l0"."dataset_id" = 'urn:ngsi-ld:Dataset:d1'"#), "{element}");
    }

    /// Clause 4.9 `unequal` asks for the target element, so the default
    /// instance, which has no `datasetId`, is no match.
    #[test]
    fn an_unequal_dataset_id_requires_one() {
        let sql = member_sql(InstanceMetadata::DatasetId, "!=\"urn:ngsi-ld:Dataset:d1\"", InstanceRow::Current);
        assert!(sql.contains(r#"nullif("ca"."dataset_key", '') IS NOT NULL"#), "{sql}");
    }

    /// A `valueType` is held as a dictionary key and compared as the expanded
    /// IRI the key names.
    #[test]
    fn a_value_type_compares_the_iri_its_key_names() {
        let sql = member_sql(InstanceMetadata::ValueType, "==\"https://example.com/String\"", InstanceRow::Current);
        assert!(
            sql.contains(r#"(SELECT "mt"."name" FROM "entity_type_names" AS "mt" WHERE "mt"."entity_type_name_key" = "ca"."value_type_key") = 'https://example.com/String'"#),
            "{sql}"
        );
    }

    /// Clause 4.9: an operand "Is included in the target value, if the latter
    /// is an array", so an `objectType` equals any type it lists.
    #[test]
    fn an_object_type_equality_tests_every_listed_type() {
        let sql = member_sql(
            InstanceMetadata::ObjectType,
            "==\"https://example.com/Device\",\"https://example.com/Gauge\"",
            InstanceRow::Current,
        );
        assert!(
            sql.contains(
                r#"EXISTS(SELECT 1 FROM "entity_type_names" AS "mt" WHERE "mt"."entity_type_name_key" = ANY("ca"."object_type_keys") AND "mt"."name" = ANY("#
            ),
            "{sql}"
        );
        let unequal = member_sql(InstanceMetadata::ObjectType, "!=\"https://example.com/Device\"", InstanceRow::Current);
        assert!(
            unequal.contains(r#""ca"."object_type_keys" IS NOT NULL"#) && unequal.contains("NOT"),
            "{unequal}"
        );
    }

    /// An array is no String, so an ordering or a pattern reads an
    /// `objectType` only when it lists one type.
    #[test]
    fn an_object_type_ordering_or_pattern_reads_a_single_type() {
        for term in [">\"https://example.com/A\"", "~=\"Dev\"", "!~=\"Dev\""] {
            let sql = member_sql(InstanceMetadata::ObjectType, term, InstanceRow::Current);
            assert!(sql.contains(r#"CASE WHEN (cardinality("ca"."object_type_keys") = (1)) THEN"#), "{term}: {sql}");
        }
    }

    /// Table 5.2.5-2 has `instanceId` only in the temporal representation, so
    /// no stored instance holds one, and no term on it matches either way.
    #[test]
    fn an_instance_id_term_matches_nothing() {
        for term in ["", "==\"urn:ngsi-ld:Instance:1\"", "!=\"urn:ngsi-ld:Instance:1\""] {
            let sql = member_sql(InstanceMetadata::InstanceId, term, InstanceRow::Current);
            assert!(sql.ends_with("WHERE FALSE"), "{term}: {sql}");
        }
    }

    /// The SQL and the bound identities of `q`
    /// `"https://example.com/t"."unitCode"<term>` over a current row.
    fn unit_code_condition(term: &str) -> (String, Vec<i16>) {
        let Expr::Pred(pred) = parse(&format!("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/unitCode\"{term}")).expect("parse q") else {
            panic!("a single predicate");
        };
        let (sql, values) = Query::select()
            .cond_where(member_condition(InstanceMetadata::UnitCode, InstanceRow::Current, &pred))
            .to_owned()
            .build(PostgresQueryBuilder);
        let mut bound: Vec<i16> = values
            .0
            .into_iter()
            .flat_map(|value| {
                if let Value::SmallInt(Some(identity)) = value {
                    return vec![identity];
                }
                let Value::Array(_, Some(elements)) = &value else {
                    panic!("not a stored identity or an identity array: {value:?}");
                };
                elements
                    .iter()
                    .map(|element| {
                        let Value::SmallInt(Some(identity)) = element else {
                            panic!("not a stored identity: {element:?}");
                        };
                        *identity
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        bound.sort_unstable();
        (sql, bound)
    }

    /// The stored identities of `codes`, in the order [`unit_code_condition`]
    /// returns them.
    fn identities(codes: &[&str]) -> Vec<i16> {
        let mut identities: Vec<i16> = codes
            .iter()
            .map(|code| stored_unit_code(resolve_unit_code(code).expect("a listed code")))
            .collect();
        identities.sort_unstable();
        identities
    }

    #[test]
    fn a_unit_code_equality_compares_the_stored_identity_of_the_code() {
        let (sql, bound) = unit_code_condition("==\"CEL\"");
        assert!(sql.contains(r#""ca"."unit_code_id" = $1"#), "{sql}");
        assert_eq!(bound, [367]);
    }

    /// Every listed code is a valid `unitCode`, whatever its status.
    #[test]
    fn a_deleted_unit_code_is_a_valid_q_operand() {
        assert_eq!(unit_code_condition("==\"KTM\"").1, identities(&["KTM"]));
    }

    /// Clause 4.9: a String naming no listed code equals no `unitCode` and is
    /// unequal to every one.
    #[test]
    fn an_operand_naming_no_listed_code_equals_none_and_is_unequal_to_each() {
        let (equal, bound) = unit_code_condition("==\"NOTACODE\"");
        assert!(equal.ends_with("WHERE FALSE"), "{equal}");
        assert_eq!(bound, Vec::<i16>::new());
        let (unequal, bound) = unit_code_condition("!=\"NOTACODE\"");
        assert!(unequal.contains(r#""ca"."unit_code_id" IS NOT NULL"#), "{unequal}");
        assert_eq!(bound, Vec::<i16>::new());
    }

    /// Clause 4.5.2 makes `unitCode` a String, so an ordering compares the
    /// text of the code, and the column is tested against the shorter list.
    #[test]
    fn a_unit_code_ordering_compares_the_text_of_the_code() {
        let (at_least, bound) = unit_code_condition(">=\"ZP\"");
        assert!(at_least.contains(r#""ca"."unit_code_id" = ANY($1)"#), "{at_least}");
        assert_eq!(bound, identities(&["ZP", "ZZ"]));
        let (below, bound) = unit_code_condition("<\"ZP\"");
        assert!(below.contains(r#""ca"."unit_code_id" <> ALL($1)"#), "{below}");
        assert_eq!(bound, identities(&["ZP", "ZZ"]));
    }

    /// Clause 4.9 recommends the UCA for a String ordering: `z` sorts before
    /// every code that begins with `Z`, where code point order puts it after
    /// all of them.
    #[test]
    fn a_unit_code_ordering_collates_under_the_icu_root_collation() {
        assert_eq!(
            unit_code_condition(">\"z\"").1,
            identities(&["Z1", "Z11", "Z2", "Z3", "Z4", "Z5", "Z6", "Z8", "Z9", "ZP", "ZZ"])
        );
    }

    #[test]
    fn a_unit_code_pattern_matches_the_text_of_the_code() {
        let (matching, bound) = unit_code_condition("~=\"^CE\"");
        assert!(matching.contains(r#""ca"."unit_code_id" = ANY($1)"#), "{matching}");
        assert_eq!(bound, identities(&["CEL", "CEN"]));
        let (not_matching, bound) = unit_code_condition("!~=\"^CE\"");
        assert!(not_matching.contains(r#""ca"."unit_code_id" <> ALL($1)"#), "{not_matching}");
        assert_eq!(bound, identities(&["CEL", "CEN"]));
    }

    /// Clause 4.9 `patternOp` and `notPatternOp` both require the target to be
    /// present, so a term every code satisfies still asks for one.
    #[test]
    fn a_pattern_every_code_matches_still_requires_a_unit_code() {
        let (sql, bound) = unit_code_condition("~=\".\"");
        assert!(sql.contains(r#""ca"."unit_code_id" IS NOT NULL"#), "{sql}");
        assert_eq!(bound, Vec::<i16>::new());
    }

    /// The bound list holds each code's text at the 1-based position its
    /// stored identity names.
    #[test]
    fn a_unit_code_text_is_read_at_the_position_of_its_stored_identity() {
        let (sql, values) = Query::select()
            .expr(unit_code_text(InstanceRow::Current.field(
                CurrentAttributes::UnitCodeId,
                AttributeHistory::UnitCodeId,
                SubAttribute::UnitCodeId,
            )))
            .to_owned()
            .build(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT ( $1 )[ "ca"."unit_code_id" ]"#);
        let [Value::Array(_, Some(texts))] = values.0.as_slice() else {
            panic!("one bound text array: {values:?}");
        };
        for code in UnitCode::ALL {
            let position = usize::try_from(stored_unit_code(*code)).expect("a positive identity") - 1;
            assert_eq!(texts.get(position), Some(&Value::String(Some(code.as_str().to_owned()))), "{code}");
        }
        assert_eq!(texts.len(), UnitCode::ALL.len());
    }

    /// A stored instant keeps no written text to match.
    #[test]
    fn an_instant_member_matches_no_pattern() {
        let sql = member_sql(InstanceMetadata::ObservedAt, "~=\"2024\"", InstanceRow::Current);
        assert!(sql.ends_with("WHERE FALSE"), "{sql}");
    }
}
