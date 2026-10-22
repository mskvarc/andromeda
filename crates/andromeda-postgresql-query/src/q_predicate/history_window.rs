//! The attribute instances of one Temporal Evolution inside a temporal window,
//! as the rows a temporal `q` is checked against.
//!
//! Clause 5.7.4.4 step S2: the query is checked against the "Attribute instances
//! (during the interval defined by the temporal query)", and clause 4.11 makes
//! those the values "valid within the specified timeframe". An instance is valid
//! within the interval when its instant on the requested Temporal Property lies
//! inside it, and so is the instance still in effect when the interval opens:
//! the latest one of its stream before the start, unless that latest one is a
//! deletion. This is Andromeda's reading of Annex C clause C.5.5, whose
//! `q=brandName!=Mercedes` still answers a vehicle whose `brandName` the
//! fixture observes an hour before the window and never since; the in-effect
//! instance is only evaluated, never rendered. A Query Term matches when one such instance satisfies it,
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
//!
//! The two kinds of valid instance are read by two probes, each one an index
//! range rather than a disjunction no index can drive: the instances inside
//! the interval, on the stream-led index of the axis, and the instance in
//! effect when it opens, which [`InEffectInstance`] places with one descending
//! probe per stream.
//!
//! Both probes read the history sources of the branch, so on the live branch
//! the entries still queued for the Temporal Evolution are instances like the
//! recorded ones.

use crate::{
    attribute_name_key::AttributeNameKey,
    error::RenderError,
    geo_predicate::{near_envelope_unindexed_prefilter, relation_expression},
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::{attribute_rows, stream_rows},
        grain::HistoryGrain,
    },
    q_predicate::{
        history_aliases::HistoryAliases,
        in_effect_instance::InEffectInstance,
        instance_test::{InstanceTest, non_empty},
    },
    time_interval_predicate::within_interval,
    tombstone::carries_a_value,
    window_reach::WindowReach,
};
use andromeda_ngsi_ld_query::{geo::query::GeoQuery, temporal::window::TimeInterval};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    cast::cast,
    literal::{false_literal, int_literal},
    pg_array::is_any_of,
    pg_type::PostgresType,
};
use chrono::{DateTime, Utc};
use sea_query::{Asterisk, Condition, DynIden, Expr as SeaExpr, ExprTrait, JoinType, Query, SelectStatement, SimpleExpr};
use typed_builder::TypedBuilder;

/// The instances of one Temporal Evolution a temporal `q` is checked against.
#[derive(Debug, Clone, TypedBuilder)]
pub struct HistoryWindow {
    /// The branch whose history the probes read.
    branch: HistoryBranch,
    /// The Temporal Evolution of the enclosing statement, which every source
    /// of the probes reads.
    of: HistoryOf,
    /// The `attribute_history` column holding the requested Temporal Property.
    axis: AttributeHistory,
    /// The interval the query selected on that axis.
    interval: TimeInterval,
    /// Which of the instances valid within the interval the probes read.
    reach: WindowReach,
}

/// The probes one set of instance tests runs over a window: the instances
/// inside it and, for an interval with a start, the instance in effect when it
/// opens.
///
/// The two hold one each of the instances valid within the interval, so a test
/// one valid instance passes is one that either probe finds an instance for.
pub(crate) struct WindowProbes {
    /// The probe over the instances inside the interval.
    inside: SelectStatement,
    /// The probe over the instance in effect when the interval opens.
    in_effect: Option<SelectStatement>,
}

impl WindowProbes {
    /// Complete every probe the same way.
    pub(crate) fn map(self, mut complete: impl FnMut(SelectStatement) -> SelectStatement) -> WindowProbes {
        WindowProbes {
            inside: complete(self.inside),
            in_effect: self.in_effect.map(complete),
        }
    }

    /// Whether either probe finds an instance.
    pub(crate) fn into_exists(self) -> Condition {
        Condition::any()
            .add(SeaExpr::exists(self.inside))
            .add_option(self.in_effect.map(SeaExpr::exists))
    }
}

