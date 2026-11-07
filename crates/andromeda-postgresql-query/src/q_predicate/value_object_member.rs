//! The members of a compound value a `q` path addresses (`attr[a.b]`), read
//! out of the `value_object` of a current or history instance.
//!
//! Clause 4.9 evaluates a trailing path "as a `MemberExpression`" against the
//! target value. A structured Property value is stored as its expanded node,
//! whose members are IRI-keyed arrays of value nodes, so the path walks the
//! expanded member names; a `JsonProperty` stores its JSON literal itself
//! (clause 4.9 EXAMPLE 11), whose members keep the keys the writer chose, so
//! the path walks the names as the client wrote them. A member of an expanded
//! value is read with the datatype its own `@type` declares; a member of a
//! JSON literal declares none.
//!
//! A member expression accesses a property of an object, and an array has no
//! named property, so every step before the last descends only through a
//! member holding exactly one value. The member the last step reaches is the
//! target element, and its value the target value: one literal of an expanded
//! member is a scalar target, two or more are an array, as is a JSON array of
//! a literal. Clause 4.9 compares an array by membership under the equality
//! operators ("Is included in the target value, if the latter is an array",
//! and under `unequal` "Is not included"), and the ordering, range and pattern
//! operators match no array.

use crate::{
    operator::does_not_hold,
    q_predicate::{
        instance_row::InstanceRow,
        json_value_term::{JsonTarget, json_equal, json_ordering, json_pattern},
        value_column::ValueColumn,
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::{
    predicate::{PatternPolarity, Predicate, SetMembership},
    scalar::CompareValue,
    trailing_path::MemberPath,
};
use andromeda_ngsi_ld_vocabulary::keyword;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_seaquery::{
    cast::typed_null,
    json_type::{JsonType, jsonb_type_is},
    jsonb::{escape_jsonpath_key, jsonb_array_length, jsonb_path_query, jsonb_path_query_array, jsonb_path_query_silent},
    literal::{int_literal, text_literal, true_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};

/// The alias the member a path reaches is read under, which is also the name
/// of its one column.
const MEMBER: &str = "member";

/// The alias one value of an array target is read under, which is also the
/// name of its one column.
const ELEMENT: &str = "element";

/// The jsonpath keeping the values of an expanded member that are literals,
/// which are what a Query Term compares: a nested node or a reference carries
/// no `@value`.
const LITERAL_VALUES: &str = "lax $[*] ? (exists(@.\"@value\"))";

/// The jsonpath reaching each value of an array target.
const EVERY_ELEMENT: &str = "strict $[*]";

/// How the member a path reaches holds its value.
#[derive(Debug, Clone, Copy)]
enum MemberShape {
    /// The literal value nodes of an expanded structured value's member, each
    /// with its value under `@value` and its datatype under `@type`.
    ValueNodes,
    /// The member of a JSON literal, which is its value and declares no
    /// datatype.
    JsonLiteral,
}

impl MemberShape {
    /// Whether the member, read under this shape, holds a scalar target.
    fn holds_scalar(self, member: SimpleExpr) -> SimpleExpr {
        match self {
            MemberShape::ValueNodes => SeaExpr::expr(jsonb_array_length(member)).eq(int_literal(1)),
            MemberShape::JsonLiteral => jsonb_type_is(member, JsonType::Array).not(),
        }
    }

    /// Whether the member, read under this shape, holds an array target.
    fn holds_array(self, member: SimpleExpr) -> SimpleExpr {
        match self {
            MemberShape::ValueNodes => SeaExpr::expr(jsonb_array_length(member)).gte(int_literal(2)),
            MemberShape::JsonLiteral => jsonb_type_is(member, JsonType::Array),
        }
    }

    /// Whether the member, read under this shape, holds a target value: an
    /// expanded member holding only nodes carries no literal to compare, while
    /// every member of a JSON literal is a value.
    fn holds_target(self, member: SimpleExpr) -> SimpleExpr {
        match self {
            MemberShape::ValueNodes => SeaExpr::expr(jsonb_array_length(member)).gte(int_literal(1)),
            MemberShape::JsonLiteral => true_literal(),
        }
    }

    /// The scalar target the member holds, read where [`Self::holds_scalar`]
    /// holds.
    fn scalar(self, member: SimpleExpr) -> JsonTarget {
        match self {
            MemberShape::ValueNodes => value_node_target(member.json_get(int_literal(0))),
            MemberShape::JsonLiteral => literal_target(member),
        }
    }

    /// The target one value of an array target is.
    fn element(self, element: SimpleExpr) -> JsonTarget {
        match self {
            MemberShape::ValueNodes => value_node_target(element),
            MemberShape::JsonLiteral => literal_target(element),
        }
    }

    /// The source of the one row the member is read in, absent when the path
    /// reaches no member.
    fn member_source(self, object: SimpleExpr, members: &MemberPath) -> SelectStatement {
        let mut source = Query::select();
        match self {
            MemberShape::ValueNodes => {
                let reached = SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER)));
                source
                    .expr_as(jsonb_path_query_array(reached, LITERAL_VALUES), Alias::new(MEMBER))
                    .from_function(
                        jsonb_path_query_silent(object, &expanded_member_jsonpath(&members.addressed)),
                        Alias::new(MEMBER),
                    );
            }
            MemberShape::JsonLiteral => {
                source
                    .expr_as(SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER))), Alias::new(MEMBER))
                    .from_function(jsonb_path_query_silent(object, &literal_member_jsonpath(&members.written)), Alias::new(MEMBER));
            }
        }
        source
    }
}

