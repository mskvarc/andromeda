//! The members of a compound value a `q` path addresses (`attr[a.b]`), read
//! out of the `value_object` of a current or history instance.
//!
//! Clause 4.9 evaluates a trailing path "as a `MemberExpression`" against the
//! target value. A structured Property value is stored as its expanded node,
//! whose members are IRI-keyed arrays of value nodes, so the path walks the
//! expanded member names; a `JsonProperty` stores its JSON literal itself
//! (clause 4.9 EXAMPLE 11), whose members keep the keys the writer chose, so
//! the path walks the names as the client wrote them. A `GeoProperty` stores
//! its geometry, so the path walks the names as the client wrote them through
//! the `GeoJSON` object the geometry is ([`MemberSource::Geometry`]). A member
//! of an expanded value is read with the datatype its own `@type` declares; a
//! member of a JSON literal or of a geometry declares none.
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

use crate::q_predicate::{
    instance_row::InstanceRow,
    json_value_term::{JsonTarget, json_passes, json_string},
    value_column::ValueColumn,
    verdict_condition::{ComplementTarget, array_rule_condition, verdict_condition},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType};
use andromeda_ngsi_ld_query::filter::{
    reading::{
        array_target::ArrayTarget,
        family_reading::{MemberSource, family_reading},
        scalar_test::ScalarTest,
        term_reading::Test,
        value_test::{ComplementScope, ValueTest},
    },
    trailing_path::MemberPath,
};
use andromeda_ngsi_ld_vocabulary::keyword;
use andromeda_postgresql_seaquery::{
    cast::{cast, typed_null},
    json_type::{JsonType, jsonb_type_is},
    jsonb::{escape_jsonpath_key, jsonb_array_length, jsonb_path_query, jsonb_path_query_array, jsonb_path_query_silent},
    literal::{int_literal, text_literal, true_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    postgis::st_as_geojson,
};
use andromeda_postgresql_stored_values::{attribute_kind::AttributeKind, attribute_kind_code::stored_attribute_kind, value_placement::ValueField};
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

/// A Query Term against the member `members` addresses in the value of
/// `row`.
///
/// The expanded reading, the JSON-literal reading and the geometry reading are
/// disjoined: a `JsonProperty` holds no IRI-keyed member nodes, only a
/// `JsonProperty` is read as a literal, and only a family whose members are its
/// geometry's reads the geometry, which no other reading reads, so at most one
/// of them reaches a member.
pub(crate) fn member_term(test: &Test, members: &MemberPath, row: InstanceRow) -> Condition {
    let object = row.value(ValueColumn::Field(ValueField::Object));
    let expanded = reading_term(test, MemberShape::ValueNodes.member_source(object.clone(), members), MemberShape::ValueNodes); // clone: both readings walk the one value
    let literal = reading_term(test, MemberShape::JsonLiteral.member_source(object, members), MemberShape::JsonLiteral);
    let geometry = reading_term(
        test,
        MemberShape::JsonLiteral.member_source(geometry_object(row), members),
        MemberShape::JsonLiteral,
    );
    Condition::any()
        .add(
            Condition::all()
                .add(reads_members_from(MemberSource::Value, row.attribute_family()))
                .add(expanded),
        )
        .add(Condition::all().add(is_json_property(row.attribute_family())).add(literal))
        .add(
            Condition::all()
                .add(reads_members_from(MemberSource::Geometry, row.attribute_family()))
                .add(geometry),
        )
}

/// The `GeoJSON` geometry object of the geometry `row` holds, as `jsonb`; NULL
/// for an instance holding none.
fn geometry_object(row: InstanceRow) -> SimpleExpr {
    cast(st_as_geojson(row.value(ValueColumn::Field(ValueField::Geometry))), PostgresType::Jsonb)
}

/// Whether an instance of the attribute family `family` has its members read
/// from `source`, as [`family_reading`] says of each family.
fn reads_members_from(source: MemberSource, family: SimpleExpr) -> SimpleExpr {
    let codes = AttributeType::ALL
        .into_iter()
        .filter(|candidate| family_reading(*candidate).members == source)
        .map(|candidate| int_literal(i64::from(stored_attribute_kind(candidate).code())));
    SeaExpr::expr(family).is_in(codes)
}

/// The member a path reaches, as read in the `source` of a member term.
fn reached_member() -> SimpleExpr {
    SimpleExpr::from(SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER))))
}

/// `EXISTS` over the member `source` reads, held to `test`.
fn reading_term(test: &Test, source: SelectStatement, shape: MemberShape) -> SimpleExpr {
    let condition = match test {
        // The member was reached, which is the target element (clause 4.9:
        // "the matching Entities shall be those which define the target
        // element").
        Test::Exists => Condition::all().add(true_literal()),
        Test::Value(value) => value_term(value, shape),
    };
    let mut scan = Query::select();
    scan.expr(int_literal(1)).from_subquery(source, Alias::new(MEMBER)).cond_where(condition);
    SeaExpr::exists(scan)
}