impl HistoryWindow {
    /// The branch schema the probe's relations are qualified with.
    #[must_use]
    pub const fn schema(&self) -> Option<&DynIden> {
        self.branch.schema()
    }

    /// Wrap a value condition in the correlated probes over the window's
    /// instances of one attribute.
    ///
    /// The attribute-name test is added before the value so its dictionary key
    /// binds ahead of the value's operands, exactly as the current-state probe
    /// orders them.
    pub(crate) fn exists(&self, id: AttributeNameKey, value: Condition, aliases: &HistoryAliases) -> Condition {
        let test = Condition::all().add(Self::of_attribute(id, aliases)).add_option(non_empty(value));
        self.probes(test, None, aliases).into_exists()
    }

    /// Wrap several tests in correlated probes over the window's instances,
    /// holding when one instance passes any of them under its own attribute key.
    ///
    /// The in-effect probe is also narrowed to the streams of the tested
    /// attributes as a whole, so the instance in effect is placed for those
    /// streams alone rather than for every stream of the entity.
    pub(crate) fn exists_any(&self, tests: Vec<InstanceTest>, aliases: &HistoryAliases) -> Condition {
        let keys: Vec<i32> = tests.iter().map(|test| test.attribute.get()).collect();
        let streams = is_any_of(aliases.stream_column(HistoryGrain::AttributeNameKey), keys);
        let any = tests.into_iter().fold(Condition::any(), |any, test| {
            let of_attribute = Self::of_attribute(test.attribute, aliases);
            any.add(test.arm(of_attribute))
        });
        self.probes(any, Some(streams), aliases).into_exists()
    }

    /// Whether one instance of the `GeoProperty` keyed `geoproperty` valid
    /// within the window meets the relation `geo` states.
    ///
    /// Clause 5.7.4.4 step S3 checks the geo-query against "the `GeoProperty`
    /// instances that are within the interval defined by the temporal query",
    /// which are the instances a temporal `q` is checked against at step S2:
    /// those inside the interval and the one in effect when it opens, never a
    /// deletion. Both filters read them through [`Self::probes`], so for one
    /// window they test the same instances. A `GeoProperty` the dictionary
    /// never keyed has no history and matches nothing.
    ///
    /// A `near` relation with a `maxDistance` first tests the near envelope as
    /// a box no geometry index serves: the probe reads one Evolution's rows
    /// through its stream index, and an indexable box would have the planner
    /// AND a geometry index into every probe. The spheroidal distance then runs
    /// only on the rows the box keeps.
    ///
    /// # Errors
    ///
    /// Returns [`RenderError`] when the reference geometry cannot be encoded or
    /// the relation cannot be rendered.
    pub fn exists_geo(&self, geoproperty: Option<AttributeNameKey>, geo: &GeoQuery) -> Result<Condition, RenderError> {
        let Some(geoproperty) = geoproperty else {
            return Ok(Condition::all().add(false_literal()));
        };
        let aliases = HistoryAliases::top_level();
        let planar = aliases.instance_column(AttributeHistory::ValueGeometry);
        let geographic = cast(aliases.instance_column(AttributeHistory::ValueGeometry), PostgresType::Geography);
        let near_box = near_envelope_unindexed_prefilter(geo, &planar);
        let spatial = relation_expression(&geo.georel, planar, geographic, &geo.geometry)?;
        let relation = Condition::all().add_option(near_box).add(spatial);
        Ok(self.exists(geoproperty, relation, &aliases))
    }

    /// The window's correlated probes over the instances of one attribute,
    /// for a traversal to join the entities they name.
    pub(crate) fn probes_of(&self, id: AttributeNameKey, aliases: &HistoryAliases) -> WindowProbes {
        self.probes(Condition::all().add(Self::of_attribute(id, aliases)), None, aliases)
    }

