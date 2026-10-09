//! Query Terms decided over an attribute instance rebuilt as an expanded
//! JSON-LD document.
//!
//! A temporal `q` (clause 5.7.4.4) on a sub-attribute (`temperature.observedAt`)
//! or on the interior of a compound value (`address[city]`) reaches a target
//! no typed column of `attribute_history` holds: the sub-attributes of a
//! history instance are kept verbatim in its `metadata`. Each in-window
//! instance is rebuilt as the expanded document of an entity holding that one
//! instance (see
//! [`instance_document`](crate::q_predicate::history_instance_document::instance_document)),
//! and the term walks it by jsonpath.

use crate::{
    dictionary::query_names::QueryNames,
    jsonb::{attribute_instances_jsonpath, instance_json_literal_member_jsonpath, instance_value_jsonpath},
    q_predicate::{
        history_instance_document::instance_document,
        history_window::{HistoryAliases, HistoryWindow},
        json_value_term::{JsonTarget, json_equality, json_ordering, json_pattern},
    },
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{
    filter::{
        path::AttrPath,
        predicate::{OrderingOp, PatternPolarity, SetMembership},
        scalar::{CompareValue, Scalar},
    },
    pattern::Pattern,
};
use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBJECT, VALUE};
use andromeda_postgresql_seaquery::{
    jsonb::{jsonb_path_exists, jsonb_path_query, jsonb_path_query_first},
    literal::{false_literal, int_literal},
};
use sea_query::{Alias, Condition, Expr as SeaExpr, ExprTrait, Func, Query, SimpleExpr};

/// The alias one instance of the addressed attribute element is read under,
/// which is also the name of its one column.
const INSTANCE: &str = "attribute_instance";

/// A condition over the rebuilt document of each in-window instance of
/// `attribute`, which holds when one instance satisfies it.
///
/// The condition is checked inside the window's probe over the instances, so
/// one instance that satisfies it is enough. An attribute the dictionary never
/// recorded has no history, so it matches nothing.
pub(crate) fn on_history_document(
    window: &HistoryWindow,
    attribute: &AttributeName,
    names: &QueryNames,
    build: impl FnOnce(SimpleExpr) -> Condition,
) -> Condition {
    let aliases = HistoryAliases::top_level();
    match names.attributes.get(attribute) {
        Some(id) => window.exists(id, build(instance_document(attribute.as_str(), &aliases)), &aliases),
        None => Condition::all().add(false_literal()),
    }
}

/// Whether some instance of the attribute element `path` addresses in
/// `document` passes `test`, which reads that one instance node.
///
/// Clause 4.9: "If a Property has multiple instances (identified by its
/// respective datasetId), and no datasetId is explicitly addressed, the target
/// value shall be any Value of such instances". The `q` grammar addresses no
/// datasetId, so the term is decided instance by instance, across every
/// instance of the attribute and of each sub-attribute on the way, and holds
/// when one of them passes. An element with no instance passes no test, which
/// is the non-match clause 4.9 gives a missing target element in both
/// polarities.
fn on_some_instance(path: &AttrPath, document: &SimpleExpr, test: impl FnOnce(&Instance<'_>) -> Condition) -> Condition {
    let sub_attrs: Vec<&str> = path.sub_attrs.iter().map(AttributeName::as_str).collect();
    let node = SeaExpr::col((Alias::new(INSTANCE), Alias::new(INSTANCE)));
    let instance = Instance { path, node };
    let mut scan = Query::select();
    scan.expr(int_literal(1))
        // clone: sea-query owns every operand, and one document is read by several terms
        .from_function(
            jsonb_path_query(document.clone(), &attribute_instances_jsonpath(path.attr.as_str(), &sub_attrs)),
            Alias::new(INSTANCE),
        )
        .cond_where(test(&instance));
    Condition::all().add(SeaExpr::exists(scan))
}

/// One instance node of the attribute element a path addresses.
struct Instance<'a> {
    /// The path the instance was reached by.
    path: &'a AttrPath,
    /// The instance node itself.
    node: SimpleExpr,
}

