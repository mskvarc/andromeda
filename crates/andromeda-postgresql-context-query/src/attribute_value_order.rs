//! Ordering entities by an attribute's value (clause 4.23.1).
//!
//! Clause 4.23.1 sorts "by target value or target object", and says that "the
//! preferred sort order for numbers and datetimes is trivial, however for
//! strings the default collation order shall be defined as using ICU `root`
//! collation order". Each of those three is a key here, applied in the order
//! clause 4.23.2 ranks the datatypes in.
//!
//! Which datatype a value has is not decided here. It is one question with one
//! answer across the broker, and
//! [`attribute_value_datatype`](crate::attribute_value_datatype) is where that
//! answer lives; the lateral projects it once per entity and every key below
//! reads the projection rather than re-deriving it from the payload.
//!
//! Every key reads one lateral projection of the attribute's value node, so the
//! jsonpath that reaches into the payload is evaluated once per entity however
//! many keys the term contributes.

use crate::attribute_value_datatype::{ARRAY_TYPE, ID_MEMBER, LIST_MEMBER, NUMBER_TYPE, VALUE_MEMBER, datatype_rank, datatype_token};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
use andromeda_ngsi_ld_vocabulary::{
    expanded::classes::{DATE_TIME, TIME},
    prefix::NGSI_LD,
};
use andromeda_postgresql_query::temporal_operand::{stored_date_time, stored_time};
use andromeda_postgresql_seaquery::{
    collation::{PostgresCollation, collate},
    jsonb::{escape_jsonpath_key, jsonb_array_length, jsonb_path_query_first, jsonb_typeof},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
};
use sea_query::{Alias, CaseStatement, Expr as SeaExpr, ExprTrait, Func, Query, SelectStatement, SimpleExpr};

/// The lateral projection one ordering term reads its attribute value from.
///
/// Each term gets its own, named after the position it holds in the `orderBy`
/// list, so a query that sorts on two attributes nests two projections without
/// either capturing the other's rows.
#[derive(Debug, Clone)]
pub struct AttributeValue {
    /// The name the enclosing statement knows the lateral by.
    alias: Alias,
}

impl AttributeValue {
    /// The projection an ordering term in `position` of the `orderBy` list
    /// reads.
    #[must_use]
    pub fn at(position: usize) -> AttributeValue {
        let mut alias = String::from(LATERAL_ALIAS_PREFIX);
        alias.push_str(&position.to_string());
        AttributeValue { alias: Alias::new(alias) }
    }

    /// The name the enclosing statement joins the projection under.
    #[must_use]
    pub fn alias(&self) -> Alias {
        // clone: sea-query takes the alias by value at the join site and every
        // key expression names it again afterwards.
        self.alias.clone()
    }

    /// The lateral body: one entity's value node for `attribute`, and the
    /// clause 4.23.2 datatype that node holds.
    ///
    /// The carrier is found by prefix rather than by name. Clause 5.2.5 gives
    /// each attribute family its own value member — `hasValue`, `hasObject`,
    /// `hasLanguageMap`, `hasVocab`, `hasValueList`, `hasObjectList` — and
    /// clause 4.23.1 orders by whichever of them the attribute happens to
    /// carry, so the jsonpath selects the one member of the attribute node
    /// whose key opens with the prefix they share.
    ///
    /// The datatype is projected rather than left to the keys: it is read by the
    /// rank and by three keys, and deriving it four times over would let the
    /// rank and the key that breaks its ties be computed from two different
    /// readings of one value.
    #[must_use]
    pub fn lateral(&self, payload: SimpleExpr, attribute: &AttributeName) -> SelectStatement {
        let mut carrier = Query::select();
        carrier.expr_as(jsonb_path_query_first(payload, &value_carrier_jsonpath(attribute)), Alias::new(CARRIER_COLUMN));

        let value_node = carrier_column().json_get(int_literal(FIRST_VALUE_NODE));
        let arity = carrier_arity();

        let mut projection = Query::select();
        projection
            .from_subquery(carrier.take(), Alias::new(CARRIER_ALIAS))
            .expr_as(datatype_token(&value_node, &arity), Alias::new(DATATYPE_COLUMN))
            .expr_as(value_node, Alias::new(VALUE_NODE_COLUMN));
        projection.take()
    }