/// The target a literal value node is: its `@value`, declared as its `@type`.
fn value_node_target(node: SimpleExpr) -> JsonTarget {
    let value = node.clone().json_get(text_literal(keyword::VALUE)); // clone: the datatype reads the same node
    JsonTarget {
        presence: value.clone().is_not_null(), // clone: the comparisons read the same value
        value,
        datatype: node.json_get_text(text_literal(keyword::TYPE)),
    }
}

/// The target a value of a JSON literal is, which declares no datatype.
fn literal_target(value: SimpleExpr) -> JsonTarget {
    JsonTarget {
        presence: value.clone().is_not_null(), // clone: the comparisons read the same value
        value,
        datatype: typed_null(PostgresType::Text),
    }
}

/// A Query Term against the member `members` addresses in the `value_object`
/// of `row`.
///
/// The expanded reading and the JSON-literal reading are disjoined: a
/// `JsonProperty` holds no IRI-keyed member nodes, and only a `JsonProperty`
/// is read as a literal, so at most one of them reaches a member.
pub(crate) fn member_term(pred: &Predicate, members: &MemberPath, row: InstanceRow) -> Condition {
    let object = row.value(ValueColumn::Object);
    let expanded = reading_term(pred, MemberShape::ValueNodes.member_source(object.clone(), members), MemberShape::ValueNodes); // clone: both readings walk the one value
    let literal = reading_term(pred, MemberShape::JsonLiteral.member_source(object, members), MemberShape::JsonLiteral);
    Condition::any()
        .add(expanded)
        .add(Condition::all().add(is_json_property(row.attribute_family())).add(literal))
}

/// `EXISTS` over the member `source` reads, held to `pred`.
fn reading_term(pred: &Predicate, source: SelectStatement, shape: MemberShape) -> SimpleExpr {
    let member = || SimpleExpr::from(SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER))));
    let scalar = Condition::all().add(shape.holds_scalar(member()));
    let target = shape.scalar(member());
    let condition = match pred {
        // The member was reached, which is the target element (clause 4.9:
        // "the matching Entities shall be those which define the target
        // element").
        Predicate::Exists(_) => Condition::all().add(true_literal()),
        Predicate::Equality { membership, rhs, .. } => {
            let equal = Condition::any().add(scalar.add(json_equal(&target, rhs)));
            // A range is two ordering comparisons, which no array satisfies;
            // a scalar and a value list are tested for inclusion.
            let equal = match rhs {
                CompareValue::Scalar(_) | CompareValue::ValueList(_) => {
                    equal.add(Condition::all().add(shape.holds_array(member())).add(some_element(member(), shape, rhs)))
                }
                CompareValue::Range { .. } => equal,
            };
            match membership {
                SetMembership::Within => equal,
                // `unequal` asks for the target element and for no match in
                // it, a value of another data type being unequal.
                SetMembership::Outside => Condition::all().add(shape.holds_target(member())).add(does_not_hold(equal)),
            }
        }
        Predicate::Ordering { op, rhs, .. } => scalar.add(json_ordering(&target, *op, rhs)),
        Predicate::MatchPattern { pattern, .. } => scalar.add(json_pattern(&target, pattern, PatternPolarity::Match)),
        Predicate::NotMatchPattern { pattern, .. } => scalar.add(json_pattern(&target, pattern, PatternPolarity::NotMatch)),
    };
    let mut scan = Query::select();
    scan.expr(int_literal(1)).from_subquery(source, Alias::new(MEMBER)).cond_where(condition);
    SeaExpr::exists(scan)
}