impl Instance<'_> {
    /// The name of the element this is an instance of: the last sub-attribute
    /// of the path, or the attribute itself.
    fn element(&self) -> &str {
        self.path.sub_attrs.last().unwrap_or(&self.path.attr).as_str()
    }

    /// The members of a compound value the path walks below the element.
    fn value_path(&self) -> Vec<&str> {
        self.path.value_path.iter().map(AttributeName::as_str).collect()
    }

    /// Whether the Property value the path addresses is present here.
    ///
    /// A trailing path is read both into an expanded Property value and into a
    /// `JsonProperty`'s raw JSON, and a member present under either one exists.
    fn value_presence(&self) -> SimpleExpr {
        let expanded = jsonb_path_exists(self.node.clone(), &self.carrier_jsonpath(VALUE.as_str())); // clone: sea-query owns every operand
        match self.json_literal_jsonpath() {
            Some(jsonpath) => expanded.or(jsonb_path_exists(self.node.clone(), &jsonpath)), // clone: sea-query owns every operand
            None => expanded,
        }
    }

    /// The Property value the path addresses here.
    ///
    /// Clause 4.9 evaluates a trailing path "as a `MemberExpression`" against the
    /// seed target value. An expanded Property value and a `JsonProperty`'s raw
    /// JSON (clause 4.9 EXAMPLE 11) are both seed target values, and an attribute
    /// holds at most one of them, so the two readings coalesce.
    fn value(&self) -> SimpleExpr {
        let expanded = self.carried(VALUE.as_str());
        match self.json_literal_jsonpath() {
            Some(jsonpath) => Func::coalesce([expanded, jsonb_path_query_first(self.node.clone(), &jsonpath)]).into(), // clone: sea-query owns every operand
            None => expanded,
        }
    }

    /// The instance as the JSON target a term compares.
    fn target(&self, object: Objects) -> JsonTarget {
        JsonTarget {
            value: self.value(),
            object: match object {
                Objects::Read => Some(self.carried(OBJECT.as_str())),
                Objects::Ignored => None,
            },
            presence: self.value_presence(),
        }
    }

    /// The value behind the `terminal_iri` carrier here.
    fn carried(&self, terminal_iri: &str) -> SimpleExpr {
        jsonb_path_query_first(self.node.clone(), &self.carrier_jsonpath(terminal_iri)) // clone: sea-query owns every operand, and one instance is read by several extractions
    }

    /// The jsonpath from this instance to the value behind `terminal_iri`.
    fn carrier_jsonpath(&self, terminal_iri: &str) -> String {
        instance_value_jsonpath(self.element(), &self.value_path(), terminal_iri)
    }

    /// The jsonpath from this instance to the member a trailing path names
    /// inside a `JsonProperty`'s raw JSON, or `None` when the path has no
    /// trailing members.
    fn json_literal_jsonpath(&self) -> Option<String> {
        (!self.path.value_path.is_empty()).then(|| instance_json_literal_member_jsonpath(&self.value_path()))
    }
}

/// Whether a term reads a Relationship's object beside a Property's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Objects {
    /// The equality operators, which clause 4.9 lets compare an object.
    Read,
    /// Every other operator, which a Relationship operand non-matches.
    Ignored,
}

/// Compile an existence predicate that carries a sub-path.
#[must_use]
pub fn document_exists(path: &AttrPath, document: &SimpleExpr) -> Condition {
    on_some_instance(path, document, |instance| Condition::all().add(instance.value_presence()))
}

/// Compile an equality predicate (`==`/`!=`) against the document.
#[must_use]
pub fn document_equality(path: &AttrPath, membership: SetMembership, rhs: &CompareValue, document: &SimpleExpr) -> Condition {
    on_some_instance(path, document, |instance| json_equality(&instance.target(Objects::Read), membership, rhs))
}

/// Compile an ordering predicate (`>`/`>=`/`<`/`<=`) against the document.
#[must_use]
pub fn document_ordering(path: &AttrPath, op: OrderingOp, rhs: &Scalar, document: &SimpleExpr) -> Condition {
    on_some_instance(path, document, |instance| json_ordering(&instance.target(Objects::Ignored), op, rhs))
}

