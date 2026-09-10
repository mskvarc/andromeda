//! Ordering entities by an attribute's value (clauses 4.23.1 and 4.23.2).
//!
//! Clause 4.23.1 sorts "by target value or target object"; clause 4.23.2 says
//! what happens when the entities matched hold different datatypes there, and
//! fixes one comparison order across all of them: Numbers, Strings, Object,
//! Array, Boolean, Time, Date, `DateTime`, Null, and last the entities that do
//! not carry the attribute at all.
//!
//! The order is rendered as a rank rather than as a chain of comparisons. The
//! datatype of a stored value is read out of the payload as a single token, and
//! the token's position in a nine-element sequence *is* its rank, so a
//! descending sort is the same expression over the reversed sequence and a
//! datatype the payload does not name ranks nowhere at all — which, sorted
//! `NULLS LAST`, is exactly where "attribute does not exist" belongs, in both
//! directions.
//!
//! Every key reads one lateral projection of the attribute's value node, so the
//! jsonpath that reaches into the payload is evaluated once per entity however
//! many keys the term contributes.

use crate::sort_direction::SortDirection;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::{
    expanded::classes::{DATE, DATE_TIME, TIME},
    prefix::NGSI_LD,
};
use andromeda_postgresql_query::temporal_operand::{stored_date_time, stored_time};
use andromeda_postgresql_seaquery::{
    cast::cast,
    collation::{PostgresCollation, collate},
    jsonb::{escape_jsonpath_key, jsonb_array_length, jsonb_key_exists, jsonb_path_query_first, jsonb_typeof},
    literal::{int_literal, text_literal},
    pg_array::{array_position, text_array},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
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

    /// The lateral body: one entity's value node for `attribute`, and how many
    /// value nodes the attribute carries.
    ///
    /// The carrier is found by prefix rather than by name. Clause 5.2.5 gives
    /// each attribute family its own value member — `hasValue`, `hasObject`,
    /// `hasLanguageMap`, `hasVocab`, `hasValueList`, `hasObjectList` — and
    /// clause 4.23.1 orders by whichever of them the attribute happens to
    /// carry, so the jsonpath selects the one member of the attribute node
    /// whose key opens with the prefix they share.
    #[must_use]
    pub fn lateral(&self, payload: SimpleExpr, attribute: &AttributeName) -> SelectStatement {
        let mut carrier = Query::select();
        carrier.expr_as(jsonb_path_query_first(payload, &value_carrier_jsonpath(attribute)), Alias::new(CARRIER_COLUMN));

        let mut projection = Query::select();
        projection
            .from_subquery(carrier.take(), Alias::new(CARRIER_ALIAS))
            .expr_as(carrier_column().json_get(int_literal(FIRST_VALUE_NODE)), Alias::new(VALUE_NODE_COLUMN))
            .expr_as(carrier_arity(), Alias::new(ARITY_COLUMN));
        projection.take()
    }

    /// The clause-4.23.2 rank of this entity's value, `NULL` when the entity
    /// carries no such attribute.
    ///
    /// Always sorted ascending, whichever way the term runs: the sequence the
    /// rank is read from is the one that already runs the term's way, which is
    /// what keeps Null and "attribute does not exist" at the end of a
    /// descending sort instead of moving them to the front.
    #[must_use]
    pub fn datatype_rank(&self, direction: SortDirection) -> SimpleExpr {
        let (time, date, date_time) = (TIME, DATE, DATE_TIME);
        let sequence: [&str; DATATYPE_COUNT] = match direction {
            SortDirection::Ascending => [
                NUMBER_TYPE,
                STRING_TYPE,
                OBJECT_TYPE,
                ARRAY_TYPE,
                BOOLEAN_TYPE,
                time.as_str(),
                date.as_str(),
                date_time.as_str(),
                NULL_TYPE,
            ],
            SortDirection::Descending => [
                date_time.as_str(),
                date.as_str(),
                time.as_str(),
                BOOLEAN_TYPE,
                ARRAY_TYPE,
                OBJECT_TYPE,
                STRING_TYPE,
                NUMBER_TYPE,
                NULL_TYPE,
            ],
        };
        array_position(cast(text_array(&sequence), PostgresType::TextArray), self.datatype_token())
    }

    /// The ordering key for numbers, `NULL` for every other datatype.
    ///
    /// Left as JSONB rather than cast: JSONB compares two numbers numerically,
    /// so `9` precedes `10` without the statement risking a cast over a value
    /// that turned out not to be a number after all.
    #[must_use]
    pub fn numeric_key(&self) -> SimpleExpr {
        CaseStatement::new()
            .case(jsonb_typeof(self.value()).eq(text_literal(NUMBER_TYPE)), self.value())
            .into()
    }

    /// The ordering key for `DateTime` values, `NULL` for every other datatype.
    ///
    /// Clause 4.23.1 calls the sort order for datetimes trivial, which it is
    /// only once each side is resolved to an instant: clause 4.6.3 admits any
    /// offset, so two spellings of one moment order by their text otherwise.
    #[must_use]
    pub fn instant_key(&self) -> SimpleExpr {
        let date_time = DATE_TIME;
        CaseStatement::new()
            .case(self.value_type().eq(text_literal(date_time.as_str())), stored_date_time(self.value_text()))
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
            .case(self.value_type().eq(text_literal(time.as_str())), stored_time(self.value_text()))
            .finally(Func::coalesce([
                self.value_text(),
                self.object_text(),
                self.list_head_text(VALUE_MEMBER),
                self.list_head_text(ID_MEMBER),
            ]));
        collate(comparable.into(), collation)
    }

    /// The one token that names this value's clause-4.23.2 datatype.
    ///
    /// Read in the order the payload distinguishes the datatypes in. More than
    /// one value node means the attribute's value was a JSON array, whatever
    /// the nodes hold, and so does a node carrying `@list`: clause 4.9 defines
    /// the target value of a `ListProperty` as "the valueList array associated
    /// to such a `ListProperty`" and that of a `ListRelationship` as "the array
    /// of objects" it names, which clause 4.23.2 ranks under Array. Expansion
    /// wraps both in one `@list` node, so their arity is 1 and only the keyword
    /// tells them apart from a scalar — without this reading every list fell
    /// through to Object and, carrying neither `@value` nor `@id`, tied with
    /// every other list on a NULL key.
    ///
    /// A node carrying `@value` names its datatype through JSONB, except that a
    /// string may be a `Time`, a `Date` or a `DateTime` wearing its NGSI-LD
    /// `@type`. A node carrying `@id` is a Relationship's target object, which
    /// is a URI and compares as a string. Anything else that is a node at all is
    /// an expanded structured value: an Object.
    fn datatype_token(&self) -> SimpleExpr {
        let (time, date, date_time) = (TIME, DATE, DATE_TIME);
        let from_arity = CaseStatement::new().case(self.arity().ne(int_literal(SINGLE_VALUE_NODE)), text_literal(ARRAY_TYPE));
        let from_list = CaseStatement::new().case(jsonb_key_exists(self.value_node(), text_literal(LIST_MEMBER)), text_literal(ARRAY_TYPE));
        let from_json_type = CaseStatement::new().case(jsonb_typeof(self.value()).ne(text_literal(STRING_TYPE)), jsonb_typeof(self.value()));
        let from_ngsi_ld_type = CaseStatement::new().case(
            self.value_type()
                .is_in([text_literal(time.as_str()), text_literal(date.as_str()), text_literal(date_time.as_str())]),
            self.value_type(),
        );
        let from_string_value = CaseStatement::new().case(jsonb_key_exists(self.value_node(), text_literal(VALUE_MEMBER)), text_literal(STRING_TYPE));
        let from_target_object = CaseStatement::new().case(jsonb_key_exists(self.value_node(), text_literal(ID_MEMBER)), text_literal(STRING_TYPE));
        let from_structure = CaseStatement::new().case(self.value_node().is_not_null(), text_literal(OBJECT_TYPE));
        let candidates: [SimpleExpr; DATATYPE_SOURCE_COUNT] = [
            from_arity.into(),
            from_list.into(),
            from_json_type.into(),
            from_ngsi_ld_type.into(),
            from_string_value.into(),
            from_target_object.into(),
            from_structure.into(),
        ];
        Func::coalesce(candidates).into()
    }

    /// The attribute's first value node, as the lateral projected it.
    fn value_node(&self) -> SimpleExpr {
        SeaExpr::col((self.alias(), Alias::new(VALUE_NODE_COLUMN)))
    }

    /// How many value nodes the attribute carries, `NULL` when it carries none.
    fn arity(&self) -> SimpleExpr {
        SeaExpr::col((self.alias(), Alias::new(ARITY_COLUMN)))
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

    /// The value node's `@type` member, as text: the NGSI-LD datatype IRI a
    /// temporal literal carries.
    fn value_type(&self) -> SimpleExpr {
        self.value_node().json_get_text(text_literal(TYPE_MEMBER))
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

/// The column the lateral projects the value-node count under.
const ARITY_COLUMN: &str = "value_arity";

/// The index of the value node an ordering reads.
///
/// Clause 4.23.1 orders by one value per entity, and the expanded payload holds
/// a scalar value as a single node, so the ordering reads the first.
const FIRST_VALUE_NODE: i64 = 0;

/// The value-node count a scalar value has.
const SINGLE_VALUE_NODE: i64 = 1;

/// The local name every NGSI-LD value carrier's IRI opens with.
const VALUE_CARRIER_LOCAL_PREFIX: &str = "has";

/// The JSON-LD keyword holding a value node's literal value.
const VALUE_MEMBER: &str = "@value";

/// The JSON-LD keyword holding a node's identifier.
const ID_MEMBER: &str = "@id";

/// The JSON-LD keyword holding a value node's datatype IRI.
const TYPE_MEMBER: &str = "@type";

/// The JSON-LD keyword holding the elements of an ordered list carrier.
///
/// Clause 5.2.24 and 5.2.25 give the two list families an ordered value, which
/// JSON-LD expansion wraps in a single node under this keyword rather than
/// spreading over one node per element.
const LIST_MEMBER: &str = "@list";

/// The `jsonb_typeof` token for a JSON number.
const NUMBER_TYPE: &str = "number";

/// The `jsonb_typeof` token for a JSON string.
const STRING_TYPE: &str = "string";

/// The `jsonb_typeof` token for a JSON object.
const OBJECT_TYPE: &str = "object";

/// The `jsonb_typeof` token for a JSON array.
const ARRAY_TYPE: &str = "array";

/// The `jsonb_typeof` token for a JSON boolean.
const BOOLEAN_TYPE: &str = "boolean";

/// The `jsonb_typeof` token for a JSON null.
const NULL_TYPE: &str = "null";

/// How many datatypes clause 4.23.2 ranks before "attribute does not exist".
const DATATYPE_COUNT: usize = 9;

/// How many readings of the payload can name a value's datatype.
const DATATYPE_SOURCE_COUNT: usize = 7;

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
    use crate::{
        attribute_value_order::{AttributeValue, value_carrier_jsonpath},
        sort_direction::SortDirection,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
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
    /// things every key reads.
    #[test]
    fn the_lateral_projects_the_value_node_and_its_arity_from_one_lookup() {
        let sql = AttributeValue::at(0).lateral(payload(), &speed()).to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches("jsonb_path_query_first(").count(), 1, "{sql}");
        assert!(sql.contains("AS \"value_node\""), "{sql}");
        assert!(sql.contains("AS \"value_arity\""), "{sql}");
        assert!(sql.contains("jsonb_array_length("), "{sql}");
    }

    /// Two ordering terms name two projections, so neither reads the other's
    /// value node.
    #[test]
    fn each_ordering_term_names_its_own_projection() {
        let first = render(AttributeValue::at(0).numeric_key());
        let second = render(AttributeValue::at(1).numeric_key());
        assert!(first.contains("\"order_value_0\".\"value_node\""), "{first}");
        assert!(second.contains("\"order_value_1\".\"value_node\""), "{second}");
    }

    /// Clause 4.23.2: Numbers, Strings, Object, Array, Boolean, Time, Date,
    /// `DateTime`, Null. The rank is the token's position in that sequence.
    #[test]
    fn the_ascending_rank_reads_the_clause_4_23_2_sequence_in_order() {
        let sql = render(AttributeValue::at(0).datatype_rank(SortDirection::Ascending));
        assert!(sql.contains("array_position("), "{sql}");
        let sequence = "'number','string','object','array','boolean','https://uri.etsi.org/ngsi-ld/Time','https://uri.etsi.org/ngsi-ld/Date','https://uri.etsi.org/ngsi-ld/DateTime','null'";
        assert!(sql.contains(sequence), "{sql}");
    }

    /// A descending sort reverses the datatype order but leaves Null where
    /// clause 4.23.2 pins it: last.
    #[test]
    fn the_descending_rank_reverses_the_datatypes_and_still_ends_with_null() {
        let sql = render(AttributeValue::at(0).datatype_rank(SortDirection::Descending));
        let null_at = sql.find("'null'").expect("the null token is ranked");
        let number_at = sql.find("'number'").expect("the number token is ranked");
        let date_time_at = sql.find("'https://uri.etsi.org/ngsi-ld/DateTime'").expect("the DateTime token is ranked");
        assert!(date_time_at < number_at, "the datatypes reverse: {sql}");
        assert!(number_at < null_at, "null stays last: {sql}");
    }

    /// A number is compared as JSONB, which orders numbers numerically, rather
    /// than as the text `9` that would exceed `10`.
    #[test]
    fn the_numeric_key_compares_numbers_as_numbers() {
        let sql = render(AttributeValue::at(0).numeric_key());
        assert!(sql.contains("jsonb_typeof("), "{sql}");
        assert!(sql.contains("'number'"), "{sql}");
        assert!(!sql.contains("::numeric"), "no cast can see a value that is not a number: {sql}");
    }

    /// Clause 4.6.3 admits any offset, so a `DateTime` is resolved to an
    /// instant before it is compared.
    #[test]
    fn the_instant_key_resolves_the_offset_of_a_date_time_value() {
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

    /// Clause 4.9 defines a `ListProperty`'s target value as "the valueList
    /// array associated to such a `ListProperty`" and a `ListRelationship`'s as
    /// "the array of objects" it names, which clause 4.23.2 ranks under Array.
    /// Expansion wraps both in one `@list` node, so the carrier's arity is 1 and
    /// only the keyword distinguishes it from a scalar — read as a bare node it
    /// fell through to Object.
    #[test]
    fn an_ordered_list_carrier_ranks_as_an_array_rather_than_an_object() {
        let sql = render(AttributeValue::at(0).datatype_rank(SortDirection::Ascending));
        assert!(sql.contains("? ('@list')) THEN 'array'"), "{sql}");
        let list_test = sql.find("? ('@list')").expect("the list keyword is tested");
        let structured_fallback = sql.find("IS NOT NULL) THEN 'object'").expect("the structured-value reading is present");
        assert!(
            list_test < structured_fallback,
            "a list is ranked before anything falls through to Object: {sql}"
        );
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
