//! The attribute instances of one Temporal Evolution inside a temporal window,
//! as the rows a temporal `q` is checked against.
//!
//! Clause 5.7.4.4 step S2: the query is checked against the "Attribute instances
//! (during the interval defined by the temporal query)", and clause 4.11 makes
//! those the values "valid within the specified timeframe". An instance is valid
//! within the interval when its instant on the requested Temporal Property lies
//! inside it, so the instance a stream held before the interval opened is read
//! by no filter. A Query Term matches when one such instance satisfies it,
//! which is clause 4.9's multi-instance reading ("the target value shall be any
//! Value of such instances") applied to the instances inside the window rather
//! than to the single current one.
//!
//! Every `datasetId` counts. Clause 5.7.4.4 checks `q` at step S2, before the
//! `datasetId` parameter "filter[s] the Attribute instances" at step S8, and
//! clause 4.9 addresses no dataset: with none "explicitly addressed, the target
//! value shall be any Value of such instances".
//!
//! A deletion is recorded in history as an instance with no value (clause
//! 4.5.7), so it is never a target value and never satisfies a Query Term.
//!
//! The instances are read by one index range on the stream-led index of the
//! axis, over the history sources of the branch, so on the live branch the
//! entries still queued for the Temporal Evolution are instances like the
//! recorded ones.