    /// An instance of the attribute with this dictionary key.
    fn of_attribute(id: AttributeNameKey, aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::expr(aliases.instance_grain(HistoryGrain::AttributeNameKey)).eq(id.get())
    }

    /// The probes over the valid instances that pass `test`, each reading the
    /// enclosing statement's Entity alone. `streams` narrows the streams the
    /// in-effect probe places an instance for.
    fn probes(&self, test: Condition, streams: Option<SimpleExpr>, aliases: &HistoryAliases) -> WindowProbes {
        let in_effect = match self.reach {
            WindowReach::Inside => None,
            WindowReach::InsideAndInEffect => self.in_effect_start(),
        }
        .map(|start| self.in_effect_probe(start, streams, test.clone(), aliases)); // clone: both probes test their instances alike
        WindowProbes {
            inside: self.inside_probe(test, aliases),
            in_effect,
        }
    }

    /// The instant an instance in effect is sought at, if any.
    ///
    /// Every instance before a `before` bound is inside the interval already,
    /// and an unbounded interval holds every instance, so the one in effect at
    /// any point of either is among them. On `deletedAt` the instance in effect
    /// at the start is a deletion, which records no value, so no probe for it
    /// could ever find a target value.
    fn in_effect_start(&self) -> Option<DateTime<Utc>> {
        match self.interval {
            TimeInterval::Unbounded | TimeInterval::Before(_) => None,
            TimeInterval::After(start) | TimeInterval::Between { from: start, .. } => (self.axis != AttributeHistory::DeletedAt).then_some(start),
        }
    }

    /// The instances of the Entity, of every dataset, narrowed to those inside
    /// the interval that pass `test` and carry a value.
    ///
    /// An instance carrying no instant on the axis lies in no interval, an
    /// unbounded one included: clause 4.5.7 places every instance of a
    /// temporal representation at the instant its Temporal Property records,
    /// so no read on the axis represents it, and the filters judge the
    /// instances the read represents.
    fn inside_probe(&self, test: Condition, aliases: &HistoryAliases) -> SelectStatement {
        let instant = aliases.instance_column(self.axis);
        let inside = within_interval(&instant, &self.interval);
        let mut probe = Query::select();
        probe
            .expr(int_literal(1))
            .from(attribute_rows(&self.branch, &self.of, aliases.instance()))
            .cond_where(test)
            .and_where(Self::carries_a_value(aliases))
            .and_where(SeaExpr::expr(instant).is_not_null())
            .cond_where(inside);
        probe
    }

    /// The streams of the Entity, of every dataset, joined to the instances
    /// they held when the interval opened at `start`, narrowed to the ones
    /// that pass `test` and carry a value.
    fn in_effect_probe(&self, start: DateTime<Utc>, streams: Option<SimpleExpr>, test: Condition, aliases: &HistoryAliases) -> SelectStatement {
        let mut probe = self.in_effect_source(start, streams, aliases);
        probe.expr(int_literal(1)).cond_where(test).and_where(Self::carries_a_value(aliases));
        probe
    }

    /// The rows of the instances the streams of the attributes keyed `keys`
    /// held when the interval opens, deletions included, every column of the
    /// attribute source; `None` when the interval has no instance in effect
    /// before it.
    ///
    /// A split query merges these with the instances other parts held before
    /// the start (clause 5.7.4.4), so a stream whose latest instance is a
    /// deletion answers the deletion: it ends whatever an older instance of
    /// another part held.
    #[must_use]
    pub fn in_effect_rows(&self, keys: &[AttributeNameKey]) -> Option<SelectStatement> {
        let start = self.in_effect_start()?;
        let aliases = &HistoryAliases::top_level();
        let streams = is_any_of(aliases.stream_column(HistoryGrain::AttributeNameKey), keys.iter().map(|key| key.get()));
        let mut rows = self.in_effect_source(start, Some(streams), aliases);
        rows.expr(SeaExpr::col((aliases.instance(), Asterisk)));
        Some(rows)
    }

