//! The strings of a `LanguageProperty`'s `languageMap`, read as the clause 4.9
//! target values a Query Term compares.
//!
//! Clause 4.9 gives a `LanguageProperty` two target values. With no target
//! language it is "a value from any of the key-value pairs held within the
//! languageMap"; with one, "the Value associated to the matching key-value pair
//! … where the key matches the target language", and a language holding several
//! strings matches "as a single element from the array of values". Either way
//! the target value is one String of the map, so every operator (ordering,
//! pattern and range included) is decided string by string: some string has to
//! satisfy `==`, `>`, `~=` or a range, and `!=`, `!~=` and an excluded range
//! hold when the map exists and no string under the selection satisfies the
//! positive form ("No matching value is found").
//!
//! The expanded map is an array of `{"@language": …, "@value": …}` entries
//! (`@language` absent on the `@none` entry), held in the `value_object` of a
//! current or history row and of an element. Each entry is one string, so the
//! map is unnested and every comparison runs against one entry's `@value`
//! text.

use crate::q_predicate::{extended_filter_pattern::extended_filter_pattern, instance_row::InstanceRow, value_column::ValueColumn};
use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, target_language::LanguageSelector};
use andromeda_ngsi_ld_vocabulary::keyword;
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_seaquery::{
    cast::cast,
    jsonb::jsonb_array_elements,
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
};
use sea_query::{Alias, CaseStatement, Condition, Expr as SeaExpr, ExprTrait, Func, Query, SelectStatement, SimpleExpr};

/// The alias one unnested `languageMap` entry is read under.
const ENTRY: &str = "ls";

/// The output column of `jsonb_array_elements`.
const ENTRY_VALUE: &str = "value";

/// Which strings of a `languageMap` a Query Term reads (clause 4.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageSelection<'a> {
    /// No target language: every string of the map, the `@none` one included.
    AnyLanguage,
    /// A target language, selecting the entries whose `@language` it admits
    /// under the matching mode: a tag compared case-insensitively (RFC 5646
    /// section 2.1.1), or RFC 4647 section 3.3.2 extended filtering.
    TargetLanguage(LanguageSelector<'a>, LanguageMatching),
}

/// The expanded `languageMap` of an attribute: a JSONB array of entries, or SQL
/// `NULL` (no entries at all) where the attribute is not a `LanguageProperty`.
#[derive(Debug, Clone)]
pub struct LanguageMap(SimpleExpr);

impl LanguageMap {
    /// The map an instance holds: its `value_object` when it was written as a
    /// `LanguageProperty`.
    ///
    /// The family is tested inside a `CASE` rather than beside the probe: the
    /// conjuncts of a `WHERE` are evaluated in whatever order the planner
    /// picks, and unnesting the object a structured Property holds in the same
    /// column would raise an error rather than yield no rows.
    #[must_use]
    pub fn of_instance(row: InstanceRow) -> LanguageMap {
        let language_property = int_literal(i64::from(AttributeKind::LanguageProperty.code()));
        let map = CaseStatement::new().case(row.attribute_family().eq(language_property), row.value(ValueColumn::Object));
        LanguageMap(map.into())
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
        SeaExpr::exists(self.entries(selection, Some(test(entry_text(keyword::VALUE)))))
    }

    /// Whether the map holds a string in any language and none under
    /// `selection` passes `test`: the complement clause 4.9 gives `!=` and
    /// `!~=`, whose matching entity contains the target element, the
    /// `LanguageProperty`, and where "No matching value is found as the value
    /// of the specified language key", which a language the map does not hold
    /// satisfies.
    #[must_use]
    pub fn no_string(&self, selection: LanguageSelection<'_>, test: impl FnOnce(SimpleExpr) -> SimpleExpr) -> SimpleExpr {
        Condition::all()
            .add(self.holds(LanguageSelection::AnyLanguage))
            .add(Condition::all().add(self.some_string(selection, test)).not())
            .into()
    }

    /// The entries under `selection`, narrowed by `test` when there is one.
    fn entries(&self, selection: LanguageSelection<'_>, test: Option<SimpleExpr>) -> SelectStatement {
        let mut scan = Query::select();
        // clone: the map is read by every probe a term builds over it
        scan.expr(int_literal(1)).from_function(jsonb_array_elements(self.0.clone()), Alias::new(ENTRY));
        if let LanguageSelection::TargetLanguage(language, matching) = selection {
            scan.and_where(selects_entry(language, matching));
        }
        if let Some(test) = test {
            scan.and_where(test);
        }
        scan
    }
}