use crate::{
    error::RenderError,
    geo_predicate::restriction,
    history_source::{
        HistoryBranch,
        HistoryOf,
        attribute_rows::{IDENTITY_COLUMNS, attribute_rows},
        axis::{axis_present, axis_window},
        entity_rows::entity_rows,
    },
    q_predicate::{
        history_aliases::HistoryAliases,
        instance_test::{InstanceTest, non_empty},
    },
    stored_geometry::{BoxIndexing, StoredGeometry},
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use andromeda_ngsi_ld_query::{geo::query::GeoQuery, temporal::window::TimeInterval};
use andromeda_postgresql_schema::temporal::AttributeHistory;
use andromeda_postgresql_seaquery::literal::{false_literal, int_literal};
use andromeda_postgresql_stored_form::{history_grain::HistoryGrain, tombstone::carries_a_value};
use andromeda_postgresql_stored_values::attribute_name_key::AttributeNameKey;
use sea_query::{Condition, DynIden, Expr as SeaExpr, ExprTrait, IntoIden, Query, SelectStatement, SimpleExpr, TableRef};
use typed_builder::TypedBuilder;

/// The instances of one Temporal Evolution a temporal `q` is checked against.
#[derive(Debug, Clone, TypedBuilder)]
pub struct HistoryWindow {
    /// The branch whose history the probe reads.
    branch: HistoryBranch,
    /// The Temporal Evolution of the enclosing statement, which every source
    /// of the probe reads.
    of: HistoryOf,
    /// The requested Temporal Property.
    timeproperty: TimeProp,
    /// The interval the query selected on that axis.
    interval: TimeInterval,
    /// The attribute whose instances the probes read as the one history row
    /// a representation tests; `None` for a window reading every instance
    /// inside it.
    #[builder(default = None, setter(skip))]
    reduced: Option<ReducedAttribute>,
}

/// An attribute reduced to one of its instances: the history row an enclosing
/// statement reads under `row`.
#[derive(Debug, Clone)]
struct ReducedAttribute {
    /// The attribute's dictionary key.
    attribute: AttributeNameKey,
    /// The alias the enclosing statement reads the history row under.
    row: DynIden,
}

impl ReducedAttribute {
    /// Whether the instance a probe reads under `aliases` is the row: the same
    /// dataset and the same write identity, which a queued entry carries as
    /// its recorded row will.
    fn is_the_row(&self, aliases: &HistoryAliases) -> impl Iterator<Item = SimpleExpr> {
        let dataset = aliases
            .instance_grain(HistoryGrain::DatasetKey)
            .eq(SeaExpr::col((self.row.clone(), HistoryGrain::DatasetKey))); // clone: each column names the row's alias
        let identity = IDENTITY_COLUMNS.map(|column| aliases.instance_column(column).eq(SeaExpr::col((self.row.clone(), column))));
        [dataset].into_iter().chain(identity)
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
            branch: self.branch.clone(), // clone: each window owns the branch its probe reads
            of,
            timeproperty: self.timeproperty,
            interval: self.interval,
            reduced: None,
        }
    }

    /// The same window with the instances of `attribute` reduced to the one
    /// history row the enclosing statement reads under `row`, which a
    /// representation tests the filters against (clause 5.7.4.4, last step).
    #[must_use]
    pub fn reduced_to(&self, attribute: AttributeNameKey, row: impl IntoIden) -> HistoryWindow {
        HistoryWindow {
            reduced: Some(ReducedAttribute {
                attribute,
                row: row.into_iden(),
            }),
            ..self.clone() // clone: the reduced window reads the branch and the Evolution this one reads
        }
    }

    /// The Temporal Evolutions of the window's branch under `alias`, keyed by
    /// the Entity's `id`.
    pub(crate) fn evolutions(&self, alias: impl IntoIden) -> TableRef {
        entity_rows(&self.branch, alias)
    }

    /// Wrap a value condition in the correlated probe over the window's
    /// instances of one attribute.
    ///
    /// The attribute-name test is added before the value so its dictionary key
    /// binds ahead of the value's operands, exactly as the current-state probe
    /// orders them.
    pub(crate) fn exists(&self, id: AttributeNameKey, value: Condition, aliases: &HistoryAliases) -> Condition {
        let test = Condition::all().add(self.of_attribute(id, aliases)).add_option(non_empty(value));
        Condition::all().add(SeaExpr::exists(self.probe(test, aliases)))
    }

    /// Wrap several tests in a correlated probe over the window's instances,
    /// holding when one instance passes any of them under its own attribute key.
    pub(crate) fn exists_any(&self, tests: Vec<InstanceTest>, aliases: &HistoryAliases) -> Condition {
        let any = tests.into_iter().fold(Condition::any(), |any, test| {
            let of_attribute = self.of_attribute(test.attribute, aliases);
            any.add(test.arm(of_attribute))
        });
        Condition::all().add(SeaExpr::exists(self.probe(any, aliases)))
    }

    /// Whether one instance of the `GeoProperty` keyed `geoproperty` inside the
    /// window meets the relation `geo` states.
    ///
    /// Clause 5.7.4.4 step S3 checks the geo-query against "the `GeoProperty`
    /// instances that are within the interval defined by the temporal query",
    /// which are the instances a temporal `q` is checked against at step S2,
    /// never a deletion. Both filters read them through [`Self::probe`], so for
    /// one window they test the same instances. A `GeoProperty` the dictionary
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

    /// The window's correlated probe over the instances of one attribute,
    /// for a traversal to join the entities it names.
    pub(crate) fn probe_of(&self, id: AttributeNameKey, aliases: &HistoryAliases) -> SelectStatement {
        self.probe(Condition::all().add(self.of_attribute(id, aliases)), aliases)
    }

    /// An instance of the attribute with this dictionary key, the reduced
    /// row alone where the window reduces the attribute.
    fn of_attribute(&self, id: AttributeNameKey, aliases: &HistoryAliases) -> SimpleExpr {
        let attribute = SeaExpr::expr(aliases.instance_grain(HistoryGrain::AttributeNameKey)).eq(id.get());
        match &self.reduced {
            Some(reduced) if reduced.attribute == id => reduced.is_the_row(aliases).fold(attribute, ExprTrait::and),
            Some(_) | None => attribute,
        }
    }

    /// The instances of the Entity, of every dataset, narrowed to those inside
    /// the interval that pass `test` and carry a value, reading the enclosing
    /// statement's Entity alone.
    ///
    /// An instance carrying no instant on the axis lies in no interval, an
    /// unbounded one included: clause 4.5.7 places every instance of a
    /// temporal representation at the instant its Temporal Property records,
    /// so no read on the axis represents it, and the filters judge the
    /// instances the read represents.
    fn probe(&self, test: Condition, aliases: &HistoryAliases) -> SelectStatement {
        let mut probe = Query::select();
        probe
            .expr(int_literal(1))
            .from(attribute_rows(&self.branch, &self.of, aliases.instance()))
            .cond_where(test)
            .and_where(carries_a_value(aliases.instance()))
            .cond_where(axis_present(aliases.instance(), self.timeproperty))
            .cond_where(axis_window(aliases.instance(), self.timeproperty, &self.interval));
        probe
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
            .build()
    }

    fn window(interval: TimeInterval) -> HistoryWindow {
        window_on(HistoryBranch::live(), TimeProp::Observed, interval)
    }

    fn render(condition: Condition) -> SelectStatement {
        Query::select().expr(Expr::val(1)).cond_where(condition).to_owned()
    }

    fn probe_of(window: &HistoryWindow, value: Condition) -> SelectStatement {
        let aliases = HistoryAliases::top_level();
        let test = Condition::all()
            .add(window.of_attribute(AttributeNameKey::from(7), &aliases))
            .add_option(non_empty(value));
        window.probe(test, &aliases)
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
        assert_statement_snapshot!(
            "history-window-the-probe-reads-the-in-window-instances-of-the-attribute-2",
            probe_of(&snapshot, above_100())
        );
    }

    /// Clause 5.7.4.4 S2: an interval with a start reads the instances inside
    /// it alone, none an earlier instant of the stream held, and keeps the
    /// parentheses of its probe beside another conjunct.
    #[test]
    fn an_interval_with_a_start_reads_only_the_instances_inside_it() {
        let probes = window(after_2019()).exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level());
        let statement = render(Condition::all().add(Expr::col(Alias::new("flag")).eq(true)).add(probes));
        assert_statement_snapshot!("history-window-an-interval-with-a-start-reads-only-the-instances-inside-it", statement);
    }

    /// Clause 4.11 bounds `between` above exclusively.
    #[test]
    fn a_between_interval_bounds_the_window_probe_above() {
        let between = TimeInterval::Between {
            from: instant("2019-01-01T00:00:00Z"),
            to: instant("2019-02-01T00:00:00Z"),
        };
        assert_statement_snapshot!(
            "history-window-a-between-interval-bounds-the-window-probe-above",
            probe_of(&window(between), Condition::all())
        );
    }

    /// Clause 4.5.7: an instance with no instant on the axis is represented by
    /// no read on it, so the probe never reads one, over an unbounded interval
    /// either.
    #[test]
    fn the_probe_reads_only_instances_placed_on_the_axis() {
        for (index, interval) in [TimeInterval::Unbounded, after_2019()].into_iter().enumerate() {
            assert_statement_snapshot!(
                format!("history-window-the-inside-probe-reads-only-instances-placed-on-the-axis-{}", index + 1),
                probe_of(&window(interval), Condition::all())
            );
        }
    }

    /// The terms of a disjunction share the probe, each arm keeping its own
    /// attribute key.
    #[test]
    fn the_terms_of_an_or_share_the_probe() {
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
        assert_statement_snapshot!("history-window-the-terms-of-an-or-share-the-probe", statement);
    }

    /// Clause 5.7.4.4 checks `q` at step S2, before the `datasetId` parameter
    /// filters instances at step S8, so the probe does not narrow by dataset.
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

    /// Read for an Evolution by key over recorded rows alone, the source of the
    /// probe is narrowed by the key, with no catalogue row read.
    #[test]
    fn the_probe_reads_an_evolution_by_key_over_recorded_rows() {
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live_recorded())
            .of(HistoryOf::Evolution(EvolutionColumns {
                id: Expr::col((Alias::new("e"), Alias::new("id"))),
                key: Expr::col((Alias::new("e"), Alias::new("entity_key"))),
            }))
            .timeproperty(TimeProp::Observed)
            .interval(after_2019())
            .build();
        let statement = render(window.exists(AttributeNameKey::from(7), above_100(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-the-probe-reads-an-evolution-by-key-over-recorded-rows", statement);
    }

    /// A Snapshot reads its own branch: every history relation of the probe is
    /// qualified with the branch schema.
    #[test]
    fn the_probe_is_qualified_with_the_branch_schema() {
        let window = window_on(HistoryBranch::recorded_in(Alias::new("snap_7").into_iden()), TimeProp::Observed, after_2019());
        let statement = render(window.exists(AttributeNameKey::from(7), Condition::all(), &HistoryAliases::top_level()));
        assert_statement_snapshot!("history-window-the-probe-is-qualified-with-the-branch-schema", statement);
    }
}
