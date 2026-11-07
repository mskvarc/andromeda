//! The members of an attribute instance a `q` path names as if they were
//! sub-attributes.
//!
//! Clause 4.9 lets a path end on `observedAt`, `unitCode` or a clause 4.8
//! timestamp of the attribute (or sub-attribute) before it: an expanded
//! document writes each as a bare value node beside the sub-attributes. A row
//! and an element hold them as typed fields instead, so a path ending on one
//! compares the field of the instance it hangs off rather than unnesting a
//! further level.

use crate::{
    operator::{compare, does_not_hold},
    q_predicate::{history_aliases::HistoryAliases, instance_row::InstanceRow, projection_probe::projection_column},
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{
    predicate::{CmpOp, Predicate, SetMembership},
    scalar::{CompareValue, Scalar},
    string_order::string_order,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{CREATED_AT, EXPIRES_AT, MODIFIED_AT, OBSERVED_AT, UNIT_CODE};
use andromeda_postgresql_codes::stored_unit_code::stored_unit_code;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    temporal::AttributeHistory,
};
use andromeda_postgresql_seaquery::{
    literal::false_literal,
    pg_array::{array_subscript, is_any_of, is_none_of},
};
use cefact_units::UnitCode;
use sea_query::{Condition, Expr as SeaExpr, ExprTrait, SimpleExpr};

/// A member of an instance a path can end on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberField {
    /// Clause 4.5.2 `observedAt`.
    ObservedAt,
    /// Clause 4.5.2 `unitCode`, held as the `cefact-units` identity of the code.
    UnitCode,
    /// Clause 4.8 `createdAt`.
    CreatedAt,
    /// Clause 4.8 `modifiedAt`.
    ModifiedAt,
    /// Clause 4.22 `expiresAt`.
    ExpiresAt,
}

impl MemberField {
    /// The member an expanded name denotes, or `None` for the name of a
    /// sub-attribute.
    #[must_use]
    pub fn of(name: &AttributeName) -> Option<MemberField> {
        let name = name.as_str();
        [
            (OBSERVED_AT.as_str(), MemberField::ObservedAt),
            (UNIT_CODE.as_str(), MemberField::UnitCode),
            (CREATED_AT.as_str(), MemberField::CreatedAt),
            (MODIFIED_AT.as_str(), MemberField::ModifiedAt),
            (EXPIRES_AT.as_str(), MemberField::ExpiresAt),
        ]
        .into_iter()
        .find_map(|(iri, field)| (iri == name).then_some(field))
    }

    /// The field of `holder`, a current row, a history row or an element.
    ///
    /// The elements of a history row record no `createdAt` or `modifiedAt`
    /// (clause 5.2.5 system times a sub-attribute is stored without), so a path
    /// ending on either below the top level matches nothing in history.
    #[must_use]
    pub fn of_instance(self, holder: InstanceRow) -> SimpleExpr {
        let (current, history, element) = match self {
            MemberField::ObservedAt => (CurrentAttributes::ObservedAt, AttributeHistory::ObservedAt, SubAttribute::ObservedAt),
            MemberField::UnitCode => (CurrentAttributes::UnitCodeId, AttributeHistory::UnitCodeId, SubAttribute::UnitCodeId),
            MemberField::CreatedAt => (CurrentAttributes::CreatedAt, AttributeHistory::CreatedAt, SubAttribute::CreatedAt),
            MemberField::ModifiedAt => (CurrentAttributes::ModifiedAt, AttributeHistory::ModifiedAt, SubAttribute::ModifiedAt),
            MemberField::ExpiresAt => (CurrentAttributes::ExpiresAt, AttributeHistory::ExpiresAt, SubAttribute::ExpiresAt),
        };
        match holder {
            InstanceRow::Current => projection_column(current),
            InstanceRow::History => HistoryAliases::top_level().instance_column(history),
            InstanceRow::Element(level) => level.field(element),
        }
    }

