//! The strings of a `LanguageProperty`'s `languageMap`, read as the clause 4.9
//! target values a Query Term compares.
//!
//! Clause 4.9 gives a `LanguageProperty` two target values. With no target
//! language it is "a value from any of the key-value pairs held within the
//! languageMap"; with one, "the Value associated to the matching key-value pair
//! … where the key matches the target language", and a language holding several
//! strings matches "as a single element from the array of values". Either way
//! the target value is one String of the map, so every operator — ordering,
//! pattern and range included — is decided string by string: some string has to
//! satisfy `==`, `>`, `~=` or a range, and `!=`, `!~=` and an excluded range
//! hold when the map carries a string under the selection and none satisfies
//! the positive form ("No matching value is found").
//!
//! The expanded map is an array of `{"@language": …, "@value": …}` entries
//! (`@language` absent on the `@none` entry), held in `value_object` for a
//! top-level attribute and under `hasLanguageMap` in the expanded document for
//! a sub-attribute. Each entry is one string, so the map is unnested and every
//! comparison runs against one entry's `@value` text.

use crate::q_predicate::{instance_scan::InstanceScan, value_column::ValueColumn};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::filter::path::AttrPath;
use andromeda_ngsi_ld_vocabulary::expanded::properties::LANGUAGE_MAP;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::{escape_jsonpath_key, jsonb_array_elements, jsonb_path_query_array},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Query, SelectStatement, SimpleExpr};
use std::iter::once;

/// The alias one unnested `languageMap` entry is read under.
const ENTRY: &str = "ls";

/// The output column of `jsonb_array_elements`.
const ENTRY_VALUE: &str = "value";

/// The JSON-LD keyword an entry carries its language tag under.
const LANGUAGE_KEYWORD: &str = "@language";

/// The JSON-LD keyword an entry carries its string under.
const VALUE_KEYWORD: &str = "@value";

/// Which strings of a `languageMap` a Query Term reads (clause 4.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSelection<'a> {
    /// No target language: every string of the map, the `@none` one included.
    AnyLanguage,
    /// A target language, compared with the `@language` of each entry as the
    /// client wrote it: a BCP 47 tag is stored verbatim, never expanded.
    TargetLanguage(&'a AttributeName),
}

/// The expanded `languageMap` of an attribute: a JSONB array of entries, or SQL
/// `NULL` (no entries at all) where the attribute is not a `LanguageProperty`.
#[derive(Debug, Clone)]
pub struct LanguageMap(SimpleExpr);

impl LanguageMap {
    /// The map the scanned instance row holds: its `value_object` when the row
    /// was written as a `LanguageProperty`.
    ///
    /// The family is tested inside a `CASE` rather than beside the probe: the
    /// conjuncts of a `WHERE` are evaluated in whatever order the planner
    /// picks, and unnesting the object a structured Property holds in the same
    /// column would raise an error rather than yield no rows.
    #[must_use]
    pub fn of_instance(scan: &InstanceScan<'_>) -> LanguageMap {
        let language_property = int_literal(i64::from(AttributeKind::LanguageProperty.code()));
        let map = CaseStatement::new().case(scan.attribute_family().eq(language_property), scan.value(ValueColumn::Object));
        LanguageMap(map.into())
    }

    /// Every map the sub-attribute `path` addresses holds in an expanded
    /// `document`, across every instance of the attribute and of each
    /// sub-attribute on the way (clause 4.9: "the target value shall be any
    /// Value of such instances").
    ///
    /// Only a `LanguageProperty` carries `hasLanguageMap`, so the path selects
    /// nothing under any other family.
    #[must_use]
    pub fn of_document(path: &AttrPath, document: SimpleExpr) -> LanguageMap {
        LanguageMap(jsonb_path_query_array(document, &language_map_jsonpath(path)))
    }

    /// A map held by an arbitrary expression, for tests that need no scan.
    #[cfg(test)]
    pub(crate) const fn of_column(map: SimpleExpr) -> LanguageMap {
        LanguageMap(map)
    }

    /// Whether the map holds a string under `selection`, which is what clause
    /// 4.9 makes a language-selected target element exist.
    #[must_use]
    pub fn holds(&self, selection: LanguageSelection<'_>) -> SimpleExpr {
        SeaExpr::exists(self.entries(selection, None))
    }

    /// Whether some string under `selection` passes `test`, which reads the
    /// string's text.
    #[must_use]
    pub fn some_string(&self, selection: LanguageSelection<'_>, test: impl FnOnce(SimpleExpr) -> SimpleExpr) -> SimpleExpr {
        SeaExpr::exists(self.entries(selection, Some(test(entry_text(VALUE_KEYWORD)))))
    }

    /// Whether the map holds a string under `selection` and none of them
    /// passes `test`: the complement clause 4.9 gives `!=`, `!~=` and an
    /// excluded range over a language map.
    #[must_use]
    pub fn no_string(&self, selection: LanguageSelection<'_>, test: impl FnOnce(SimpleExpr) -> SimpleExpr) -> SimpleExpr {
        Condition::all()
            .add(self.holds(selection))
            .add(Condition::all().add(self.some_string(selection, test)).not())
            .into()
    }

