//! The attribute instances of one Temporal Evolution inside a temporal window,
//! as the rows a temporal `q` is checked against.
//!
//! Clause 5.7.4.4 step S2: the query is checked against the "Attribute instances
//! (during the interval defined by the temporal query)", and clause 4.11 makes
//! those the values "valid within the specified timeframe". An instance is valid
//! within the interval when its instant on the requested Temporal Property lies
//! inside it, and so is the instance still in effect when the interval opens:
//! the latest one of its stream before the start, unless that latest one is a
//! deletion. Annex C clause C.5.5 relies on exactly this -- `brandName`,
//! observed an hour before the window and never since, still answers
//! `q=brandName!=Mercedes` -- while the in-effect instance is only evaluated,
//! never rendered. A Query Term matches when one such instance satisfies it,
//! which is clause 4.9's multi-instance reading ("the target value shall be any
//! Value of such instances") applied to the instances valid in the window
//! rather than to the single current one.
//!
//! Every `datasetId` counts. Clause 5.7.4.4 checks `q` at step S2, before the
//! `datasetId` parameter "filter[s] the Attribute instances" at step S8, and
//! clause 4.9 addresses no dataset: with none "explicitly addressed, the target
//! value shall be any Value of such instances".
//!
//! A deletion is recorded in history as an instance with no value (clause
//! 4.5.7), so it is never a target value and never satisfies a Query Term.

use crate::{attribute_name_key::AttributeNameKey, time_interval_predicate::within_interval};
use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
use andromeda_ngsi_ld_vocabulary::expanded::properties::{OBJECT, OBSERVED_AT, VALUE};
use andromeda_postgresql_schema::temporal::{AttributeHistory, TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    jsonb::{jsonb_build_array, jsonb_build_object, jsonb_strip_nulls, jsonb_typeof, to_jsonb},
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
};
use chrono::{DateTime, Utc};
use sea_query::{Alias, CaseStatement, Condition, DynIden, Expr as SeaExpr, ExprTrait, Func, JoinType, Query, QueryStatementBuilder, SimpleExpr};
use typed_builder::TypedBuilder;

/// The instances of one Temporal Evolution a temporal `q` is checked against.
#[derive(Debug, Clone, TypedBuilder)]
pub struct HistoryWindow {
    /// The context branch schema the history relations are qualified with, or
    /// `None` for the live branch.
    schema: Option<DynIden>,
    /// The enclosing statement's `temporal_entity_key`, which every probe is
    /// correlated against.
    temporal_entity_key: SimpleExpr,
    /// The `attribute_history` column holding the requested Temporal Property.
    axis: AttributeHistory,
    /// The interval the query selected on that axis.
    interval: TimeInterval,
}

/// The two relations one history probe scans, under the names it gives them.
///
/// A probe nested inside another (a linked-entity hop reading a Relationship's
/// history) takes its own suffix, so its relations cannot shadow the enclosing
/// probe's.
#[derive(Debug, Clone)]
pub(crate) struct HistoryAliases {
    /// Alias of the `temporal_attribute_streams` scan.
    stream: String,
    /// Alias of the `attribute_history` scan.
    instance: String,
    /// Alias of the scan that finds the latest instance before the interval.
    prior: String,
}

impl HistoryAliases {
    /// The aliases of the top-level probe.
    pub(crate) fn top_level() -> HistoryAliases {
        HistoryAliases {
            stream: "qs".to_owned(),
            instance: "qh".to_owned(),
            prior: "qp".to_owned(),
        }
    }

    /// The aliases of a linked-entity hop `depth` levels into a traversal.
    pub(crate) fn at_depth(depth: usize) -> HistoryAliases {
        HistoryAliases {
            stream: format!("qs_ln{depth}"),
            instance: format!("qh_ln{depth}"),
            prior: format!("qp_ln{depth}"),
        }
    }