    /// Whether the member of `field` satisfies `pred`.
    ///
    /// The four instants compare with a `DateTime` operand and `unitCode`, a
    /// String (clause 4.5.2), with a String operand and a pattern; an operand
    /// of any other kind is of another datatype, which clause 4.9 makes
    /// non-matching and, under `unequal`, unequal. A stored instant keeps no
    /// written text, so a pattern matches none of them.
    #[must_use]
    pub fn condition(self, field: &SimpleExpr, pred: &Predicate) -> Condition {
        let present = || field.clone().is_not_null(); // clone: the field is read by every comparison
        match pred {
            Predicate::Exists(_) => Condition::all().add(present()),
            Predicate::Equality { membership, rhs, .. } => {
                let equal = match rhs {
                    CompareValue::Scalar(scalar) => Condition::all().add(self.compared(field, CmpOp::Equal, scalar)),
                    CompareValue::ValueList(values) => values
                        .iter()
                        .fold(Condition::any(), |any, value| any.add(self.compared(field, CmpOp::Equal, value))),
                    CompareValue::Range { min, max } => {
                        Condition::all()
                            .add(self.compared(field, CmpOp::GreaterEq, min))
                            .add(self.compared(field, CmpOp::LessEq, max))
                    }
                };
                match membership {
                    SetMembership::Within => equal,
                    SetMembership::Outside => Condition::all().add(present()).add(does_not_hold(equal)),
                }
            }
            Predicate::Ordering { op, rhs, .. } => Condition::all().add(self.compared(field, op.as_cmp_op(), rhs)),
            Predicate::MatchPattern { pattern, .. } => Condition::all().add(self.matched(field, |text| pattern.is_match(text))),
            Predicate::NotMatchPattern { pattern, .. } => Condition::all().add(self.matched(field, |text| !pattern.is_match(text))),
        }
    }

    /// A pattern term on the field, `holds` deciding the written text it is
    /// tested against.
    fn matched(self, field: &SimpleExpr, holds: impl Fn(&str) -> bool) -> SimpleExpr {
        match self {
            MemberField::UnitCode => listed_code_where(field, holds),
            MemberField::ObservedAt | MemberField::CreatedAt | MemberField::ModifiedAt | MemberField::ExpiresAt => false_literal(),
        }
    }

    /// One comparison of the field with an operand, constant `FALSE` when the
    /// operand is of another datatype.
    fn compared(self, field: &SimpleExpr, op: CmpOp, scalar: &Scalar) -> SimpleExpr {
        // clone: the field is compared with every operand of the term
        match (self, scalar) {
            (MemberField::ObservedAt | MemberField::CreatedAt | MemberField::ModifiedAt | MemberField::ExpiresAt, Scalar::DateTime(instant)) => {
                compare(field.clone(), op, SeaExpr::val(*instant))
            }
            (MemberField::UnitCode, Scalar::Str(operand)) => listed_code_where(field, |code| op.holds(string_order(code, operand, op))),
            (
                MemberField::ObservedAt | MemberField::CreatedAt | MemberField::ModifiedAt | MemberField::ExpiresAt | MemberField::UnitCode,
                Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_),
            ) => false_literal(),
        }
    }
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
        sub_attribute::member_field::{MemberField, unit_code_text},
    };
    use andromeda_ngsi_ld_lexicon::unit_code_resolution::resolve_unit_code;
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use andromeda_ngsi_ld_vocabulary::expanded::properties::OBSERVED_AT;
    use andromeda_postgresql_codes::stored_unit_code::stored_unit_code;
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, Value};

    #[test]
    fn a_member_name_is_recognised_by_its_expanded_iri() {
        assert_eq!(
            MemberField::of(&OBSERVED_AT.as_str().parse().expect("valid name")),
            Some(MemberField::ObservedAt)
        );
        assert_eq!(MemberField::of(&"https://example.com/accuracy".parse().expect("valid name")), None);
    }

    #[test]
    fn an_instant_member_compares_its_field_with_a_date_time() {
        let Expr::Pred(pred) = parse("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/observedAt\">2024-01-01T00:00:00Z").expect("parse q") else {
            panic!("a single predicate");
        };
        let field = MemberField::ObservedAt.of_instance(InstanceRow::Element(ElementLevel::at(1)));
        let sql = Query::select()
            .cond_where(MemberField::ObservedAt.condition(&field, &pred))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""l1"."observed_at" > '2024-01-01 00:00:00.000000 +00:00'"#), "{sql}");
    }

    /// The SQL and the bound identities of `q`
    /// `"https://example.com/t"."unitCode"<term>` over a current row.
    fn unit_code_condition(term: &str) -> (String, Vec<i16>) {
        let Expr::Pred(pred) = parse(&format!("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/unitCode\"{term}")).expect("parse q") else {
            panic!("a single predicate");
        };
        let field = MemberField::UnitCode.of_instance(InstanceRow::Current);
        let (sql, values) = Query::select()
            .cond_where(MemberField::UnitCode.condition(&field, &pred))
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
            .expr(unit_code_text(MemberField::UnitCode.of_instance(InstanceRow::Current)))
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
        let Expr::Pred(pred) = parse("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/observedAt\"~=\"2024\"").expect("parse q") else {
            panic!("a single predicate");
        };
        let field = MemberField::ObservedAt.of_instance(InstanceRow::Current);
        let sql = Query::select()
            .cond_where(MemberField::ObservedAt.condition(&field, &pred))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.ends_with("WHERE FALSE"), "{sql}");
    }
}