    /// The clause-4.23.2 rank of this entity's value, `NULL` when the entity
    /// carries no such attribute.
    #[must_use]
    pub fn datatype_rank(&self, direction: SortDirection) -> SimpleExpr {
        datatype_rank(self.datatype(), direction)
    }

    /// The ordering key for numbers, `NULL` for every other datatype.
    ///
    /// Left as JSONB rather than cast: JSONB compares two numbers numerically,
    /// so `9` precedes `10` without the statement risking a cast over a value
    /// that turned out not to be a number after all.
    #[must_use]
    pub fn numeric_key(&self) -> SimpleExpr {
        CaseStatement::new().case(self.datatype().eq(text_literal(NUMBER_TYPE)), self.value()).into()
    }

    /// The ordering key for `DateTime` values, `NULL` for every other datatype.
    ///
    /// Clause 4.23.1 calls the sort order for datetimes trivial, which it is
    /// only once each side is resolved to an instant: clause 4.6.3 admits a
    /// fraction of any width, so two spellings of one moment order by their text
    /// otherwise.
    #[must_use]
    pub fn instant_key(&self) -> SimpleExpr {
        let date_time = DATE_TIME;
        CaseStatement::new()
            .case(self.datatype().eq(text_literal(date_time.as_str())), stored_date_time(self.value_text()))
            .into()
    }

    /// The collated ordering key for every datatype that compares as text.
    ///
    /// Strings compare as themselves, a Relationship's target object as its
    /// URI, and a `Date` as its canonical `YYYY-MM-DD` form, which is
    /// fixed-width and so already in chronological order. A `Time` is
    /// normalised to a single UTC spelling first, for the reason a `DateTime`
    /// is resolved to an instant.
    ///
    /// An ordered list carrier is read through its first element. Clause 4.23.2
    /// ranks every list under one bucket, Array, and says nothing about how two
    /// Arrays compare; the first element is what a multi-node array already
    /// orders by here, so both spellings of an array order the same way instead
    /// of every list tying on a NULL key.
    #[must_use]
    pub fn text_key(&self, collation: &PostgresCollation) -> SimpleExpr {
        let time = TIME;
        let comparable = CaseStatement::new()
            .case(self.datatype().eq(text_literal(time.as_str())), stored_time(self.value_text()))
            .finally(Func::coalesce([
                self.value_text(),
                self.object_text(),
                self.list_head_text(VALUE_MEMBER),
                self.list_head_text(ID_MEMBER),
            ]));
        collate(comparable.into(), collation)
    }

    /// The attribute's first value node, as the lateral projected it.
    fn value_node(&self) -> SimpleExpr {
        SeaExpr::col((self.alias(), Alias::new(VALUE_NODE_COLUMN)))
    }

    /// The clause 4.23.2 datatype of that value node, as the lateral projected
    /// it.
    fn datatype(&self) -> SimpleExpr {
        SeaExpr::col((self.alias(), Alias::new(DATATYPE_COLUMN)))
    }

    /// The value node's `@value` member, as JSONB.
    fn value(&self) -> SimpleExpr {
        self.value_node().json_get(text_literal(VALUE_MEMBER))
    }

    /// The value node's `@value` member, as text.
    fn value_text(&self) -> SimpleExpr {
        self.value_node().json_get_text(text_literal(VALUE_MEMBER))
    }

    /// The value node's `@id` member, as text: a Relationship's target object.
    fn object_text(&self) -> SimpleExpr {
        self.value_node().json_get_text(text_literal(ID_MEMBER))
    }

    /// One keyword of the first element of an ordered list carrier, as text.
    fn list_head_text(&self, keyword: &str) -> SimpleExpr {
        self.value_node()
            .json_get(text_literal(LIST_MEMBER))
            .json_get(int_literal(FIRST_VALUE_NODE))
            .json_get_text(text_literal(keyword))
    }
}

/// The prefix each ordering term's lateral projection is aliased with.
const LATERAL_ALIAS_PREFIX: &str = "order_value_";

/// The alias the lateral gives its inner carrier lookup.
const CARRIER_ALIAS: &str = "carrier";