    /// A column of the instance row this probe scans.
    pub(crate) fn instance_column(&self, column: AttributeHistory) -> SimpleExpr {
        SeaExpr::col((Alias::new(self.instance.as_str()), column))
    }

    /// A column of the stream row this probe scans.
    fn stream_column(&self, column: TemporalAttributeStreams) -> SimpleExpr {
        SeaExpr::col((Alias::new(self.stream.as_str()), column))
    }
}

impl HistoryWindow {
    /// The branch schema the probe's relations are qualified with.
    #[must_use]
    pub const fn schema(&self) -> Option<&DynIden> {
        self.schema.as_ref()
    }

    /// Wrap a value condition in the correlated probe over the window's
    /// instances of one attribute.
    ///
    /// The attribute-name test is added before the value so its dictionary key
    /// binds ahead of the value's operands, and the correlation last, exactly as
    /// the current-state probe orders them.
    pub(crate) fn exists(&self, id: AttributeNameKey, value: Condition, aliases: &HistoryAliases) -> Condition {
        let mut probe = self.instances_of(id, aliases);
        probe.cond_where(value);
        probe.and_where(SeaExpr::expr(aliases.stream_column(TemporalAttributeStreams::TemporalEntityKey)).eq(self.temporal_entity_key.clone())); // clone: every probe of the query correlates on the same outer column
        Condition::all().add(SeaExpr::exists(probe.take()))
    }

    /// The window's instances of one attribute, before any correlation: the
    /// stream rows of the attribute joined to their history rows, of every
    /// dataset, narrowed to the instances valid within the interval that carry
    /// a value.
    pub(crate) fn instances_of(&self, id: AttributeNameKey, aliases: &HistoryAliases) -> sea_query::SelectStatement {
        let stream = || Alias::new(aliases.stream.as_str());
        let instance = || Alias::new(aliases.instance.as_str());
        let mut probe = Query::select();
        probe
            .expr(int_literal(1))
            .from(qualified(self.schema.as_ref(), TemporalAttributeStreams::Table).alias(stream()))
            .join_as(
                JoinType::Join,
                qualified(self.schema.as_ref(), AttributeHistory::Table),
                instance(),
                SeaExpr::col((instance(), AttributeHistory::StreamKey)).equals((stream(), TemporalAttributeStreams::StreamKey)),
            )
            .and_where(SeaExpr::expr(aliases.stream_column(TemporalAttributeStreams::AttributeNameKey)).eq(id.get()))
            // Clause 4.5.7: a deletion records no value, so it is no target value.
            .and_where(SeaExpr::expr(aliases.instance_column(AttributeHistory::DeletedAt)).is_null())
            .cond_where(self.valid_within(aliases));
        probe
    }

    /// Whether the scanned instance is valid within the interval (clause 4.11):
    /// its instant lies inside it, or it is the instance in effect when an
    /// interval with a start opens.
    fn valid_within(&self, aliases: &HistoryAliases) -> Condition {
        let inside = within_interval(&aliases.instance_column(self.axis), &self.interval);
        match self.interval {
            // Every instance before the bound is inside the interval already,
            // so the one in effect at any point of it is among them.
            TimeInterval::Unbounded | TimeInterval::Before(_) => inside,
            TimeInterval::After(start) | TimeInterval::Between { from: start, .. } => Condition::any().add(inside).add(self.in_effect_at(start, aliases)),
        }
    }