/// Compile a pattern predicate against the document.
#[must_use]
pub fn document_pattern(path: &AttrPath, pattern: &Pattern, polarity: PatternPolarity, document: &SimpleExpr) -> Condition {
    on_some_instance(path, document, |instance| json_pattern(&instance.target(Objects::Ignored), pattern, polarity))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
        q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, projection::compile},
    };
    use andromeda_ngsi_ld_query::{
        filter::{expression::Expr, parser, path::QueryPath, predicate::Predicate, scalar::Scalar},
        temporal::window::TimeInterval,
    };
    use andromeda_postgresql_schema::{current::Entities, temporal::AttributeHistory};
    use chrono::{DateTime, Utc};
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    const SPEED: &str = "https://example.com/speed";

    fn names() -> QueryNames {
        QueryNames {
            attributes: attribute_names(),
            types: ResolvedNames::default(),
        }
    }

    fn attribute_names() -> ResolvedNames<AttributeName> {
        ResolvedNames::from_pairs([
            (SPEED.parse().expect("valid attribute name"), AttributeNameKey::from(7)),
            (
                "https://example.com/temperature".parse().expect("valid attribute name"),
                AttributeNameKey::from(12),
            ),
            ("https://example.com/address".parse().expect("valid attribute name"), AttributeNameKey::from(15)),
            (
                "https://example.com/parkingTickets".parse().expect("valid attribute name"),
                AttributeNameKey::from(16),
            ),
            ("https://example.com/o'brien".parse().expect("valid attribute name"), AttributeNameKey::from(17)),
        ])
    }

    fn render_condition(condition: Condition) -> String {
        let (sql, _) = Query::select()
            .column(Entities::Id)
            .from(Entities::Table)
            .cond_where(condition)
            .build(PostgresQueryBuilder);
        sql
    }

    fn history_over(interval: TimeInterval) -> HistoryWindow {
        HistoryWindow::builder()
            .schema(None)
            .temporal_entity_key(SeaExpr::col((Alias::new("e"), Alias::new("temporal_entity_key"))))
            .axis(AttributeHistory::ModifiedAt)
            .interval(interval)
            .build()
    }

    fn render(q: &str) -> String {
        let expr = parser::parse(q).expect("parse q");
        render_condition(compile(&expr, &InstanceScan::History(&history_over(TimeInterval::Unbounded)), &names()))
    }

    /// Clause 4.9: a `q` on the system sub-attribute `observedAt` reaches its
    /// value node directly. It is stored bare (`{"@type":…,"@value":…}`), so
    /// the jsonpath from each instance must terminate at `@value` with no
    /// `hasValue` carrier hop.
    #[test]
    fn sub_attribute_predicate_falls_back_to_jsonpath() {
        let sql = render("\"https://example.com/temperature\".\"https://uri.etsi.org/ngsi-ld/observedAt\">=\"2024-01-01T00:00:00Z\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(
            sql.contains("'$[0].\"https://example.com/temperature\"[*].\"https://uri.etsi.org/ngsi-ld/observedAt\"[*]'"),
            "every instance of the system sub-attribute is read: {sql}"
        );
        assert!(sql.contains("'$.\"@value\"'"), "a system sub-attribute reaches @value directly: {sql}");
        assert!(
            !sql.contains("'$.\"https://uri.etsi.org/ngsi-ld/hasValue\""),
            "no hasValue hop is inserted: {sql}"
        );
    }

    /// Clause 4.9 makes the target value any Value of the instances when no
    /// datasetId is addressed, so a sub-attribute term is decided per instance
    /// inside one existence test rather than against the first instance alone.
    #[test]
    fn a_sub_attribute_term_holds_when_some_instance_passes() {
        let sql = render("\"https://example.com/temperature\".\"https://example.com/accuracy\"<0.2");
        assert!(
            sql.contains(r#"'$[0]."https://example.com/temperature"[*]."https://example.com/accuracy"[*]') AS "attribute_instance""#),
            "{sql}"
        );
        assert!(
            sql.contains(r#"jsonb_path_query_first("attribute_instance"."attribute_instance", '$."https://uri.etsi.org/ngsi-ld/hasValue"[0]."@value"')"#),
            "{sql}"
        );
        assert!(
            !sql.contains("[0].\"https://example.com/accuracy\""),
            "no hop is pinned to the first instance: {sql}"
        );
    }

    /// Clause 4.9 gives `attr[member]` two readings — a compound value member,
    /// and the target language of a `LanguageProperty` — and the query text
    /// cannot tell them apart, so both are compiled and disjoined. This is the
    /// compound half: the document jsonpath still carries the member key.
    #[test]
    fn compound_value_path_falls_back_to_jsonpath_with_terminal_key() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(sql.contains("jsonb_path_query_first"), "{sql}");
        assert!(sql.contains(".\\\"city\\\"") || sql.contains(".\"city\""), "{sql}");
        assert!(
            sql.contains("jsonb_build_object('@language'"),
            "the language-map reading is compiled beside it: {sql}"
        );
        assert!(sql.contains(" OR "), "{sql}");
    }

    /// A history instance keeps its sub-attributes in its document alone: the
    /// Property-value reading and the `languageMap` reading of a
    /// `LanguageProperty` sub-attribute (clause 4.9) both read it.
    /// Clause 4.9 EXAMPLE 11 reads the member of a `JsonProperty`'s raw JSON,
    /// which the expanded-value reading never reaches.
    #[test]
    fn a_trailing_path_also_reads_the_member_of_a_json_property() {
        let json_member = "$.\"https://uri.etsi.org/ngsi-ld/hasJSON\"[0].\"@value\".\"value\"";
        for expression in [
            "\"https://example.com/parkingTickets\"[value]==\"Overstay 60 minutes\"",
            "\"https://example.com/parkingTickets\"[value]>=10",
            "\"https://example.com/parkingTickets\"[value]~=\"Over.*\"",
            "\"https://example.com/parkingTickets\"[value]",
        ] {
            let sql = render(expression);
            assert!(sql.contains(json_member), "{expression}: {sql}");
        }
    }

    #[test]
    fn a_path_without_trailing_members_never_reads_a_json_property() {
        let sql = render("\"https://example.com/speed\"==10");
        assert!(!sql.contains("hasJSON"), "{sql}");
    }

    #[test]
    fn a_sub_attribute_path_reads_the_rebuilt_document_alone() {
        let sql = render("\"https://example.com/address\".\"https://example.com/sub\"==\"a\"");
        assert!(sql.contains("hasValue"), "{sql}");
        assert!(sql.contains("hasLanguageMap"), "{sql}");
    }

    /// The member hop precedes the `@value` selector, because JSON-LD expansion
    /// of a structured Property value produces IRI-keyed member nodes rather than
    /// one `@value` holding a raw object. The whole jsonpath is pinned: a
    /// `contains` assertion cannot see the two selectors in the wrong order.
    #[test]
    fn a_compound_value_predicate_hops_the_member_before_selecting_its_value() {
        let sql = render("\"https://example.com/address\"[city]==\"Berlin\"");
        assert!(sql.contains("'$.\"https://uri.etsi.org/ngsi-ld/hasValue\"[0].\"city\"[*].\"@value\"'"), "{sql}");
        assert!(
            !sql.contains("hasValue\"[0].\"@value\".\"city\""),
            "the member must not be read out of the value node: {sql}"
        );
    }

    #[test]
    fn existence_with_a_sub_path_tests_the_whole_path() {
        let sql = render("\"https://example.com/address\".\"https://example.com/city\"");
        assert!(sql.contains("jsonb_path_exists"), "{sql}");
        assert!(sql.contains("city"), "the sub-path must not be silently dropped: {sql}");
    }

    /// Clause 4.9 requires a Relationship operand to non-match every operator
    /// other than equality and inequality, so the document reading must not reach
    /// `hasObject` under an ordering or pattern operator.
    #[test]
    fn ordering_and_pattern_predicates_do_not_read_a_relationship_object() {
        for expression in [
            "\"https://example.com/address\".\"https://example.com/sub\">\"a\"",
            "\"https://example.com/address\".\"https://example.com/sub\"~=\"^a\"",
            "\"https://example.com/address\".\"https://example.com/sub\"!~=\"^a\"",
        ] {
            let sql = render(expression);
            assert!(!sql.contains("hasObject\"[0].\"@id\""), "{expression}: {sql}");
            assert!(sql.contains("hasValue\"[0].\"@value\""), "{expression}: {sql}");
        }
    }

    /// Clause 4.9 recommends the UCA for ordering text strings, so a document
    /// String ordering and both String bounds of a range collate under the ICU
    /// root collation, and a String equality does not.
    #[test]
    fn a_document_string_ordering_and_range_collate_under_the_icu_root_collation() {
        for expression in [
            "\"https://example.com/address\".\"https://example.com/sub\">\"a\"",
            "\"https://example.com/address\".\"https://example.com/sub\"==\"a\"..\"m\"",
            "\"https://example.com/address\".\"https://example.com/sub\"!=\"a\"..\"m\"",
        ] {
            let sql = render(expression);
            assert!(sql.contains(r#"COLLATE "und-x-icu""#), "{expression}: {sql}");
        }
        let sql = render("\"https://example.com/address\".\"https://example.com/sub\"==\"a\"");
        assert!(!sql.contains(r#"COLLATE "und-x-icu""#), "{sql}");
    }

    #[test]
    fn an_equality_predicate_still_reads_a_relationship_object() {
        let sql = render("\"https://example.com/address\".\"https://example.com/sub\"==\"a\"");
        assert!(sql.contains("hasObject"), "clause 4.9 admits a Relationship object under equality: {sql}");
        assert!(sql.contains("COALESCE"), "{sql}");
    }

    #[test]
    fn a_value_list_still_covers_relationship_objects_in_both_polarities() {
        // Every arm of a value
        // list is an equality comparison, so each one may match a Relationship's
        // object, not only a Property's value. A sub-attribute path forces the
        // document reading.
        for expression in [
            "\"https://example.com/address\".\"https://example.com/sub\"==\"a\",\"b\"",
            "\"https://example.com/address\".\"https://example.com/sub\"!=\"a\",\"b\"",
        ] {
            let sql = render(expression);
            assert!(sql.contains("hasObject\"[0].\"@id\""), "{expression}: {sql}");
            assert_eq!(
                sql.matches("COALESCE(jsonb_path_query_first(\"attribute_instance\"").count(),
                2,
                "{expression}: one per list member: {sql}"
            );
        }
    }

    /// A Query Term only a jsonpath answers is checked against every instance
    /// valid within the window, so the rebuilt instance document is read in
    /// the probe over the instances inside it and in the probe over the
    /// instance in effect when it opens (clause 4.11, Annex C clause C.5.5).
    #[test]
    fn a_document_condition_over_history_probes_both_window_and_in_effect_instances() {
        let start = DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        let window = history_over(TimeInterval::After(start));
        let address = "https://example.com/address".parse::<AttributeName>().expect("a valid attribute name");
        let sql = render_condition(on_history_document(&window, &address, &names(), |document| {
            Condition::all().add(SeaExpr::expr(document).is_not_null())
        }));
        assert_eq!(sql.matches("EXISTS").count(), 2, "{sql}");
        assert_eq!(
            sql.matches("jsonb_build_array(jsonb_build_object('https://example.com/address'").count(),
            2,
            "{sql}"
        );
        assert!(sql.contains("JOIN LATERAL"), "{sql}");
    }

    #[test]
    fn a_document_reading_escapes_apostrophes_in_attribute_iris() {
        let path = AttrPath {
            attr: "https://example.com/o'brien".parse().expect("valid attribute name"),
            sub_attrs: vec!["https://example.com/sub".parse().expect("valid attribute name")],
            value_path: Vec::new(),
            value_path_compact: Vec::new(),
            match_all_languages: false,
        };
        let expr = Expr::Pred(Predicate::Equality {
            path: QueryPath::Local(path),
            membership: SetMembership::Within,
            rhs: CompareValue::Scalar(Scalar::Str("x".to_string())),
        });
        let condition = compile(&expr, &InstanceScan::History(&history_over(TimeInterval::Unbounded)), &names());
        let sql = render_condition(condition);
        assert!(sql.contains("o''brien"), "apostrophe must be doubled inside the SQL literal: {sql}");
    }
}