    /// The streams of the Entity, of every dataset, joined to the instances
    /// they held when the interval opened at `start`, `streams` narrowing
    /// which, with nothing projected yet.
    ///
    /// The instance is read laterally, its stream and placement written inside
    /// the lateral source, so each stream reads its own instances by index.
    fn in_effect_source(&self, start: DateTime<Utc>, streams: Option<SimpleExpr>, aliases: &HistoryAliases) -> SelectStatement {
        let instance = InEffectInstance::builder().branch(&self.branch).axis(self.axis).start(start).build();
        let mut placed = Query::select();
        placed
            .expr(SeaExpr::col((aliases.placed_instance(), Asterisk)))
            .from(attribute_rows(&self.branch, &self.of, aliases.placed_instance()))
            .cond_where(instance.placed_on_stream(aliases, &aliases.placed_instance()));
        let mut source = Query::select();
        source
            .from(stream_rows(&self.branch, &self.of, aliases.stream()))
            .join_lateral(JoinType::Join, instance.placement_source(aliases), aliases.placement(), SimpleExpr::from(true))
            .join_lateral(JoinType::Join, placed.take(), aliases.instance(), SimpleExpr::from(true));
        if let Some(streams) = streams {
            source.and_where(streams);
        }
        source.and_where(instance.opens_before_start(aliases));
        source
    }