    /// Whether the scanned instance is the one its stream held at `start`: the
    /// latest before it on the axis, unless another instance replaced it at
    /// `start` itself -- the interval then opens on that one, which it holds
    /// already.
    ///
    /// The latest is sought among every instance of the stream, deletions
    /// included, and a deletion is placed at its instant on the axis or, when
    /// it carries none there, at the instant it was deleted: an Attribute
    /// deleted before the interval opens holds nothing in it, so a value
    /// recorded before that deletion is not in effect.
    fn in_effect_at(&self, start: DateTime<Utc>, aliases: &HistoryAliases) -> Condition {
        let prior = || Alias::new(aliases.prior.as_str());
        let placed = || Func::coalesce([SeaExpr::col((prior(), self.axis)), SeaExpr::col((prior(), AttributeHistory::DeletedAt))]);
        let latest = Query::select()
            .expr(Func::max(placed()))
            .from(qualified(self.schema.as_ref(), AttributeHistory::Table).alias(prior()))
            .and_where(SeaExpr::col((prior(), AttributeHistory::StreamKey)).eq(aliases.instance_column(AttributeHistory::StreamKey)))
            .and_where(SeaExpr::expr(placed()).lte(start))
            .to_owned();
        let instant = || SeaExpr::expr(aliases.instance_column(self.axis));
        Condition::all()
            .add(instant().lt(start))
            .add(instant().eq(SimpleExpr::SubQuery(None, Box::new(latest.into_sub_query_statement()))))
    }

    /// The correlation a hop's own probe carries, for a traversal that starts
    /// from this window.
    pub(crate) fn correlation(&self, aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::expr(aliases.stream_column(TemporalAttributeStreams::TemporalEntityKey)).eq(self.temporal_entity_key.clone()) // clone: every probe of the query correlates on the same outer column
    }
}

