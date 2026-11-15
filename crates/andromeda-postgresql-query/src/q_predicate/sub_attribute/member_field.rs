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
    operator::{PatternIndexes, compare, pattern as pattern_test},
    q_predicate::{
        instance_row::InstanceRow,
        string_ordering::ordered_string,
        typed_column::bound_operand,
        verdict_condition::{ComplementTarget, verdict_condition},
    },
};
use andromeda_ngsi_ld_query::{
    filter::{
        instance_metadata::InstanceMetadata,
        reading::{
            comparator::Comparator,
            operand::Operand,
            order_relation::OrderRelation,
            scalar_test::ScalarTest,
            target_kind::TargetKind,
            term_reading::Test,
            value_test::ComplementScope,
        },
    },
    pattern::Pattern,
};
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
use andromeda_postgresql_stored_values::stored_unit_code::stored_unit_code;
use cefact_units::UnitCode;
use sea_query::{Alias, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Query, SimpleExpr};

/// The alias the dictionary row naming one type is read under.
const NAMED_TYPE: &str = "mt";

/// Whether `member` of the instance `holder` satisfies `test`.
///
/// The elements of a history row record no `createdAt` or `modifiedAt`
/// (clause 5.2.5 system times a sub-attribute is stored without), so a path
/// ending on either below the top level matches nothing in history.
#[must_use]
pub fn member_condition(member: InstanceMetadata, holder: InstanceRow, test: &Test) -> Condition {
    StoredMember::of(member, holder).condition(test)
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

    /// The target kind the member is: an instant a `DateTime`, a `unitCode`,
    /// a `datasetId` and a `valueType` a String, an `objectType` a String,
    /// which one type is and each of several is; `None` for a member no stored
    /// instance holds.
    const fn target_kind(&self) -> Option<TargetKind> {
        match self {
            StoredMember::Instant(_) => Some(TargetKind::DateTime),
            StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::TypeKeys(_) => Some(TargetKind::String),
            StoredMember::Unheld => None,
        }
    }

    /// Whether the member satisfies `test`.
    ///
    /// An operand of a kind the member is not compares with no member: clause
    /// 4.9 makes it non-matching and, under `unequal`, unequal. A complement
    /// of `!=` ranges over a member the instance holds, and one of `!~=` over
    /// a member that is one String.
    fn condition(&self, test: &Test) -> Condition {
        if matches!(self, StoredMember::Unheld) {
            return Condition::all().add(false_literal());
        }
        match test {
            Test::Exists => Condition::all().add(self.present()),
            Test::Value(value) => verdict_condition(value, self.positive(value.test()), |scope| {
                ComplementTarget::Where(match scope {
                    ComplementScope::AnyKind => self.present(),
                    ComplementScope::StringTarget => self.one_string(),
                })
            }),
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

    /// Whether the instance holds the member as one String: an `objectType`
    /// listing several types is an array of them.
    fn one_string(&self) -> SimpleExpr {
        if self.target_kind() != Some(TargetKind::String) {
            return false_literal();
        }
        match self {
            StoredMember::TypeKeys(keys) => single_type_name(keys).is_not_null(),
            StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::Unheld => self.present(),
        }
    }

    /// Whether the member passes `test`, asserted: equal to an operand,
    /// ordered against it, within a range, or matching a pattern.
    ///
    /// Clause 4.9: an operand "Is included in the target value, if the latter
    /// is an array", so an `objectType` is equal to any type it lists.
    fn positive(&self, test: &ScalarTest) -> Condition {
        match (self, test) {
            (StoredMember::TypeKeys(keys), ScalarTest::EqualsAny(operands)) => Condition::all().add(some_type_named(keys, operands)),
            (StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::Unheld, ScalarTest::EqualsAny(operands)) => {
                match operands.as_slice() {
                    [operand] => Condition::all().add(self.compared(Comparator::Equal, operand)),
                    operands => operands
                        .iter()
                        .fold(Condition::any(), |any, operand| any.add(self.compared(Comparator::Equal, operand))),
                }
            }
            (
                StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::TypeKeys(_) | StoredMember::Unheld,
                ScalarTest::Within(interval),
            ) => Condition::all()
                .add(self.compared(Comparator::Ordered(OrderRelation::GreaterOrEqual), interval.min()))
                .add(self.compared(Comparator::Ordered(OrderRelation::LessOrEqual), interval.max())),
            (
                StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::TypeKeys(_) | StoredMember::Unheld,
                ScalarTest::Ordered { relation, operand },
            ) => Condition::all().add(self.compared(Comparator::Ordered(*relation), operand)),
            (
                StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::TypeKeys(_) | StoredMember::Unheld,
                ScalarTest::Matches(pattern),
            ) => Condition::all().add(match self.target_kind() {
                Some(kind) if test.compares_with(kind) => self.matched(pattern),
                Some(_) | None => false_literal(),
            }),
            (
                StoredMember::Instant(_) | StoredMember::UnitCode(_) | StoredMember::Text(_) | StoredMember::TypeKeys(_) | StoredMember::Unheld,
                ScalarTest::Unsatisfiable,
            ) => Condition::all().add(false_literal()),
        }
    }

    /// One comparison of the member with an operand, constant `FALSE` when the
    /// operand does not compare with the member's kind.
    ///
    /// An `objectType` listing several types is an array, which no ordering
    /// holds for, so only a single type compares.
    fn compared(&self, comparator: Comparator, operand: &Operand) -> SimpleExpr {
        let compares = self.target_kind().is_some_and(|kind| operand.compares_with(kind, comparator.comparison()));
        if !compares {
            return false_literal();
        }
        match self {
            StoredMember::Instant(field) => compare(field.clone(), comparator, bound_operand(operand)),
            StoredMember::UnitCode(field) => listed_code_where(field, |code| operand.text().is_some_and(|text| comparator.holds_for_strings(code, text))),
            StoredMember::Text(stored) => text_compared(stored.clone(), comparator, operand),
            StoredMember::TypeKeys(keys) => text_compared(single_type_name(keys), comparator, operand),
            StoredMember::Unheld => false_literal(),
        }
    }

    /// Whether the member, a String, is in the language of `pattern`.
    fn matched(&self, pattern: &Pattern) -> SimpleExpr {
        match self {
            StoredMember::UnitCode(field) => listed_code_where(field, |code| pattern.is_match(code)),
            StoredMember::Text(text) => pattern_test(text.clone(), pattern, PatternIndexes::None),
            StoredMember::TypeKeys(keys) => pattern_test(single_type_name(keys), pattern, PatternIndexes::None),
            StoredMember::Instant(_) | StoredMember::Unheld => false_literal(),
        }
    }
}

/// `stored` compared with the text `operand` is, a String ordering collating
/// as a stored String value's does.
fn text_compared(stored: SimpleExpr, comparator: Comparator, operand: &Operand) -> SimpleExpr {
    compare(ordered_string(stored, operand, comparator), comparator, bound_operand(operand))
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
fn some_type_named(keys: &SimpleExpr, operands: &[Operand]) -> SimpleExpr {
    // clone: the bound array owns its elements
    let spellings: Vec<String> = operands
        .iter()
        .filter_map(|operand| match operand {
            Operand::Text(text) => Some(text.clone()),
            Operand::Number(_) | Operand::Boolean(_) | Operand::DateTime(_) | Operand::Date(_) | Operand::Time(_) => None,
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
        lowered_filter::term_of,
        sub_attribute::member_field::{member_condition, unit_code_text},
    };
    use andromeda_ngsi_ld_query::filter::{instance_metadata::InstanceMetadata, reading::terminal::Terminal};
    use andromeda_postgresql_schema::{
        current::{CurrentAttributes, SubAttribute},
        temporal::AttributeHistory,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::stored_unit_code::stored_unit_code;
    use cefact_units::UnitCode;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query, SelectStatement, Value};

    /// The statement of the `q` term `"https://example.com/t".<member><term>`
    /// over `holder`.
    fn member_sql(member: InstanceMetadata, term: &str, holder: InstanceRow) -> SelectStatement {
        let term = term_of(&format!("\"https://example.com/t\".\"{}\"{term}", member.iri().as_str()));
        assert_eq!(term.terminal(), &Terminal::InstanceMember(member));
        Query::select().cond_where(member_condition(member, holder, term.test())).to_owned()
    }

    #[test]
    fn an_instant_member_compares_its_field_with_a_date_time() {
        let statement = member_sql(InstanceMetadata::ObservedAt, ">2024-01-01T00:00:00Z", InstanceRow::Element(ElementLevel::at(1)));
        assert_statement_snapshot!("member-field-an-instant-member-compares-its-field-with-a-date-time", statement);
    }

    /// Clause 4.5.5: the default instance has no `datasetId`, which a row
    /// keys by the empty string and an element records as NULL.
    #[test]
    fn a_dataset_id_reads_the_grain_key_of_a_row_and_the_field_of_an_element() {
        let operand = "==\"urn:ngsi-ld:Dataset:d1\"";
        let current = member_sql(InstanceMetadata::DatasetId, operand, InstanceRow::Current);
        assert_statement_snapshot!("member-field-a-dataset-id-reads-the-grain-key-of-a-row-and-the-field-of-an-element-1", current);
        let history = member_sql(InstanceMetadata::DatasetId, operand, InstanceRow::History);
        assert_statement_snapshot!("member-field-a-dataset-id-reads-the-grain-key-of-a-row-and-the-field-of-an-element-2", history);
        let element = member_sql(InstanceMetadata::DatasetId, operand, InstanceRow::Element(ElementLevel::at(0)));
        assert_statement_snapshot!("member-field-a-dataset-id-reads-the-grain-key-of-a-row-and-the-field-of-an-element-3", element);
    }

    /// Clause 4.9 `unequal` asks for the target element, so the default
    /// instance, which has no `datasetId`, is no match.
    #[test]
    fn an_unequal_dataset_id_requires_one() {
        let statement = member_sql(InstanceMetadata::DatasetId, "!=\"urn:ngsi-ld:Dataset:d1\"", InstanceRow::Current);
        assert_statement_snapshot!("member-field-an-unequal-dataset-id-requires-one", statement);
    }

    /// A `valueType` is held as a dictionary key and compared as the expanded
    /// IRI the key names.
    #[test]
    fn a_value_type_compares_the_iri_its_key_names() {
        let statement = member_sql(InstanceMetadata::ValueType, "==\"https://example.com/String\"", InstanceRow::Current);
        assert_statement_snapshot!("member-field-a-value-type-compares-the-iri-its-key-names", statement);
    }

    /// Clause 4.9: an operand "Is included in the target value, if the latter
    /// is an array", so an `objectType` equals any type it lists.
    #[test]
    fn an_object_type_equality_tests_every_listed_type() {
        let statement = member_sql(
            InstanceMetadata::ObjectType,
            "==\"https://example.com/Device\",\"https://example.com/Gauge\"",
            InstanceRow::Current,
        );
        assert_statement_snapshot!("member-field-an-object-type-equality-tests-every-listed-type-1", statement);
        let unequal = member_sql(InstanceMetadata::ObjectType, "!=\"https://example.com/Device\"", InstanceRow::Current);
        assert_statement_snapshot!("member-field-an-object-type-equality-tests-every-listed-type-2", unequal);
    }

    /// An array is no String, so an ordering or a pattern reads an
    /// `objectType` only when it lists one type.
    #[test]
    fn an_object_type_ordering_or_pattern_reads_a_single_type() {
        for (index, term) in [">\"https://example.com/A\"", "~=\"Dev\"", "!~=\"Dev\""].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("member-field-an-object-type-ordering-or-pattern-reads-a-single-type-{}", index + 1),
                member_sql(InstanceMetadata::ObjectType, term, InstanceRow::Current)
            );
        }
    }

    /// Table 5.2.5-2 has `instanceId` only in the temporal representation, so
    /// no stored instance holds one, and no term on it matches either way.
    #[test]
    fn an_instance_id_term_matches_nothing() {
        for (index, term) in ["", "==\"urn:ngsi-ld:Instance:1\"", "!=\"urn:ngsi-ld:Instance:1\""].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("member-field-an-instance-id-term-matches-nothing-{}", index + 1),
                member_sql(InstanceMetadata::InstanceId, term, InstanceRow::Current)
            );
        }
    }

    /// The statement of `q` `"https://example.com/t"."unitCode"<term>` over a
    /// current row.
    fn unit_code_condition(term: &str) -> SelectStatement {
        let term = term_of(&format!("\"https://example.com/t\".\"https://uri.etsi.org/ngsi-ld/unitCode\"{term}"));
        Query::select()
            .cond_where(member_condition(InstanceMetadata::UnitCode, InstanceRow::Current, term.test()))
            .to_owned()
    }

    #[test]
    fn a_unit_code_equality_compares_the_stored_identity_of_the_code() {
        assert_statement_snapshot!(
            "member-field-a-unit-code-equality-compares-the-stored-identity-of-the-code",
            unit_code_condition("==\"CEL\"")
        );
    }

    /// Every listed code is a valid `unitCode`, whatever its status.
    #[test]
    fn a_deleted_unit_code_is_a_valid_q_operand() {
        assert_statement_snapshot!("member-field-a-deleted-unit-code-is-a-valid-q-operand", unit_code_condition("==\"KTM\""));
    }

    /// Clause 4.9: a String naming no listed code equals no `unitCode` and is
    /// unequal to every one.
    #[test]
    fn an_operand_naming_no_listed_code_equals_none_and_is_unequal_to_each() {
        assert_statement_snapshot!(
            "member-field-an-operand-naming-no-listed-code-equals-none-and-is-unequal-to-each-1",
            unit_code_condition("==\"NOTACODE\"")
        );
        assert_statement_snapshot!(
            "member-field-an-operand-naming-no-listed-code-equals-none-and-is-unequal-to-each-2",
            unit_code_condition("!=\"NOTACODE\"")
        );
    }

    /// Clause 4.5.2 makes `unitCode` a String, so an ordering compares the
    /// text of the code, and the column is tested against the shorter list.
    #[test]
    fn a_unit_code_ordering_compares_the_text_of_the_code() {
        assert_statement_snapshot!(
            "member-field-a-unit-code-ordering-compares-the-text-of-the-code-1",
            unit_code_condition(">=\"ZP\"")
        );
        assert_statement_snapshot!(
            "member-field-a-unit-code-ordering-compares-the-text-of-the-code-2",
            unit_code_condition("<\"ZP\"")
        );
    }

    /// Clause 4.9 recommends the UCA for a String ordering: `z` sorts before
    /// every code that begins with `Z`, where code point order puts it after
    /// all of them.
    #[test]
    fn a_unit_code_ordering_collates_under_the_icu_root_collation() {
        assert_statement_snapshot!(
            "member-field-a-unit-code-ordering-collates-under-the-icu-root-collation",
            unit_code_condition(">\"z\"")
        );
    }

    #[test]
    fn a_unit_code_pattern_matches_the_text_of_the_code() {
        assert_statement_snapshot!(
            "member-field-a-unit-code-pattern-matches-the-text-of-the-code-1",
            unit_code_condition("~=\"^CE\"")
        );
        assert_statement_snapshot!(
            "member-field-a-unit-code-pattern-matches-the-text-of-the-code-2",
            unit_code_condition("!~=\"^CE\"")
        );
    }

    /// Clause 4.9 `patternOp` and `notPatternOp` both require the target to be
    /// present, so a term every code satisfies still asks for one.
    #[test]
    fn a_pattern_every_code_matches_still_requires_a_unit_code() {
        assert_statement_snapshot!(
            "member-field-a-pattern-every-code-matches-still-requires-a-unit-code",
            unit_code_condition("~=\".\"")
        );
    }

    /// The bound list holds each code's text at the 1-based position its
    /// stored identity names.
    #[test]
    fn a_unit_code_text_is_read_at_the_position_of_its_stored_identity() {
        let statement = Query::select()
            .expr(unit_code_text(InstanceRow::Current.field(
                CurrentAttributes::UnitCodeId,
                AttributeHistory::UnitCodeId,
                SubAttribute::UnitCodeId,
            )))
            .to_owned();
        let (_, values) = statement.build(PostgresQueryBuilder);
        assert_statement_snapshot!("member-field-a-unit-code-text-is-read-at-the-position-of-its-stored-identity", statement);
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
        let statement = member_sql(InstanceMetadata::ObservedAt, "~=\"2024\"", InstanceRow::Current);
        assert_statement_snapshot!("member-field-an-instant-member-matches-no-pattern", statement);
    }
}