/// Whether one value of the array target `member` holds equals the operand.
fn some_element(member: SimpleExpr, shape: MemberShape, rhs: &CompareValue) -> SimpleExpr {
    let element = shape.element(SeaExpr::col((Alias::new(ELEMENT), Alias::new(ELEMENT))));
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_path_query(member, EVERY_ELEMENT), Alias::new(ELEMENT))
        .cond_where(json_equal(&element, rhs));
    SeaExpr::exists(scan)
}

/// Whether an instance of the attribute family `family` holds a
/// `JsonProperty`, whose `value_object` is the JSON literal itself.
#[must_use]
pub fn is_json_property(family: SimpleExpr) -> SimpleExpr {
    SeaExpr::expr(family).eq(int_literal(i64::from(AttributeKind::JsonProperty.code())))
}

/// The `strict` jsonpath reaching the values of the member an expanded
/// structured value holds under the expanded member names, through a member
/// holding exactly one value at every step before the last:
/// `strict $."<m1>" ? (@.size() == 1)[0]."<m2>"`.
fn expanded_member_jsonpath(members: &[AttributeName]) -> String {
    let mut jsonpath = String::from("strict $");
    for (position, member) in members.iter().enumerate() {
        if position > 0 {
            jsonpath.push_str(" ? (@.size() == 1)[0]");
        }
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(member.as_str()));
        jsonpath.push('"');
    }
    jsonpath
}

/// The `strict` jsonpath reaching the member of a JSON literal under the names
/// as the client wrote them, through objects only: `strict $."<m1>"."<m2>"`.
fn literal_member_jsonpath(members: &[AttributeName]) -> String {
    format!("strict {}", literal_jsonpath(members))
}

/// The jsonpath reaching each value node an expanded structured value holds
/// under the expanded member names: `$."<m1>"[*]."<m2>"[*]`.
#[must_use]
pub fn expanded_jsonpath(members: &[AttributeName]) -> String {
    let mut jsonpath = String::from("$");
    for member in members {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(member.as_str()));
        jsonpath.push_str("\"[*]");
    }
    jsonpath
}

