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
//! effect when it opens, which [`InEffectInstance`] reads down from the start
//! on the stream-led indexes of the axis and of `deletedAt`, one stream of a
//! tested attribute at a time.
//!
//! Both probes read the history sources of the branch, so on the live branch
//! the entries still queued for the Temporal Evolution are instances like the
//! recorded ones.

use crate::{
    error::RenderError,
    geo_predicate::restriction,
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::{attribute_rows, stream_rows},
        axis::{axis_present, axis_window},
        entity_rows::entity_rows,
    },
    q_predicate::{
        history_aliases::HistoryAliases,
        in_effect_instance::InEffectInstance,
        instance_test::{InstanceTest, non_empty},
    },
    stored_geometry::{BoxIndexing, StoredGeometry},
    window_reach::WindowReach,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{geo::query::GeoQuery, temporal::window::TimeInterval};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::{
    literal::{false_literal, int_literal},
    pg_array::is_any_of,
};
use andromeda_postgresql_stored_form::{history_grain::HistoryGrain, tombstone::carries_a_value};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use chrono::{DateTime, Utc};
use sea_query::{Asterisk, Condition, DynIden, Expr as SeaExpr, ExprTrait, IntoIden, JoinType, Query, SelectStatement, SimpleExpr, TableRef};
use typed_builder::TypedBuilder;

/// The instances of one Temporal Evolution a temporal `q` is checked against.
#[derive(Debug, Clone, TypedBuilder)]
pub struct HistoryWindow {
    /// The branch whose history the probes read.
    branch: HistoryBranch,
    /// The Temporal Evolution of the enclosing statement, which every source
    /// of the probes reads.
    of: HistoryOf,
    /// The requested Temporal Property.
    timeproperty: TimeProp,
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

    /// The same window over the Temporal Evolution `of` names, which a hop
    /// reads its linked entity through.
    pub(crate) fn over(&self, of: HistoryOf) -> HistoryWindow {
        HistoryWindow {
            branch: self.branch.clone(), // clone: each window owns the branch its probes read
            of,
            timeproperty: self.timeproperty,
            interval: self.interval,
            reach: self.reach,
        }
    }

    /// The Temporal Evolutions of the window's branch under `alias`, keyed by
    /// the Entity's `id`.
    pub(crate) fn evolutions(&self, alias: impl IntoIden) -> TableRef {
        entity_rows(&self.branch, alias)
    }

    /// Wrap a value condition in the correlated probes over the window's
    /// instances of one attribute.
    ///
    /// The attribute-name test is added before the value so its dictionary key
    /// binds ahead of the value's operands, exactly as the current-state probe
    /// orders them.
    pub(crate) fn exists(&self, id: AttributeNameKey, value: Condition, aliases: &HistoryAliases) -> Condition {
        let test = Condition::all().add(Self::of_attribute(id, aliases)).add_option(non_empty(value));
        self.probes(test, Self::streams_of(id, aliases), aliases).into_exists()
    }

    /// Wrap several tests in correlated probes over the window's instances,
    /// holding when one instance passes any of them under its own attribute key.
    pub(crate) fn exists_any(&self, tests: Vec<InstanceTest>, aliases: &HistoryAliases) -> Condition {
        let keys: Vec<i32> = tests.iter().map(|test| test.attribute.get()).collect();
        let streams = is_any_of(aliases.stream_column(HistoryGrain::AttributeNameKey), keys);
        let any = tests.into_iter().fold(Condition::any(), |any, test| {
            let of_attribute = Self::of_attribute(test.attribute, aliases);
            any.add(test.arm(of_attribute))
        });
        self.probes(any, streams, aliases).into_exists()
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
    /// Every relation admitting a box first tests it, over the rows'
    /// great-circle reach, as a box no geometry index serves: the probe reads
    /// one Evolution's rows through its stream index, and an indexable box
    /// would have the planner AND a geometry index into every probe. The exact
    /// relation then runs only on the rows the box keeps.
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
        // Clause 4.7.2 stores an invalid geometry as written; the relation
        // judges its repaired form.
        let stored = StoredGeometry::written(
            aliases.instance_column(AttributeHistory::ValueGeometryReach),
            aliases.instance_column(AttributeHistory::ValueGeometryValid),
            aliases.instance_column(AttributeHistory::ValueGeometry),
        );
        let relation = restriction(&geo.georel, &geo.geometry, &stored, BoxIndexing::Unindexed)?;
        Ok(self.exists(geoproperty, relation, &aliases))
    }