    /// Clause 4.5.7: a deletion records no value, so it is no target value.
    fn carries_a_value(aliases: &HistoryAliases) -> SimpleExpr {
        carries_a_value(aliases.instance())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
        q_predicate::{
            history_aliases::HistoryAliases,
            history_window::HistoryWindow,
            instance_test::{InstanceTest, non_empty},
        },
        window_reach::WindowReach,
    };
    use andromeda_ngsi_ld_lexicon::time::date_time::NgsiLdDateTime;
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, ExprTrait, IntoIden, PostgresQueryBuilder, Query, SelectStatement};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn window_on(branch: HistoryBranch, axis: AttributeHistory, interval: TimeInterval) -> HistoryWindow {
        HistoryWindow::builder()
            .branch(branch)
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .axis(axis)
            .interval(interval)
            .reach(WindowReach::InsideAndInEffect)
            .build()
    }

    fn window(interval: TimeInterval) -> HistoryWindow {
        window_on(HistoryBranch::live(), AttributeHistory::ObservedAt, interval)
    }

    fn render(condition: Condition) -> String {
        Query::select()
            .expr(Expr::val(1))
            .cond_where(condition)
            .to_owned()
            .to_string(PostgresQueryBuilder)
    }

    fn statement(probe: &SelectStatement) -> String {
        probe.to_string(PostgresQueryBuilder)
    }

    /// The probe over the instances inside the window and, when there is one,
    /// the probe over the instance in effect when it opens, rendered apart.
    fn probes_of(window: &HistoryWindow, value: Condition) -> (String, Option<String>) {
        let aliases = HistoryAliases::top_level();
        let test = Condition::all()
            .add(HistoryWindow::of_attribute(AttributeNameKey::from(7), &aliases))
            .add_option(non_empty(value));
        let probes = window.probes(test, None, &aliases);
        (statement(&probes.inside), probes.in_effect.as_ref().map(statement))
    }

    fn after_2019() -> TimeInterval {
        TimeInterval::After(instant("2019-01-01T00:00:00Z"))
    }

    fn above_100() -> Condition {
        Condition::all().add(Expr::col((Alias::new("qh"), Alias::new("value_number"))).gt(100))
    }

    /// Clause 5.7.4.4 S2: the probe reads the window's instances of the one
    /// attribute, correlated to the temporal entity, and never a deletion. The
    /// instances inside the window are one index range, with no disjunction.
    #[test]
    fn the_probe_reads_the_in_window_instances_of_the_attribute() {
        let sql = render(window(after_2019()).exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        for fragment in [
            r#"FROM "attribute_history" AS "rh""#,
            r#""re"."id" = "e"."id""#,
            r#""pe"."entity_id" = "e"."id""#,
            r#") AS "qh" WHERE "qh"."attribute_name_key" = 7"#,
            r#""qh"."value_kind" <> "#,
            r#""qh"."observed_at" >= '2019-01-01"#,
            r#""qh"."value_number" > 100"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        let snapshot = window_on(
            HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            AttributeHistory::ObservedAt,
            after_2019(),
        );
        let (inside, _) = probes_of(&snapshot, above_100());
        assert!(!inside.contains(" OR "), "{inside}");
    }

    /// Clause 4.11 with Annex C clause C.5.5: an interval with a start also
    /// admits the instance its stream held when it opened, through a probe of
    /// its own that places that instance without an aggregate.
    #[test]
    fn an_interval_with_a_start_admits_the_instance_in_effect_when_it_opens() {
        // Beside another conjunct, so the disjunction of the two probes has to
        // keep its parentheses.
        let probes = window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level());
        let sql = render(Condition::all().add(Expr::col(Alias::new("flag")).eq(true)).add(probes));
        assert!(sql.contains(r#""flag" = TRUE AND (EXISTS(SELECT 1 FROM (SELECT"#), "{sql}");
        assert!(sql.contains(r") OR EXISTS(SELECT 1 FROM (SELECT"), "{sql}");
        assert!(sql.contains(r#"AS "qs" JOIN LATERAL"#), "{sql}");
        let (inside, in_effect) = probes_of(&window(after_2019()), Condition::all());
        assert!(inside.contains(r#""qh"."observed_at" >= '2019-01-01"#), "{inside}");
        assert!(!inside.contains(r#""qe""#), "{inside}");
        let in_effect = in_effect.expect("an interval with a start has an in-effect probe");
        for fragment in [
            r#"AS "qe" ON TRUE"#,
            r#""qi"."observed_at" = "qe"."placed") AS "qh" ON TRUE"#,
            r#""qe"."placed" < '2019-01-01"#,
            r#""qh"."value_kind" <> "#,
        ] {
            assert!(in_effect.contains(fragment), "{fragment} missing: {in_effect}");
        }
        assert!(!in_effect.contains("COALESCE("), "{in_effect}");
    }

    /// Clause 4.11 bounds `between` above exclusively, and only the instances
    /// inside the window are bounded there: the instance in effect at the start
    /// lies before both bounds.
    #[test]
    fn a_between_interval_bounds_only_the_window_probe_above() {
        let between = TimeInterval::Between {
            from: instant("2019-01-01T00:00:00Z"),
            to: instant("2019-02-01T00:00:00Z"),
        };
        let (inside, in_effect) = probes_of(&window(between), Condition::all());
        assert!(inside.contains(r#""qh"."observed_at" < '2019-02-01"#), "{inside}");
        let in_effect = in_effect.expect("an interval with a start has an in-effect probe");
        assert!(!in_effect.contains("2019-02-01"), "{in_effect}");
    }

    /// Clause 4.5.7: an instance with no instant on the axis is represented by
    /// no read on it, so the inside probe never reads one, over an unbounded
    /// interval either.
    #[test]
    fn the_inside_probe_reads_only_instances_placed_on_the_axis() {
        for interval in [TimeInterval::Unbounded, after_2019()] {
            let (inside, _) = probes_of(&window(interval), Condition::all());
            assert!(inside.contains(r#""qh"."observed_at" IS NOT NULL"#), "{inside}");
        }
    }

    /// Clause 5.7.4.4: a window read inside only, as a split query's leaves
    /// read it, renders no in-effect probe even when the interval has a start.
    #[test]
    fn an_inside_reach_renders_no_in_effect_probe() {
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live())
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .axis(AttributeHistory::ObservedAt)
            .interval(after_2019())
            .reach(WindowReach::Inside)
            .build();
        let (inside, in_effect) = probes_of(&window, Condition::all());
        assert!(inside.contains(r#""qh"."observed_at" >= '2019-01-01"#), "{inside}");
        assert!(in_effect.is_none(), "{in_effect:?}");
    }

    /// Clause 5.7.4.4: the rows a split query merges are the instances each
    /// named stream held when the window opened, a deletion included, placed
    /// as the probe places them.
    #[test]
    fn the_in_effect_rows_keep_deletions_of_the_named_streams() {
        let rows = window(after_2019())
            .in_effect_rows(&[AttributeNameKey::from(7), AttributeNameKey::from(9)])
            .expect("an interval with a start has in-effect rows");
        let sql = statement(&rows);
        for fragment in [
            r#"SELECT "qh".* FROM"#,
            r#""qs"."attribute_name_key" = ANY(ARRAY [7,9])"#,
            r#""qe"."placed" < '2019-01-01"#,
            r#""qd"."observed_at" IS NULL AND "qd"."deleted_at" <= '2019-01-01"#,
        ] {
            assert!(sql.contains(fragment), "{fragment} missing: {sql}");
        }
        assert!(!sql.contains(r#""qh"."value_kind" <> "#), "{sql}");
        assert!(window(TimeInterval::Unbounded).in_effect_rows(&[AttributeNameKey::from(7)]).is_none());
    }

    /// Every instance before a `before` bound is already inside the interval,
    /// and an unbounded one admits everything, so neither seeks a prior one.
    #[test]
    fn an_interval_without_a_start_seeks_no_prior_instance() {
        for interval in [TimeInterval::Before(instant("2019-01-01T00:00:00Z")), TimeInterval::Unbounded] {
            let sql = render(window(interval).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
            assert!(!sql.contains(") OR EXISTS(SELECT 1 FROM (SELECT"), "{sql}");
            for alias in [r#""qe""#, r#""qp""#, r#""qd""#, r#""qs""#] {
                assert!(!sql.contains(alias), "{alias}: {sql}");
            }
        }
    }

    /// On `deletedAt` the instance in effect at the start is a deletion, which
    /// carries no value (clause 4.5.7), so no probe seeks it.
    #[test]
    fn a_deleted_at_interval_renders_no_in_effect_probe() {
        let window = window_on(HistoryBranch::live(), AttributeHistory::DeletedAt, after_2019());
        let sql = render(window.exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
        assert!(!sql.contains(r#""qe""#) && !sql.contains(r#""qs""#), "{sql}");
    }

    /// Both probes test their instances alike and read the same Entity in
    /// each of their sources.
    #[test]
    fn both_probes_carry_the_same_value_test() {
        let snapshot = window_on(
            HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            AttributeHistory::ObservedAt,
            after_2019(),
        );
        let sql = render(snapshot.exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        pretty_assertions::assert_eq!(sql.matches(r#""qh"."value_number" > 100"#).count(), 2, "{sql}");
        pretty_assertions::assert_eq!(sql.matches(r#""re"."id" = "e"."id""#).count(), 3, "{sql}");
    }

    /// The terms of a disjunction share each probe, each arm keeping its own
    /// attribute key, and the in-effect probe places an instance only for the
    /// streams of the tested attributes.
    #[test]
    fn the_terms_of_an_or_share_each_probe() {
        let tests = vec![
            InstanceTest {
                attribute: AttributeNameKey::from(7),
                value: above_100(),
            },
            InstanceTest {
                attribute: AttributeNameKey::from(9),
                value: Condition::all().add(Expr::col((Alias::new("qh"), Alias::new("value_string"))).eq("x")),
            },
        ];
        let sql = render(window(after_2019()).exists_any(tests, &HistoryAliases::top_level()));
        let arms = r#"(("qh"."attribute_name_key" = 7 AND "qh"."value_number" > 100) OR ("qh"."attribute_name_key" = 9 AND "qh"."value_string" = 'x'))"#;
        pretty_assertions::assert_eq!(sql.matches(arms).count(), 2, "{sql}");
        pretty_assertions::assert_eq!(sql.matches(r#""qs"."attribute_name_key" = ANY(ARRAY [7,9])"#).count(), 1, "{sql}");
        let in_effect = sql.find("JOIN LATERAL").expect("the in-effect probe is rendered");
        let narrowing = sql.find(r#""qs"."attribute_name_key" = ANY(ARRAY [7,9])"#).expect("the narrowing is rendered");
        assert!(in_effect < narrowing, "the narrowing belongs to the in-effect probe: {sql}");
    }

    /// A nested hop seeks its prior instance under its own alias.
    #[test]
    fn a_hop_seeks_its_prior_instance_under_its_own_alias() {
        let sql = render(window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::at_depth(2)));
        for alias in [r#"AS "qp_ln2""#, r#"AS "qd_ln2""#, r#"AS "qe_ln2""#] {
            assert!(sql.contains(alias), "{alias} missing: {sql}");
        }
    }

    /// Clause 5.7.4.4 checks `q` at step S2, before the `datasetId` parameter
    /// filters instances at step S8, so neither probe narrows by dataset.
    #[test]
    fn the_probe_reads_the_instances_of_every_dataset() {
        for interval in [TimeInterval::Unbounded, after_2019()] {
            let sql = render(window(interval).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
            assert!(!sql.contains(r#""qh"."dataset_id" ="#) && !sql.contains(r#""qh"."dataset_id" IN"#), "{sql}");
        }
    }

    /// A nested hop's probe takes its own aliases.
    #[test]
    fn a_hop_probe_takes_aliases_of_its_own() {
        let sql = render(window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::at_depth(1)));
        assert!(
            sql.contains(r#"AS "qs_ln1""#) && sql.contains(r#"AS "qh_ln1""#) && sql.contains(r#"AS "qi_ln1""#),
            "{sql}"
        );
    }

    /// Read for an Evolution by key over recorded rows alone, every source of
    /// both probes is narrowed by the key, with no catalogue row read.
    #[test]
    fn both_probes_read_an_evolution_by_key_over_recorded_rows() {
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live_recorded())
            .of(HistoryOf::Evolution(EvolutionColumns {
                id: Expr::col((Alias::new("e"), Alias::new("id"))),
                key: Expr::col((Alias::new("e"), Alias::new("temporal_entity_key"))),
            }))
            .axis(AttributeHistory::ObservedAt)
            .interval(after_2019())
            .reach(WindowReach::InsideAndInEffect)
            .build();
        let sql = render(window.exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        for alias in ["qh", "qs", "qi"] {
            assert!(
                sql.contains(&format!(r#""rs"."temporal_entity_key" = "e"."temporal_entity_key") AS "{alias}""#)),
                "{alias}: {sql}"
            );
        }
        assert!(!sql.contains("temporal_entities") && !sql.contains("entity_id"), "{sql}");
    }

    /// A Snapshot reads its own branch: every history relation of both probes,
    /// the two placement probes included, is qualified with the branch schema.
    #[test]
    fn both_probes_are_qualified_with_the_branch_schema() {
        let window = window_on(
            HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()),
            AttributeHistory::ObservedAt,
            after_2019(),
        );
        let sql = render(window.exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
        pretty_assertions::assert_eq!(sql.matches(r#""attribute_history""#).count(), 4, "{sql}");
        pretty_assertions::assert_eq!(sql.matches(r#""snap_7"."attribute_history""#).count(), 4, "{sql}");
        pretty_assertions::assert_eq!(sql.matches(r#""temporal_attribute_streams""#).count(), 3, "{sql}");
        pretty_assertions::assert_eq!(sql.matches(r#""snap_7"."temporal_attribute_streams""#).count(), 3, "{sql}");
        assert!(!sql.contains("temporal_append"), "{sql}");
    }
}