/// The column the inner lookup projects the value-carrier array under.
const CARRIER_COLUMN: &str = "value_carrier";

/// The column the lateral projects the first value node under.
const VALUE_NODE_COLUMN: &str = "value_node";

/// The column the lateral projects that node's clause 4.23.2 datatype under.
const DATATYPE_COLUMN: &str = "value_datatype";

/// The index of the value node an ordering reads.
///
/// Clause 4.23.1 orders by one value per entity, and the expanded payload holds
/// a scalar value as a single node, so the ordering reads the first.
const FIRST_VALUE_NODE: i64 = 0;

/// The local name every NGSI-LD value carrier's IRI opens with.
const VALUE_CARRIER_LOCAL_PREFIX: &str = "has";

/// How many value nodes an attribute carries, `NULL` when its carrier is not an
/// array at all.
///
/// Guarded by a `CASE` rather than by a conjunct: `jsonb_array_length` raises on
/// a value that is not an array, and only a `CASE` guarantees the test is
/// evaluated before the call it guards.
fn carrier_arity() -> SimpleExpr {
    CaseStatement::new()
        .case(
            jsonb_typeof(carrier_column()).eq(text_literal(ARRAY_TYPE)),
            jsonb_array_length(carrier_column()),
        )
        .into()
}

/// The value-carrier array the lateral's inner lookup projected.
fn carrier_column() -> SimpleExpr {
    SeaExpr::col((Alias::new(CARRIER_ALIAS), Alias::new(CARRIER_COLUMN)))
}

/// The jsonpath reaching the value carrier of one attribute inside the expanded
/// payload: `$[0]."<attr>"[0].keyvalue() ? (@.key starts with "…has").value`.
///
/// The attribute node's members are walked as key/value pairs so the carrier is
/// selected by the prefix its key shares with every other carrier, rather than
/// by naming one family's member and missing the rest.
fn value_carrier_jsonpath(attribute: &AttributeName) -> String {
    let mut jsonpath = String::from("$[0].\"");
    jsonpath.push_str(&escape_jsonpath_key(attribute.as_str()));
    jsonpath.push_str("\"[0].keyvalue() ? (@.key starts with \"");
    jsonpath.push_str(&escape_jsonpath_key(NGSI_LD.as_str()));
    jsonpath.push_str(VALUE_CARRIER_LOCAL_PREFIX);
    jsonpath.push_str("\").value");
    jsonpath
}