/// What `value` asks of the member reached, read under `shape`.
///
/// A scalar target is tested directly, and an array target by the rule the
/// reading gives it, each of its values tested alike. A complement of `!=`
/// ranges over a member holding a target value, and one of `!~=` over a
/// member holding one String.
fn value_term(value: &ValueTest, shape: MemberShape) -> Condition {
    let target = shape.scalar(reached_member());
    let scalar = Condition::all()
        .add(shape.holds_scalar(reached_member()))
        .add(json_passes(&target, value.test()));
    let array = array_rule_condition(value.asserted_array_rule(ArrayTarget::ValueArray), shape.holds_array(reached_member()), || {
        some_element(reached_member(), shape, value.test())
    });
    let positive = match array {
        Some(array) => Condition::any().add(scalar).add(array),
        None => scalar,
    };
    verdict_condition(value, positive, |scope| {
        ComplementTarget::Where(match scope {
            ComplementScope::AnyKind => shape.holds_target(reached_member()),
            ComplementScope::StringTarget => shape.holds_scalar(reached_member()).and(json_string(&target)),
        })
    })
}

/// Whether one value of the array target `member` holds passes `test`.
fn some_element(member: SimpleExpr, shape: MemberShape, test: &ScalarTest) -> SimpleExpr {
    let element = shape.element(SeaExpr::col((Alias::new(ELEMENT), Alias::new(ELEMENT))));
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_path_query(member, EVERY_ELEMENT), Alias::new(ELEMENT))
        .cond_where(json_passes(&element, test));
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
        lowered_filter::term_of,
        value_object_member::{expanded_jsonpath, expanded_member_jsonpath, literal_jsonpath, literal_member_jsonpath, member_term},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::reading::terminal::Terminal;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use pretty_assertions::assert_eq;
    use sea_query::{Query, SelectStatement};

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// The statement the one member term of `q` compiles to over `row`.
    fn member_sql(q: &str, row: InstanceRow) -> SelectStatement {
        let term = term_of(q);
        let (Terminal::Members(members) | Terminal::ByFamily { members, .. }) = term.terminal() else {
            panic!("a member path");
        };
        Query::select().cond_where(member_term(term.test(), members, row)).to_owned()
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
        let statement = member_sql("\"https://example.com/address\"[city]==\"Bern\"", InstanceRow::History);
        assert_statement_snapshot!("value-object-member-a-temporal-q-on-a-compound-value-reads-value-object", statement);
    }

    /// Only the literals of an expanded member are compared; one is a scalar
    /// target and two or more an array.
    #[test]
    fn an_expanded_member_keeps_its_literals_and_counts_them() {
        let statement = member_sql("address[city]==4", InstanceRow::Current);
        assert_statement_snapshot!("value-object-member-an-expanded-member-keeps-its-literals-and-counts-them", statement);
    }

    /// Clause 4.9 `equal`: the operand "Is included in the target value, if
    /// the latter is an array", for a JSON array of a literal too.
    #[test]
    fn an_equality_on_a_member_tests_each_value_of_an_array_target() {
        let statement = member_sql("address[city]==4", InstanceRow::Current);
        assert_statement_snapshot!("value-object-member-an-equality-on-a-member-tests-each-value-of-an-array-target", statement);
    }

    /// Clause 4.9: a target of another data type is unequal to the operand, and
    /// `unequal` asks for the target element, so `!=` negates the whole
    /// equality over a reached member.
    #[test]
    fn an_inequality_on_a_member_is_the_total_negation_of_its_equality_over_the_target() {
        let statement = member_sql("address[city]!=4", InstanceRow::Current);
        assert_statement_snapshot!(
            "value-object-member-an-inequality-on-a-member-is-the-total-negation-of-its-equality-over-the-target",
            statement
        );
    }

    #[test]
    fn an_outside_range_on_a_member_is_the_total_negation_of_within() {
        let statement = member_sql("address[floor]!=1..5", InstanceRow::Current);
        assert_statement_snapshot!("value-object-member-an-outside-range-on-a-member-is-the-total-negation-of-within", statement);
    }

    #[test]
    fn an_outside_value_list_on_a_member_is_the_total_negation_of_membership() {
        let statement = member_sql("address[floor]!=1,2", InstanceRow::Current);
        assert_statement_snapshot!(
            "value-object-member-an-outside-value-list-on-a-member-is-the-total-negation-of-membership",
            statement
        );
    }

    /// Clause 4.9 makes an ordering across data types false and matches no
    /// array, so the ordering reads the guarded lift of a scalar target alone.
    #[test]
    fn an_ordering_on_a_member_compares_a_scalar_target_only() {
        let statement = member_sql("address[city]<4", InstanceRow::Current);
        assert_statement_snapshot!("value-object-member-an-ordering-on-a-member-compares-a-scalar-target-only", statement);
    }

    /// Clause 4.9: "If the target value data type is different than String
    /// then it shall be considered as not matching", in both polarities.
    #[test]
    fn a_pattern_on_a_member_reads_only_a_string_not_declared_as_a_time_value() {
        for (index, q) in ["address[city]~=\"^Cel\"", "address[city]!~=\"^Cel\""].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!(
                    "value-object-member-a-pattern-on-a-member-reads-only-a-string-not-declared-as-a-time-value-{}",
                    index + 1
                ),
                member_sql(q, InstanceRow::Current)
            );
        }
    }

    /// The member reached is the target element, whatever it holds.
    #[test]
    fn an_existence_on_a_member_asks_only_for_the_member() {
        let statement = member_sql("address[city]", InstanceRow::Current);
        assert_statement_snapshot!("value-object-member-an-existence-on-a-member-asks-only-for-the-member", statement);
    }
}
