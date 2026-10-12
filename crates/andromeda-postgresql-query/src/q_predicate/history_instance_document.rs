//! One history instance rebuilt as the expanded entity document a `q`
//! jsonpath reads.

use crate::q_predicate::history_window::HistoryAliases;
use andromeda_ngsi_ld_vocabulary::expanded::properties::{JSON, OBJECT, OBSERVED_AT, VALUE};
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    jsonb::{jsonb_build_array, jsonb_build_object, jsonb_strip_nulls, jsonb_typeof, to_jsonb},
    literal::{int_literal, text_literal},
};
use sea_query::{CaseStatement, Expr as SeaExpr, ExprTrait, Func, SimpleExpr};

/// The one history instance a probe scans, rebuilt as the expanded entity
/// document the `q` jsonpath reads: `[{"<attr>": [<instance>]}]`.
///
/// A Query Term reaching a sub-attribute or a member of a compound value is
/// answered by a jsonpath over an expanded entity (clause 4.9), and a history
/// instance keeps its value in the typed columns and its `observedAt` in its
/// own column; its sub-attributes are typed elements this document does not
/// rebuild, so a path into one matches nothing here. The value becomes the `hasValue` node the path descends into
/// (the stored node itself for an object value, a value node for a scalar),
/// except a `JsonProperty`'s raw JSON, which becomes the one `@json` value node
/// of a `hasJSON` carrier (clause 4.5.24). The Relationship object becomes a
/// `hasObject` node and `observedAt` its value node.
/// Members that hold nothing are stripped, so a path that reaches none of them
/// matches nothing rather than a JSON null.
pub(crate) fn instance_document(attribute_iri: &str, aliases: &HistoryAliases) -> SimpleExpr {
    let column = |name| aliases.instance_column(name);
    let scalar = Func::coalesce([
        to_jsonb(column(AttributeHistory::ValueNumber)),
        to_jsonb(column(AttributeHistory::ValueString)),
        to_jsonb(column(AttributeHistory::ValueBoolean)),
        to_jsonb(column(AttributeHistory::ValueDateTime)),
        column(AttributeHistory::ValueObject),
    ]);
    let value_node: SimpleExpr = CaseStatement::new()
        .case(
            SeaExpr::expr(jsonb_typeof(column(AttributeHistory::ValueObject))).eq("object"),
            column(AttributeHistory::ValueObject),
        )
        .finally(jsonb_build_object([text_literal("@value"), scalar.into()]))
        .into();
    let is_json_property = SeaExpr::expr(column(AttributeHistory::AttributeKind)).eq(int_literal(i64::from(AttributeKind::JsonProperty.code())));
    let value_carrier: SimpleExpr = CaseStatement::new()
        .case(is_json_property.clone().not(), jsonb_build_array(value_node)) // clone: both carriers test the family
        .into();
    let json_carrier: SimpleExpr = CaseStatement::new()
        .case(
            is_json_property,
            jsonb_build_array(jsonb_build_object([text_literal("@value"), column(AttributeHistory::ValueObject)])),
        )
        .into();
    let carriers = jsonb_strip_nulls(jsonb_build_object([
        text_literal(VALUE.as_str()),
        value_carrier,
        text_literal(JSON.as_str()),
        json_carrier,
        text_literal(OBJECT.as_str()),
        jsonb_build_array(jsonb_build_object([text_literal("@id"), column(AttributeHistory::ValueUri)])),
        text_literal(OBSERVED_AT.as_str()),
        jsonb_build_array(jsonb_build_object([text_literal("@value"), to_jsonb(column(AttributeHistory::ObservedAt))])),
    ]));
    jsonb_build_array(jsonb_build_object([text_literal(attribute_iri), jsonb_build_array(carriers)]))
}

#[cfg(test)]
mod tests {
    use crate::q_predicate::{history_instance_document::instance_document, history_window::HistoryAliases};
    use sea_query::{PostgresQueryBuilder, Query, SimpleExpr};

    /// The rebuilt instance carries the value carriers and `observedAt` the
    /// `q` jsonpath walks, under the attribute's IRI.
    #[test]
    fn the_instance_document_rebuilds_the_expanded_attribute() {
        let document: SimpleExpr = instance_document("https://example.org/speed", &HistoryAliases::top_level());
        let sql = Query::select().expr(document).to_owned().to_string(PostgresQueryBuilder);
        for fragment in [
            r"jsonb_build_array(jsonb_build_object('https://example.org/speed', jsonb_build_array(jsonb_strip_nulls(",
            "'https://uri.etsi.org/ngsi-ld/hasValue'",
            "'https://uri.etsi.org/ngsi-ld/hasObject'",
            r#"'https://uri.etsi.org/ngsi-ld/hasJSON', (CASE WHEN ("qh"."attribute_kind" = (6)) THEN jsonb_build_array(jsonb_build_object('@value', "qh"."value_object")) END)"#,
            r#"(CASE WHEN (NOT "qh"."attribute_kind" = (6)) THEN jsonb_build_array("#,
            "'https://uri.etsi.org/ngsi-ld/observedAt'",
            r#"CASE WHEN (jsonb_typeof("qh"."value_object") = 'object') THEN "qh"."value_object""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }
}