    /// The entries under `selection`, narrowed by `test` when there is one.
    fn entries(&self, selection: LanguageSelection<'_>, test: Option<SimpleExpr>) -> SelectStatement {
        let mut scan = Query::select();
        // clone: the map is read by every probe a term builds over it
        scan.expr(int_literal(1)).from_function(jsonb_array_elements(self.0.clone()), Alias::new(ENTRY));
        if let LanguageSelection::TargetLanguage(language) = selection {
            scan.and_where(entry_text(LANGUAGE_KEYWORD).eq(cast(SeaExpr::val(language.as_str()), PostgresType::Text)));
        }
        if let Some(test) = test {
            scan.and_where(test);
        }
        scan
    }
}

/// One member of the current entry, as text: `"ls"."value" ->> '<keyword>'`.
fn entry_text(keyword: &str) -> SimpleExpr {
    SeaExpr::col((Alias::new(ENTRY), Alias::new(ENTRY_VALUE))).json_get_text(text_literal(keyword))
}

/// The jsonpath reaching every `languageMap` entry of the sub-attribute a path
/// addresses: `$[0]."<attr>"[*]."<sub>"[*]…."hasLanguageMap"[*]`.
fn language_map_jsonpath(path: &AttrPath) -> String {
    let mut jsonpath = String::from("$[0]");
    for name in once(&path.attr).chain(&path.sub_attrs) {
        jsonpath.push_str(".\"");
        jsonpath.push_str(&escape_jsonpath_key(name.as_str()));
        jsonpath.push_str("\"[*]");
    }
    jsonpath.push_str(".\"");
    jsonpath.push_str(&escape_jsonpath_key(LANGUAGE_MAP.as_str()));
    jsonpath.push_str("\"[*]");
    jsonpath
}

#[cfg(test)]
mod tests {
    use crate::{
        entity_row_refs::EntityRowRefs,
        q_predicate::{
            instance_scan::InstanceScan,
            language_map::{LanguageMap, LanguageSelection},
        },
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::filter::{expression::Expr, parser::parse, path::QueryPath};
    use andromeda_postgresql_seaquery::literal::text_literal;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Expr as SeaExpr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr, Value};

    fn render(expr: SimpleExpr) -> (String, Vec<Value>) {
        let (sql, values) = Query::select().expr(expr).build(PostgresQueryBuilder);
        (sql, values.0)
    }

    fn column_map() -> LanguageMap {
        LanguageMap::of_column(SeaExpr::col(Alias::new("map")))
    }

    fn language(tag: &str) -> AttributeName {
        tag.parse().expect("a valid language tag")
    }

    fn equals_door(text: SimpleExpr) -> SimpleExpr {
        text.eq(text_literal("Door"))
    }

    /// Only a row written as a `LanguageProperty` offers its `value_object` as
    /// a map; any other family's object is never unnested.
    #[test]
    fn an_instance_row_offers_its_object_only_as_a_language_property() {
        let rows = EntityRowRefs::unaliased(None);
        let (sql, _) = render(LanguageMap::of_instance(&InstanceScan::Current(&rows)).holds(LanguageSelection::AnyLanguage));
        assert!(
            sql.contains(r#"jsonb_array_elements((CASE WHEN ("cav"."attribute_kind" = (5)) THEN "cav"."value_object" END)) AS "ls""#),
            "{sql}"
        );
    }

    /// Clause 4.9: with a target language the target value is the string of
    /// that language's key-value pair, so the entry's tag is pinned.
    #[test]
    fn a_target_language_pins_the_language_of_the_string_it_reads() {
        let tag = language("en");
        let (sql, binds) = render(column_map().some_string(LanguageSelection::TargetLanguage(&tag), equals_door));
        assert!(sql.contains(r#"("ls"."value" ->> ('@language')) = ($1::text)"#), "{sql}");
        assert!(sql.contains(r#"("ls"."value" ->> ('@value')) = ('Door')"#), "{sql}");
        assert!(matches!(&binds[0], Value::String(Some(text)) if text.as_str() == "en"), "{binds:?}");
    }

    /// With no target language every entry is a candidate, the `@none` one
    /// included, so no tag is tested.
    #[test]
    fn no_target_language_reads_every_string() {
        let (sql, _) = render(column_map().some_string(LanguageSelection::AnyLanguage, equals_door));
        assert!(!sql.contains("@language"), "{sql}");
    }

    /// Clause 4.9 requires the target element: the complement holds only over
    /// a map carrying a string under the selection.
    #[test]
    fn no_string_requires_a_string_and_denies_every_match() {
        let tag = language("fr");
        let (sql, _) = render(column_map().no_string(LanguageSelection::TargetLanguage(&tag), equals_door));
        assert_eq!(sql.matches("EXISTS").count(), 2, "{sql}");
        assert!(sql.contains("NOT EXISTS"), "{sql}");
    }

    /// A sub-attribute's maps are read from every instance of the attribute
    /// and of the sub-attribute, under the `hasLanguageMap` carrier.
    #[test]
    fn a_document_reads_the_maps_under_every_instance_of_the_path() {
        let expr = parse("\"https://example.com/door\".\"https://example.com/label\"").expect("parses");
        let Expr::Pred(pred) = expr else {
            panic!("a single predicate");
        };
        let QueryPath::Local(path) = pred.path() else {
            panic!("a local path");
        };
        let (sql, _) = render(LanguageMap::of_document(path, SeaExpr::col(Alias::new("payload"))).holds(LanguageSelection::AnyLanguage));
        assert!(
            sql.contains(
                r#"jsonb_path_query_array("payload", '$[0]."https://example.com/door"[*]."https://example.com/label"[*]."https://uri.etsi.org/ngsi-ld/hasLanguageMap"[*]')"#
            ),
            "{sql}"
        );
    }
}