/// The jsonpath reaching a member of a JSON literal under the names as the
/// client wrote them: `$."<m1>"."<m2>"`.
#[must_use]
pub fn literal_jsonpath(members: &[AttributeName]) -> String {
    let mut jsonpath = String::from("$");
    for member in members {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(member.as_str()));
        jsonpath.push('"');
    }
    jsonpath
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{
        instance_row::InstanceRow,
        value_object_member::{expanded_jsonpath, expanded_member_jsonpath, literal_jsonpath, literal_member_jsonpath, member_term},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse, path::QueryPath};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// The SQL the one member term of `q` compiles to over `row`.
    fn member_sql(q: &str, row: InstanceRow) -> String {
        let Expr::Pred(pred) = parse(q).expect("parse q") else {
            panic!("a single predicate");
        };
        let QueryPath::Local(path) = pred.path() else {
            panic!("a local path");
        };
        let members = path.members().expect("a member path");
        Query::select()
            .cond_where(member_term(&pred, members, row))
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    /// Expansion turns the members of a structured value into IRI-keyed nodes,
    /// so each member hop steps over the values of the member before it.
    #[test]
    fn an_expanded_member_is_hopped_before_its_value_is_selected() {
        assert_eq!(
            expanded_jsonpath(&[name("https://example.com/city"), name("https://example.com/name")]),
            r#"$."https://example.com/city"[*]."https://example.com/name"[*]"#
        );
    }

    #[test]
    fn a_json_literal_member_is_reached_by_its_written_names() {
        assert_eq!(literal_jsonpath(&[name("value"), name("total")]), r#"$."value"."total""#);
    }

    /// A member expression over an array addresses no member, so every step
    /// before the last descends only through a member holding one value.
    #[test]
    fn an_expanded_member_path_steps_only_through_a_member_holding_one_value() {
        assert_eq!(
            expanded_member_jsonpath(&[name("https://example.com/city")]),
            r#"strict $."https://example.com/city""#
        );
        assert_eq!(
            expanded_member_jsonpath(&[name("https://example.com/a"), name("https://example.com/b"), name("https://example.com/c")]),
            r#"strict $."https://example.com/a" ? (@.size() == 1)[0]."https://example.com/b" ? (@.size() == 1)[0]."https://example.com/c""#
        );
    }

    #[test]
    fn a_json_literal_member_path_steps_through_objects_only() {
        assert_eq!(literal_member_jsonpath(&[name("value"), name("total")]), r#"strict $."value"."total""#);
    }

    /// Clause 4.9 evaluates `address[city]` as a `MemberExpression` of the
    /// target value, which a history row holds in its own `value_object`.
    #[test]
    fn a_temporal_q_on_a_compound_value_reads_value_object() {
        let sql = member_sql("\"https://example.com/address\"[city]==\"Bern\"", InstanceRow::History);
        assert!(sql.contains(r#"jsonb_path_query("qh"."value_object", 'strict $."city"', '{}', TRUE)"#), "{sql}");
        assert!(sql.contains(r"-> ('@value')"), "{sql}");
        assert!(sql.contains("'Bern'"), "{sql}");
        assert!(!sql.contains("jsonb_build_object"), "{sql}");
    }

    /// Only the literals of an expanded member are compared; one is a scalar
    /// target and two or more an array.
    #[test]
    fn an_expanded_member_keeps_its_literals_and_counts_them() {
        let sql = member_sql("address[city]==4", InstanceRow::Current);
        assert!(
            sql.contains(r#"jsonb_path_query_array("member"."member", 'lax $[*] ? (exists(@."@value"))')"#),
            "{sql}"
        );
        assert!(sql.contains(r#"jsonb_array_length("member"."member") = (1)"#), "{sql}");
        assert!(sql.contains(r#"jsonb_array_length("member"."member") >= (2)"#), "{sql}");
    }

    /// Clause 4.9 `equal`: the operand "Is included in the target value, if
    /// the latter is an array", for a JSON array of a literal too.
    #[test]
    fn an_equality_on_a_member_tests_each_value_of_an_array_target() {
        let sql = member_sql("address[city]==4", InstanceRow::Current);
        assert!(sql.contains(r#"jsonb_path_query("member"."member", 'strict $[*]') AS "element""#), "{sql}");
        assert!(sql.contains(r#"jsonb_typeof("member"."member") = ('array')"#), "{sql}");
    }

    /// Clause 4.9: a target of another data type is unequal to the operand, and
    /// `unequal` asks for the target element, so `!=` negates the whole
    /// equality over a reached member.
    #[test]
    fn an_inequality_on_a_member_is_the_total_negation_of_its_equality_over_the_target() {
        let sql = member_sql("address[city]!=4", InstanceRow::Current);
        assert!(sql.contains("NOT COALESCE("), "{sql}");
        assert!(!sql.contains(") <> 4"), "{sql}");
        assert!(
            sql.contains(r#"WHERE jsonb_array_length("member"."member") >= (1) AND (NOT COALESCE("#),
            "{sql}"
        );
    }

    #[test]
    fn an_outside_range_on_a_member_is_the_total_negation_of_within() {
        let sql = member_sql("address[floor]!=1..5", InstanceRow::Current);
        assert!(sql.contains("NOT COALESCE("), "{sql}");
        assert!(sql.contains(">= 1") && sql.contains("<= 5"), "{sql}");
        assert!(!sql.contains(r#"AS "element""#), "a range is never within an array: {sql}");
    }

    #[test]
    fn an_outside_value_list_on_a_member_is_the_total_negation_of_membership() {
        let sql = member_sql("address[floor]!=1,2", InstanceRow::Current);
        assert!(sql.contains("NOT COALESCE("), "{sql}");
        assert!(sql.contains(" = 1") && sql.contains(" = 2"), "{sql}");
        assert!(sql.contains(r#"AS "element""#), "{sql}");
    }

    /// Clause 4.9 makes an ordering across data types false and matches no
    /// array, so the ordering reads the guarded lift of a scalar target alone.
    #[test]
    fn an_ordering_on_a_member_compares_a_scalar_target_only() {
        let sql = member_sql("address[city]<4", InstanceRow::Current);
        assert!(sql.contains("= ('number')"), "{sql}");
        assert!(!sql.contains("NOT COALESCE("), "{sql}");
        assert!(!sql.contains(r#"AS "element""#), "{sql}");
    }

    /// Clause 4.9: "If the target value data type is different than String
    /// then it shall be considered as not matching", in both polarities.
    #[test]
    fn a_pattern_on_a_member_reads_only_a_string_not_declared_as_a_time_value() {
        for q in ["address[city]~=\"^Cel\"", "address[city]!~=\"^Cel\""] {
            let sql = member_sql(q, InstanceRow::Current);
            assert!(sql.contains("= ('string')"), "{sql}");
            assert!(sql.contains("'https://uri.etsi.org/ngsi-ld/DateTime'"), "{sql}");
        }
    }

    /// The member reached is the target element, whatever it holds.
    #[test]
    fn an_existence_on_a_member_asks_only_for_the_member() {
        let sql = member_sql("address[city]", InstanceRow::Current);
        assert!(sql.contains("WHERE TRUE"), "{sql}");
    }
}