/// The one history instance a probe scans, rebuilt as the expanded entity
/// document the payload jsonpath reads: `[{"<attr>": [<instance>]}]`.
///
/// A Query Term reaching a sub-attribute or a member of a compound value is
/// answered by a jsonpath over an expanded entity (clause 4.9), and a history
/// instance keeps exactly what that path walks: its sub-attributes in
/// `metadata`, verbatim, its value in the typed columns, and its `observedAt` in
/// its own column. The value becomes the `hasValue` node the path descends into
/// -- the stored node itself for an object value, a value node for a scalar --
/// the Relationship object a `hasObject` node, and `observedAt` its value node.
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
    let carriers = jsonb_strip_nulls(jsonb_build_object([
        text_literal(VALUE.as_str()),
        jsonb_build_array(value_node),
        text_literal(OBJECT.as_str()),
        jsonb_build_array(jsonb_build_object([text_literal("@id"), column(AttributeHistory::ValueUri)])),
        text_literal(OBSERVED_AT.as_str()),
        jsonb_build_array(jsonb_build_object([text_literal("@value"), to_jsonb(column(AttributeHistory::ObservedAt))])),
    ]));
    let metadata: SimpleExpr = Func::coalesce([column(AttributeHistory::Metadata), jsonb_build_object([])]).into();
    let instance = metadata.jsonb_concat(carriers);
    jsonb_build_array(jsonb_build_object([text_literal(attribute_iri), jsonb_build_array(instance)]))
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        q_predicate::history_window::{HistoryAliases, HistoryWindow, instance_document},
    };
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr};

    fn window(interval: TimeInterval) -> HistoryWindow {
        HistoryWindow::builder()
            .schema(None)
            .temporal_entity_key(Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))))
            .axis(AttributeHistory::ObservedAt)
            .interval(interval)
            .build()
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .expr(Expr::val(1))
            .cond_where(condition)
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn after_2019() -> TimeInterval {
        TimeInterval::After(
            DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
                .expect("a valid instant")
                .with_timezone(&Utc),
        )
    }

    /// Clause 5.7.4.4 S2: the probe reads the window's instances of the one
    /// attribute, correlated to the temporal entity, and never a deletion.
    #[test]
    fn the_probe_reads_the_in_window_instances_of_the_attribute() {
        let value = Condition::all().add(Expr::col((Alias::new("qh"), Alias::new("value_number"))).gt(100));
        let sql = render(window(after_2019()).exists(AttributeNameKey::from(7), value, &HistoryAliases::top_level()));
        for fragment in [
            r#"FROM "temporal_attribute_streams" AS "qs" JOIN "attribute_history" AS "qh" ON "qh"."stream_key" = "qs"."stream_key""#,
            r#""qs"."attribute_name_key" = 7"#,
            r#""qh"."deleted_at" IS NULL"#,
            r#""qh"."observed_at" >= '2019-01-01"#,
            r#""qh"."value_number" > 100"#,
            r#""qs"."temporal_entity_key" = "e"."temporal_entity_key""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    /// Clause 4.11 with Annex C clause C.5.5: an interval with a start also
    /// admits the instance its stream held when it opened -- the latest one
    /// before the start, deletions included in finding it.
    #[test]
    fn an_interval_with_a_start_admits_the_instance_in_effect_when_it_opens() {
        let sql = render(window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
        for fragment in [
            r#""qh"."observed_at" >= '2019-01-01"#,
            r#"OR ("qh"."observed_at" < '2019-01-01"#,
            r#""qh"."observed_at" = (SELECT MAX(COALESCE("qp"."observed_at", "qp"."deleted_at")) FROM "attribute_history" AS "qp" WHERE "qp"."stream_key" = "qh"."stream_key""#,
            r#"COALESCE("qp"."observed_at", "qp"."deleted_at") <= '2019-01-01"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }

    /// Every instance before a `before` bound is already inside the interval,
    /// and an unbounded one admits everything, so neither seeks a prior one.
    #[test]
    fn an_interval_without_a_start_seeks_no_prior_instance() {
        let before = TimeInterval::Before(
            DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
                .expect("a valid instant")
                .with_timezone(&Utc),
        );
        for interval in [before, TimeInterval::Unbounded] {
            let sql = render(window(interval).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
            assert!(!sql.contains(r#""qp""#), "{sql}");
        }
    }

    /// A nested hop seeks its prior instance under its own alias.
    #[test]
    fn a_hop_seeks_its_prior_instance_under_its_own_alias() {
        let sql = render(window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::at_depth(2)));
        assert!(sql.contains(r#"AS "qp_ln2""#), "{sql}");
    }

    /// Clause 5.7.4.4 checks `q` at step S2, before the `datasetId` parameter
    /// filters instances at step S8, so the probe reads the instances of every
    /// dataset.
    #[test]
    fn the_probe_reads_the_instances_of_every_dataset() {
        for interval in [TimeInterval::Unbounded, after_2019()] {
            let sql = render(window(interval).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
            assert!(!sql.contains("dataset_id"), "{sql}");
        }
    }

    /// A nested hop's probe takes its own aliases.
    #[test]
    fn a_hop_probe_takes_aliases_of_its_own() {
        let sql = render(window(TimeInterval::Unbounded).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::at_depth(1)));
        assert!(sql.contains(r#"AS "qs_ln1""#) && sql.contains(r#"AS "qh_ln1""#), "{sql}");
    }

    /// The rebuilt instance carries the sub-attributes, the value carriers and
    /// `observedAt` the payload jsonpath walks, under the attribute's IRI.
    #[test]
    fn the_instance_document_rebuilds_the_expanded_attribute() {
        let document: SimpleExpr = instance_document("https://example.org/speed", &HistoryAliases::top_level());
        let sql = Query::select().expr(document).to_owned().to_string(PostgresQueryBuilder);
        for fragment in [
            r#"jsonb_build_array(jsonb_build_object('https://example.org/speed', jsonb_build_array(COALESCE("qh"."metadata", jsonb_build_object()) || jsonb_strip_nulls("#,
            "'https://uri.etsi.org/ngsi-ld/hasValue'",
            "'https://uri.etsi.org/ngsi-ld/hasObject'",
            "'https://uri.etsi.org/ngsi-ld/observedAt'",
            r#"CASE WHEN (jsonb_typeof("qh"."value_object") = 'object') THEN "qh"."value_object""#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
    }
}
