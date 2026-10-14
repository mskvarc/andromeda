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
    q_predicate::{history_window::HistoryAliases, instance_row::InstanceRow, projection_probe::projection_column},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, unit_code_resolution::resolve_unit_code};
use andromeda_ngsi_ld_query::filter::{
    predicate::{CmpOp, Predicate, SetMembership},
    scalar::{CompareValue, Scalar},
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{CREATED_AT, EXPIRES_AT, MODIFIED_AT, OBSERVED_AT, UNIT_CODE};
use andromeda_postgresql_codes::stored_unit_code::stored_unit_code;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, SubAttribute},
    temporal::AttributeHistory,
};
use andromeda_postgresql_seaquery::literal::false_literal;
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
    /// The four instants compare with a `DateTime` operand and `unitCode` with a
    /// String naming a catalogued code; an operand of any other kind is of
    /// another datatype, which clause 4.9 makes non-matching and, under
    /// `unequal`, unequal. A stored instant keeps no written text, so a pattern
    /// matches none of them.
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
            Predicate::MatchPattern { .. } | Predicate::NotMatchPattern { .. } => Condition::all().add(false_literal()),
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
            (MemberField::UnitCode, Scalar::Str(code)) => match (op, resolve_unit_code(code)) {
                (CmpOp::Equal | CmpOp::Unequal, Ok(unit_code)) => compare(field.clone(), op, SeaExpr::val(stored_unit_code(unit_code))),
                (CmpOp::Equal | CmpOp::Unequal | CmpOp::Greater | CmpOp::GreaterEq | CmpOp::Less | CmpOp::LessEq, Ok(_) | Err(_)) => false_literal(),
            },
            (
                MemberField::ObservedAt | MemberField::CreatedAt | MemberField::ModifiedAt | MemberField::ExpiresAt | MemberField::UnitCode,
                Scalar::Number(_) | Scalar::Bool(_) | Scalar::Str(_) | Scalar::Uri(_) | Scalar::DateTime(_) | Scalar::Date(_) | Scalar::Time(_),
            ) => false_literal(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::{ElementLevel, InstanceRow},
        sub_attribute::member_field::MemberField,
    };
    use andromeda_ngsi_ld_lexicon::unit_code_resolution::resolve_unit_code;
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse};
    use andromeda_ngsi_ld_vocabulary::expanded::properties::OBSERVED_AT;
    use andromeda_postgresql_codes::stored_unit_code::stored_unit_code;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

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
        let Expr::Pred(pred) = parse("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/observedAt\">\"2024-01-01T00:00:00Z\"").expect("parse q") else {
            panic!("a single predicate");
        };
        let field = MemberField::ObservedAt.of_instance(InstanceRow::Element(ElementLevel::at(1)));
        let sql = Query::select()
            .cond_where(MemberField::ObservedAt.condition(&field, &pred))
            .to_owned()
            .to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""l1"."observed_at" > '2024-01-01 00:00:00.000000 +00:00'"#), "{sql}");
    }

    /// The SQL of `q` `"https://example.com/t"."unitCode"==<code>` over a
    /// current row.
    fn unit_code_condition(code: &str) -> String {
        let Expr::Pred(pred) = parse(&format!("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/unitCode\"==\"{code}\"")).expect("parse q") else {
            panic!("a single predicate");
        };
        let field = MemberField::UnitCode.of_instance(InstanceRow::Current);
        Query::select()
            .cond_where(MemberField::UnitCode.condition(&field, &pred))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_unit_code_compares_the_stored_identity_of_the_code() {
        let sql = unit_code_condition("CEL");
        assert!(sql.contains(r#""ca"."unit_code_id" = 367"#), "{sql}");
    }

    /// Every listed code is a valid `unitCode`, whatever its status.
    #[test]
    fn a_deleted_unit_code_is_a_valid_q_operand() {
        let deleted = stored_unit_code(resolve_unit_code("KTM").expect("a listed code"));
        let sql = unit_code_condition("KTM");
        assert!(sql.contains(&format!(r#""ca"."unit_code_id" = {deleted}"#)), "{sql}");
    }
}
