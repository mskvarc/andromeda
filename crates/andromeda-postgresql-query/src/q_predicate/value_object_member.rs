//! The members of a compound value a `q` path addresses (`attr[a.b]`), read
//! out of the `value_object` of a current instance.
//!
//! Clause 4.9 evaluates a trailing path "as a `MemberExpression`" against the
//! target value. A structured Property value is stored as its expanded node,
//! whose members are IRI-keyed arrays of value nodes, so the path walks the
//! expanded member names and ends at `@value`; a `JsonProperty` stores its JSON
//! literal itself (clause 4.9 EXAMPLE 11), whose members keep the keys the
//! writer chose, so the path walks the names as the client wrote them. Each
//! member the path reaches is a candidate, and the term holds when one of them
//! passes (clause 4.9: "the target value shall be any Value"). Lax jsonpath
//! steps over an array of values as over each of its values, so a Property
//! holding several values is walked value by value.

use crate::q_predicate::{instance_row::InstanceRow, json_value_term::JsonTarget, value_column::ValueColumn};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::path::AttrPath;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_seaquery::{
    jsonb::{escape_jsonpath_key, jsonb_path_query},
    literal::int_literal,
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Query, SimpleExpr};

/// The alias one member the path reaches is read under, which is also the name
/// of its one column.
const MEMBER: &str = "member";

/// The JSON-LD keyword a value node carries its value under.
const VALUE_KEYWORD: &str = "@value";

/// Whether some member `path` addresses in the `value_object` of `row` passes
/// `test`.
pub(crate) fn on_some_member(path: &AttrPath, row: InstanceRow, test: impl Fn(&JsonTarget) -> Condition) -> Condition {
    let object = row.value(ValueColumn::Object);
    let expanded = members_passing(object.clone(), &expanded_jsonpath(&path.value_path), &test); // clone: both readings walk the one value
    let literal =
        Condition::all()
            .add(is_json_property(row.attribute_family()))
            .add(members_passing(object, &literal_jsonpath(&path.value_path_compact), &test));
    Condition::any().add(expanded).add(literal)
}

/// Whether an instance of the attribute family `family` holds a
/// `JsonProperty`, whose `value_object` is the JSON literal itself.
#[must_use]
pub fn is_json_property(family: SimpleExpr) -> SimpleExpr {
    SeaExpr::expr(family).eq(int_literal(i64::from(AttributeKind::JsonProperty.code())))
}

/// `EXISTS` over the members `jsonpath` reaches in `object` that pass `test`.
fn members_passing(object: SimpleExpr, jsonpath: &str, test: &impl Fn(&JsonTarget) -> Condition) -> SimpleExpr {
    let member = SeaExpr::col((Alias::new(MEMBER), Alias::new(MEMBER)));
    let target = JsonTarget {
        value: member.clone(), // clone: the presence test reads the same member
        object: None,
        presence: member.is_not_null(),
    };
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        .from_function(jsonb_path_query(object, jsonpath), Alias::new(MEMBER))
        .cond_where(test(&target));
    SeaExpr::exists(scan)
}

/// The jsonpath reaching the value of each member an expanded structured value
/// holds under the expanded member names: `$."<m1>"[*]."<m2>"[*]."@value"`.
#[must_use]
pub fn expanded_jsonpath(members: &[AttributeName]) -> String {
    let mut jsonpath = String::from("$");
    for member in members {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(member.as_str()));
        jsonpath.push_str("\"[*]");
    }
    jsonpath.push_str(".\"");
    jsonpath.push_str(VALUE_KEYWORD);
    jsonpath.push('"');
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
    use crate::q_predicate::value_object_member::{expanded_jsonpath, literal_jsonpath};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// Expansion turns the members of a structured value into IRI-keyed nodes,
    /// so the member hop precedes the `@value` selector.
    #[test]
    fn an_expanded_member_is_hopped_before_its_value_is_selected() {
        assert_eq!(
            expanded_jsonpath(&[name("https://example.com/city"), name("https://example.com/name")]),
            r#"$."https://example.com/city"[*]."https://example.com/name"[*]."@value""#
        );
    }

    #[test]
    fn a_json_literal_member_is_reached_by_its_written_names() {
        assert_eq!(literal_jsonpath(&[name("value"), name("total")]), r#"$."value"."total""#);
    }
}