#[cfg(test)]
mod tests {
    use crate::attribute_value_order::{AttributeValue, value_carrier_jsonpath};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_query::ordering::sort_direction::SortDirection;
    use andromeda_postgresql_seaquery::collation::PostgresCollation;
    use sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SimpleExpr};

    const SPEED: &str = "https://example.com/speed";

    fn speed() -> AttributeName {
        SPEED.parse().expect("a valid attribute name")
    }

    fn payload() -> SimpleExpr {
        Expr::col(Alias::new("payload"))
    }

    fn render(expr: SimpleExpr) -> String {
        Query::select().expr(expr).to_owned().to_string(PostgresQueryBuilder)
    }

    /// Clause 5.2.5 gives every attribute family its own value member, so the
    /// carrier is matched on the prefix they share instead of on one name.
    #[test]
    fn the_carrier_is_selected_by_the_prefix_every_value_member_shares() {
        let jsonpath = value_carrier_jsonpath(&speed());
        assert!(
            jsonpath.contains("keyvalue() ? (@.key starts with \"https://uri.etsi.org/ngsi-ld/has\")"),
            "{jsonpath}"
        );
        assert!(jsonpath.ends_with("\").value"), "the carrier's value is what the path selects: {jsonpath}");
        assert!(jsonpath.starts_with("$[0].\"https://example.com/speed\"[0]."), "{jsonpath}");
    }

    /// An attribute IRI carrying a double quote cannot break out of the
    /// jsonpath's own quoting.
    #[test]
    fn an_attribute_iri_cannot_break_out_of_the_jsonpath() {
        let quoted: AttributeName = "https://example.com/a\"b".parse().expect("a valid attribute name");
        let jsonpath = value_carrier_jsonpath(&quoted);
        assert!(jsonpath.contains("a\\\"b"), "{jsonpath}");
    }

    /// The lateral evaluates the payload lookup once and names both of the
    /// things every key reads: the value node, and the one datatype every key
    /// and the rank agree on.
    #[test]
    fn the_lateral_projects_the_value_node_and_its_datatype_from_one_lookup() {
        let sql = AttributeValue::at(0).lateral(payload(), &speed()).to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches("jsonb_path_query_first(").count(), 1, "{sql}");
        assert!(sql.contains("AS \"value_node\""), "{sql}");
        assert!(sql.contains("AS \"value_datatype\""), "{sql}");
        assert!(sql.contains("jsonb_array_length("), "{sql}");
    }

    /// Two ordering terms name two projections, so neither reads the other's
    /// value node.
    #[test]
    fn each_ordering_term_names_its_own_projection() {
        let first = render(AttributeValue::at(0).numeric_key());
        let second = render(AttributeValue::at(1).numeric_key());
        assert!(first.contains("\"order_value_0\".\"value_datatype\""), "{first}");
        assert!(second.contains("\"order_value_1\".\"value_datatype\""), "{second}");
    }

    /// The rank and the three keys read the one projected datatype, so the
    /// bucket an entity is placed in and the key that orders it inside that
    /// bucket cannot be computed from two different readings of one value.
    #[test]
    fn every_key_reads_the_one_projected_datatype() {
        let value = AttributeValue::at(0);
        for sql in [
            render(value.datatype_rank(SortDirection::Ascending)),
            render(value.numeric_key()),
            render(value.instant_key()),
            render(value.text_key(&PostgresCollation::new("und-x-icu"))),
        ] {
            assert!(sql.contains("\"order_value_0\".\"value_datatype\""), "{sql}");
        }
    }

    /// A number is compared as JSONB, which orders numbers numerically, rather
    /// than as the text `9` that would exceed `10`.
    #[test]
    fn the_numeric_key_compares_numbers_as_numbers() {
        let sql = render(AttributeValue::at(0).numeric_key());
        assert!(sql.contains("'number'"), "{sql}");
        assert!(!sql.contains("::numeric"), "no cast can see a value that is not a number: {sql}");
    }

    /// Clause 4.6.3 admits a fraction of any width up to six digits, so a
    /// `DateTime` is resolved to an instant before it is compared.
    #[test]
    fn the_instant_key_resolves_a_date_time_value_to_an_instant() {
        let sql = render(AttributeValue::at(0).instant_key());
        assert!(sql.contains("'https://uri.etsi.org/ngsi-ld/DateTime'"), "{sql}");
        assert!(sql.contains("::timestamptz"), "{sql}");
    }

    /// Clause 4.23.1: strings sort under the requested collation. The value is
    /// read out of the payload as text, because PostgreSQL collates no JSONB.
    #[test]
    fn the_text_key_is_collated_and_read_as_text() {
        let sql = render(AttributeValue::at(0).text_key(&PostgresCollation::new("und-x-icu")));
        assert!(sql.contains("COLLATE \"und-x-icu\""), "{sql}");
        assert!(sql.contains("->> ('@value')"), "{sql}");
        assert!(!sql.contains("::text"), "a jsonb text cast would keep the JSON quotes: {sql}");
    }

    /// Clause 4.23.1 sorts "by target value or target object", so a
    /// Relationship orders by the URI of the entity it points at.
    #[test]
    fn the_text_key_falls_back_to_a_relationships_target_object() {
        let sql = render(AttributeValue::at(0).text_key(&PostgresCollation::new("und-x-icu")));
        assert!(sql.contains("->> ('@id')"), "{sql}");
    }

    /// Clause 4.23.2 puts every list in one bucket and says nothing about how
    /// two Arrays compare, so they order by their first element rather than all
    /// tying on a key no list carrier can fill.
    #[test]
    fn the_text_key_reads_the_first_element_of_an_ordered_list() {
        let sql = render(AttributeValue::at(0).text_key(&PostgresCollation::new("und-x-icu")));
        assert!(sql.contains("-> ('@list')) -> (0)) ->> ('@value')"), "{sql}");
        assert!(
            sql.contains("-> ('@list')) -> (0)) ->> ('@id')"),
            "a list of objects orders by its first URI: {sql}"
        );
    }
}