/// Whether the current entry's `@language` is a key `language` selects under
/// `matching`, the same decision
/// [`LanguageSelector::admits`] makes in memory.
///
/// The target language is the client's own text, so it reaches the statement
/// as a parameter: the tag itself, or the pattern restating the range.
fn selects_entry(language: LanguageSelector<'_>, matching: LanguageMatching) -> SimpleExpr {
    let tag = entry_text(keyword::LANGUAGE);
    match (language, matching) {
        (LanguageSelector::Tag(wanted), LanguageMatching::TagEquality) => {
            SimpleExpr::from(Func::lower(tag)).eq(Func::lower(cast(SeaExpr::val(wanted.as_str()), PostgresType::Text)))
        }
        (LanguageSelector::Tag(_) | LanguageSelector::Range(_), LanguageMatching::ExtendedFiltering)
        | (LanguageSelector::Range(_), LanguageMatching::TagEquality) => {
            tag.posix_matches_ignoring_case(cast(SeaExpr::val(extended_filter_pattern(language)), PostgresType::Text))
        }
    }
}

/// One member of the current entry, as text: `"ls"."value" ->> '<keyword>'`.
fn entry_text(keyword: &str) -> SimpleExpr {
    SeaExpr::col((Alias::new(ENTRY), Alias::new(ENTRY_VALUE))).json_get_text(text_literal(keyword))
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::q_predicate::{
        instance_row::InstanceRow,
        language_map::{LanguageMap, LanguageSelection},
    };
    use andromeda_ngsi_ld_query::filter::{language_matching::LanguageMatching, target_language::TargetLanguage};
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

    fn language(text: &str) -> TargetLanguage {
        TargetLanguage::parse(text).expect("a valid target language")
    }

    fn equals_door(text: SimpleExpr) -> SimpleExpr {
        text.eq(text_literal("Door"))
    }

    /// Only a row written as a `LanguageProperty` offers its `value_object` as
    /// a map; any other family's object is never unnested.
    #[test]
    fn an_instance_row_offers_its_object_only_as_a_language_property() {
        let (sql, _) = render(LanguageMap::of_instance(InstanceRow::Current).holds(LanguageSelection::AnyLanguage));
        assert!(
            sql.contains(r#"jsonb_array_elements((CASE WHEN ("ca"."attribute_kind" = (5)) THEN "ca"."value_object" END)) AS "ls""#),
            "{sql}"
        );
    }

    /// Clause 4.9: with a target language the target value is the string of
    /// that language's key-value pair, the tag compared case-insensitively
    /// (RFC 5646 section 2.1.1).
    #[test]
    fn tag_equality_pins_the_language_of_the_string_it_reads_ignoring_case() {
        let tag = language("en-GB");
        let (sql, binds) = render(column_map().some_string(LanguageSelection::TargetLanguage(tag.selector(), LanguageMatching::TagEquality), equals_door));
        assert!(sql.contains(r#"LOWER("ls"."value" ->> ('@language')) = LOWER($1::text)"#), "{sql}");
        assert!(sql.contains(r#"("ls"."value" ->> ('@value')) = ('Door')"#), "{sql}");
        assert!(matches!(&binds[0], Value::String(Some(text)) if text.as_str() == "en-GB"), "{binds:?}");
    }

    /// RFC 4647 section 3.3.2: under extended filtering the entry's tag is
    /// matched against the range's pattern, bound as a parameter.
    #[test]
    fn extended_filtering_matches_the_tag_against_the_range_pattern() {
        for (text, matching) in [
            ("en", LanguageMatching::ExtendedFiltering),
            ("de-*-DE", LanguageMatching::ExtendedFiltering),
            ("de-*-DE", LanguageMatching::TagEquality),
        ] {
            let target = language(text);
            let (sql, binds) = render(column_map().holds(LanguageSelection::TargetLanguage(target.selector(), matching)));
            assert!(sql.contains(r#"("ls"."value" ->> ('@language')) ~* ($1::text)"#), "{text}: {sql}");
            assert!(!sql.contains("LOWER"), "{text}: {sql}");
            assert!(
                matches!(&binds[0], Value::String(Some(pattern)) if pattern.starts_with('^')),
                "{text}: {binds:?}"
            );
        }
    }

    /// With no target language every entry is a candidate, the `@none` one
    /// included, so no tag is tested.
    #[test]
    fn no_target_language_reads_every_string() {
        let (sql, _) = render(column_map().some_string(LanguageSelection::AnyLanguage, equals_door));
        assert!(!sql.contains("@language"), "{sql}");
    }

    /// Clause 4.9 `unequal` and `notPatternOp` require the `LanguageProperty`,
    /// not the target language: the presence probe tests no tag, the denial
    /// pins it.
    #[test]
    fn no_string_requires_the_map_and_denies_a_match_under_the_language() {
        let tag = language("fr");
        let (sql, binds) = render(column_map().no_string(LanguageSelection::TargetLanguage(tag.selector(), LanguageMatching::TagEquality), equals_door));
        assert_eq!(sql.matches("EXISTS").count(), 2, "{sql}");
        assert_eq!(sql.matches("@language").count(), 1, "{sql}");
        assert!(sql.contains("(NOT EXISTS("), "{sql}");
        assert!(matches!(&binds[0], Value::String(Some(text)) if text.as_str() == "fr"), "{binds:?}");
    }
}