    /// The window's correlated probes over the instances of one attribute,
    /// for a traversal to join the entities they name.
    pub(crate) fn probes_of(&self, id: AttributeNameKey, aliases: &HistoryAliases) -> WindowProbes {
        self.probes(Condition::all().add(Self::of_attribute(id, aliases)), Self::streams_of(id, aliases), aliases)
    }

    /// An instance of the attribute with this dictionary key.
    fn of_attribute(id: AttributeNameKey, aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::expr(aliases.instance_grain(HistoryGrain::AttributeNameKey)).eq(id.get())
    }

    /// A stream of the attribute with this dictionary key.
    fn streams_of(id: AttributeNameKey, aliases: &HistoryAliases) -> SimpleExpr {
        SeaExpr::expr(aliases.stream_column(HistoryGrain::AttributeNameKey)).eq(id.get())
    }

    /// The probes over the valid instances that pass `test`, each reading the
    /// enclosing statement's Entity alone.
    ///
    /// `streams` narrows the streams the in-effect probe places an instance
    /// for to those of the tested attributes, so no other stream of the
    /// Entity is read.
    fn probes(&self, test: Condition, streams: SimpleExpr, aliases: &HistoryAliases) -> WindowProbes {
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
    const fn in_effect_start(&self) -> Option<DateTime<Utc>> {
        match self.interval {
            TimeInterval::Unbounded | TimeInterval::Before(_) => None,
            TimeInterval::After(start) | TimeInterval::Between { from: start, .. } => match self.timeproperty {
                TimeProp::Observed | TimeProp::Created | TimeProp::Modified => Some(start),
                TimeProp::Deleted => None,
            },
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
        let mut probe = Query::select();
        probe
            .expr(int_literal(1))
            .from(attribute_rows(&self.branch, &self.of, aliases.instance()))
            .cond_where(test)
            .and_where(Self::carries_a_value(aliases))
            .cond_where(axis_present(aliases.instance(), self.timeproperty))
            .cond_where(axis_window(aliases.instance(), self.timeproperty, &self.interval));
        probe
    }

    /// The streams of the Entity, of every dataset, joined to the instances
    /// they held when the interval opened at `start`, narrowed to the ones
    /// that pass `test` and carry a value.
    fn in_effect_probe(&self, start: DateTime<Utc>, streams: SimpleExpr, test: Condition, aliases: &HistoryAliases) -> SelectStatement {
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
        let mut rows = self.in_effect_source(start, streams, aliases);
        rows.expr(SeaExpr::col((aliases.instance(), Asterisk)));
        Some(rows)
    }

    /// The streams of the Entity, of every dataset, joined to the instances
    /// they held when the interval opened at `start`, `streams` narrowing
    /// which, with nothing projected yet.
    ///
    /// The instance is read laterally, its stream written inside the lateral
    /// source, so each stream reads its own instances by index.
    fn in_effect_source(&self, start: DateTime<Utc>, streams: SimpleExpr, aliases: &HistoryAliases) -> SelectStatement {
        let instance = InEffectInstance::builder()
            .branch(&self.branch)
            .of(&self.of)
            .timeproperty(self.timeproperty)
            .start(start)
            .build();
        let mut source = Query::select();
        source
            .from(stream_rows(&self.branch, &self.of, aliases.stream()))
            .join_lateral(JoinType::Join, instance.rows(aliases), aliases.instance(), SimpleExpr::from(true))
            .and_where(streams)
            .and_where(instance.opens_before_start(aliases));
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
        history_source::{EvolutionColumns, HistoryBranch, HistoryOf},
        q_predicate::{
            history_aliases::HistoryAliases,
            history_window::HistoryWindow,
            instance_test::{InstanceTest, non_empty},
        },
        window_reach::WindowReach,
    };
    use andromeda_ngsi_ld_lexicon::{time::date_time::NgsiLdDateTime, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, ExprTrait, IntoIden, Query, SelectStatement};

    fn instant(raw: &str) -> DateTime<Utc> {
        raw.parse::<NgsiLdDateTime>().expect("a valid instant").get()
    }

    fn window_on(branch: HistoryBranch, timeproperty: TimeProp, interval: TimeInterval) -> HistoryWindow {
        HistoryWindow::builder()
            .branch(branch)
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .timeproperty(timeproperty)
            .interval(interval)
            .reach(WindowReach::InsideAndInEffect)
            .build()
    }

    fn window(interval: TimeInterval) -> HistoryWindow {
        window_on(HistoryBranch::live(), TimeProp::Observed, interval)
    }

    fn render(condition: Condition) -> SelectStatement {
        Query::select().expr(Expr::val(1)).cond_where(condition).to_owned()
    }

    /// The probe over the instances inside the window and, when there is one,
    /// the probe over the instance in effect when it opens, apart.
    fn probes_of(window: &HistoryWindow, value: Condition) -> (SelectStatement, Option<SelectStatement>) {
        let aliases = HistoryAliases::top_level();
        let test = Condition::all()
            .add(HistoryWindow::of_attribute(AttributeNameKey::from(7), &aliases))
            .add_option(non_empty(value));
        let probes = window.probes(test, HistoryWindow::streams_of(AttributeNameKey::from(7), &aliases), &aliases);
        (probes.inside, probes.in_effect)
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
        let statement = render(window(after_2019()).exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-the-probe-reads-the-in-window-instances-of-the-attribute-1", statement);
        let snapshot = window_on(HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()), TimeProp::Observed, after_2019());
        let (inside, _) = probes_of(&snapshot, above_100());
        assert_statement_snapshot!("history-window-the-probe-reads-the-in-window-instances-of-the-attribute-2", inside);
    }

    /// Clause 4.11 with Annex C clause C.5.5: an interval with a start also
    /// admits the instance its stream held when it opened, through a probe of
    /// its own that places that instance without an aggregate.
    #[test]
    fn an_interval_with_a_start_admits_the_instance_in_effect_when_it_opens() {
        // Beside another conjunct, so the disjunction of the two probes has to
        // keep its parentheses.
        let probes = window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level());
        let statement = render(Condition::all().add(Expr::col(Alias::new("flag")).eq(true)).add(probes));
        assert_statement_snapshot!(
            "history-window-an-interval-with-a-start-admits-the-instance-in-effect-when-it-opens-1",
            statement
        );
        let (inside, in_effect) = probes_of(&window(after_2019()), Condition::all());
        assert_statement_snapshot!("history-window-an-interval-with-a-start-admits-the-instance-in-effect-when-it-opens-2", inside);
        let in_effect = in_effect.expect("an interval with a start has an in-effect probe");
        assert_statement_snapshot!(
            "history-window-an-interval-with-a-start-admits-the-instance-in-effect-when-it-opens-3",
            in_effect
        );
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
        assert_statement_snapshot!("history-window-a-between-interval-bounds-only-the-window-probe-above-1", inside);
        let in_effect = in_effect.expect("an interval with a start has an in-effect probe");
        assert_statement_snapshot!("history-window-a-between-interval-bounds-only-the-window-probe-above-2", in_effect);
    }

    /// Clause 4.5.7: an instance with no instant on the axis is represented by
    /// no read on it, so the inside probe never reads one, over an unbounded
    /// interval either.
    #[test]
    fn the_inside_probe_reads_only_instances_placed_on_the_axis() {
        for (index, interval) in [TimeInterval::Unbounded, after_2019()].into_iter().enumerate() {
            let (inside, _) = probes_of(&window(interval), Condition::all());
            assert_statement_snapshot!(
                format!("history-window-the-inside-probe-reads-only-instances-placed-on-the-axis-{}", index + 1),
                inside
            );
        }
    }

    /// Clause 5.7.4.4: a window read inside only, as a split query's leaves
    /// read it, renders no in-effect probe even when the interval has a start.
    #[test]
    fn an_inside_reach_renders_no_in_effect_probe() {
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live())
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .timeproperty(TimeProp::Observed)
            .interval(after_2019())
            .reach(WindowReach::Inside)
            .build();
        let (inside, in_effect) = probes_of(&window, Condition::all());
        assert_statement_snapshot!("history-window-an-inside-reach-renders-no-in-effect-probe", inside);
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
        assert_statement_snapshot!("history-window-the-in-effect-rows-keep-deletions-of-the-named-streams", rows);
        assert!(window(TimeInterval::Unbounded).in_effect_rows(&[AttributeNameKey::from(7)]).is_none());
    }

    /// Every instance before a `before` bound is already inside the interval,
    /// and an unbounded one admits everything, so neither seeks a prior one.
    #[test]
    fn an_interval_without_a_start_seeks_no_prior_instance() {
        for (index, interval) in [TimeInterval::Before(instant("2019-01-01T00:00:00Z")), TimeInterval::Unbounded]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(
                format!("history-window-an-interval-without-a-start-seeks-no-prior-instance-{}", index + 1),
                render(window(interval).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()))
            );
        }
    }

    /// On `deletedAt` the instance in effect at the start is a deletion, which
    /// carries no value (clause 4.5.7), so no probe seeks it.
    #[test]
    fn a_deleted_at_interval_renders_no_in_effect_probe() {
        let window = window_on(HistoryBranch::live(), TimeProp::Deleted, after_2019());
        let statement = render(window.exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-a-deleted-at-interval-renders-no-in-effect-probe", statement);
    }

    /// Both probes test their instances alike and read the same Entity in
    /// each of their sources.
    #[test]
    fn both_probes_carry_the_same_value_test() {
        let snapshot = window_on(HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()), TimeProp::Observed, after_2019());
        let statement = render(snapshot.exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-both-probes-carry-the-same-value-test", statement);
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
        let statement = render(window(after_2019()).exists_any(tests, &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-the-terms-of-an-or-share-each-probe", statement);
    }

    /// A nested hop seeks its prior instance under its own alias.
    #[test]
    fn a_hop_seeks_its_prior_instance_under_its_own_alias() {
        let statement = render(window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::at_depth(2)));
        assert_statement_snapshot!("history-window-a-hop-seeks-its-prior-instance-under-its-own-alias", statement);
    }

    /// Clause 5.7.4.4 checks `q` at step S2, before the `datasetId` parameter
    /// filters instances at step S8, so neither probe narrows by dataset.
    #[test]
    fn the_probe_reads_the_instances_of_every_dataset() {
        for (index, interval) in [TimeInterval::Unbounded, after_2019()].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("history-window-the-probe-reads-the-instances-of-every-dataset-{}", index + 1),
                render(window(interval).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()))
            );
        }
    }

    /// A nested hop's probe takes its own aliases.
    #[test]
    fn a_hop_probe_takes_aliases_of_its_own() {
        let statement = render(window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::at_depth(1)));
        assert_statement_snapshot!("history-window-a-hop-probe-takes-aliases-of-its-own", statement);
    }

    /// Read for an Evolution by key over recorded rows alone, every source of
    /// both probes is narrowed by the key, with no catalogue row read.
    #[test]
    fn both_probes_read_an_evolution_by_key_over_recorded_rows() {
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live_recorded())
            .of(HistoryOf::Evolution(EvolutionColumns {
                id: Expr::col((Alias::new("e"), Alias::new("id"))),
                key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
            }))
            .timeproperty(TimeProp::Observed)
            .interval(after_2019())
            .reach(WindowReach::InsideAndInEffect)
            .build();
        let statement = render(window.exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-both-probes-read-an-evolution-by-key-over-recorded-rows", statement);
    }

    /// A Snapshot reads its own branch: every history relation of both probes,
    /// both heads of the stream the instance in effect is placed in included,
    /// is qualified with the branch schema.
    #[test]
    fn both_probes_are_qualified_with_the_branch_schema() {
        let window = window_on(HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()), TimeProp::Observed, after_2019());
        let statement = render(window.exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-both-probes-are-qualified-with-the-branch-schema", statement);
    }
}
