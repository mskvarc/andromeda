//! The clause 4.9 linked-entity traversal axis compiled to a correlated probe.
//!
//! `rel{attr}=="x"` asks about an attribute of the entity a Relationship points
//! at, so answering it means leaving the row being filtered. One hop becomes one
//! `EXISTS`: the Relationship's instances are read, each object they hold is
//! expanded to one row and joined to the entity it names, and the rest of the
//! predicate is compiled against that entity. Hops nest, so the probes nest
//! with them.
//!
//! Over current state the instances are `current_attributes` rows and the join
//! lands on `entities.id`, because a Relationship's object is the Entity's URI
//! (clause 4.5.3); it is the surrogate `entity_key` of that row the inner
//! predicate correlates against, and the linked entity's `entity_search` row is
//! joined only when the hop carries a type guard, the one thing a hop reads
//! from it. Over a temporal window the instances are the history instances
//! valid within it, and the join lands on the linked entity's Temporal
//! Evolution in the temporal catalogue, whose types a guard reads: the
//! `temporal_linked_retrieval` extension renders every Linked Entity over the
//! window of the roots, so a hop asks about the instances of the linked entity
//! valid within that window too.
//!
//! An index path reads the same hop inward instead, from the linked entities
//! the rest of the predicate keeps to the instances naming them
//! ([`LinkedHop::linking_branches`]), so its cost follows the linked matches
//! rather than the Entities being filtered.
//!
//! This lives apart from the comparison rendering next door because it changes
//! *which row* is being asked about rather than *what is asked* of it; the two
//! change for unrelated reasons.

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
    entity_sides::holds_current_state,
    history_source::HistoryOf,
    instance_expiry::instance_stands,
    q_predicate::{
        history_aliases::HistoryAliases,
        history_window::{HistoryWindow, WindowProbes},
        instance_scan::InstanceScan,
    },
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
use andromeda_postgresql_codes::attribute_kind::AttributeKind;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities, EntitySearch},
    temporal::AttributeHistory,
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    jsonb::{jsonb_array_elements_text, jsonb_build_array},
    literal::{false_literal, int_literal},
    operator::array_contains_key,
    pg_operator::PgBinaryOperator,
};
use sea_query::{Alias, Condition, DynIden, Expr as SeaExpr, ExprTrait, Func, JoinType, Query, SelectStatement, SimpleExpr, TableRef};

/// One hop of a traversal: the relations its probe introduces, and the probe
/// itself.
///
/// The aliases are suffixed by the hop's depth so a nested traversal's probe
/// cannot shadow the one enclosing it, and they are decided once here rather
/// than by the caller, because the caller needs the *same* aliases twice: once
/// to compile the inner predicate against the entity this hop lands on, and
/// again to render the probe around it.
pub(crate) struct LinkedHop {
    /// How many hops into the traversal this one is, counting from one.
    depth: usize,
    /// Alias of this hop's `current_attributes` scan.
    attributes: String,
    /// Alias of the objects this hop's instances hold, one row each.
    objects: String,
    /// Alias of the `entities` row this hop lands on.
    linked_entity: String,
    /// Alias of that entity's `entity_search` row, joined only for a type guard.
    linked_search: String,
}

impl LinkedHop {
    /// The hop `depth` levels into a traversal, counting from one.
    pub(crate) fn at_depth(depth: usize) -> LinkedHop {
        LinkedHop {
            depth,
            attributes: format!("ca_ln{depth}"),
            objects: format!("lo{depth}"),
            linked_entity: format!("le{depth}"),
            linked_search: format!("ls{depth}"),
        }
    }

    /// Where a predicate compiled *inside* this hop reads its entity: the
    /// linked identity and search rows, whose key its probes correlate on.
    ///
    /// `schema` is the branch schema of the statement this hop is nested in, so
    /// the linked entity is read in the same branch the traversal started from
    /// rather than falling back to the live default schema.
    pub(crate) fn inner_rows(&self, schema: Option<DynIden>) -> EntityRowRefs {
        EntityRowRefs::aliased(schema, Alias::new(self.linked_entity.as_str()), Alias::new(self.linked_search.as_str()))
    }

    /// Wrap an already-compiled inner predicate in this hop's probe.
    ///
    /// The probe is an `EXISTS` over the Relationship's instances `outer`
    /// reads, joined to the entity each one names. Over current state those are
    /// the `current_attributes` rows of the outer entity; over a temporal
    /// window they are the history instances valid within it, so a temporal `q`
    /// follows the targets the Relationship named inside the window (clause
    /// 5.7.4.4), and the window's two probes each become one `EXISTS`. The join
    /// lands on the Entity's `id`, the unique external key of `entities`, and
    /// keeps the linked Entity's side the outer read serves.
    ///
    /// A deleted Relationship needs no exclusion: `current_attributes` holds no
    /// tombstone row, and the history probe excludes deletions itself.
    pub(crate) fn probe(
        &self,
        relationship_id: AttributeNameKey,
        guard_types: &[AttributeName],
        types: &ResolvedNames<EntityTypeName>,
        outer: &InstanceScan<'_>,
        inner: Condition,
    ) -> Condition {
        match outer {
            InstanceScan::Current(rows) => {
                let probe = self.current_objects(relationship_id, rows);
                let guarded = GuardedRow::Search(outer.schema());
                Condition::all().add(SeaExpr::exists(self.completed(probe, guard_types, types, guarded, inner)))
            }
            InstanceScan::History(window) => self
                .windowed_objects(relationship_id, window)
                .map(|probe| self.completed(probe, guard_types, types, GuardedRow::Catalogue, inner.clone())) // clone: each of the window's probes tests the linked entity
                .into_exists(),
        }
    }

    /// The hop read inward, in the branch `outer` names: two selects of the
    /// `entity_key` of every Entity whose standing instance of the
    /// Relationship names an entity the type guard and `inner` keep.
    ///
    /// Each starts at the linked entities and reaches the instances naming
    /// them through an index over the object: a single object through
    /// `idx_ca_value_uri`, several through the containment
    /// `idx_ca_value_object` answers, since an array of objects holds plain
    /// URIs. A `value_long_uri` object is longer than any Entity identifier,
    /// so it names no stored entity and needs no select. An Entity appears
    /// once for each instance and object that reaches a kept entity.
    pub(crate) fn linking_branches(
        &self,
        relationship_id: AttributeNameKey,
        guard_types: &[AttributeName],
        types: &ResolvedNames<EntityTypeName>,
        outer: &EntityRowRefs,
        inner: Condition,
    ) -> [SelectStatement; 2] {
        let hop = || Alias::new(self.attributes.as_str());
        let linked_id = || SeaExpr::col((Alias::new(self.linked_entity.as_str()), Entities::Id));
        let single = SeaExpr::col((hop(), CurrentAttributes::ValueUri)).equals((Alias::new(self.linked_entity.as_str()), Entities::Id));
        let several = SimpleExpr::from(SeaExpr::col((hop(), CurrentAttributes::ValueObject))).array_contains(jsonb_build_array(linked_id()));
        // clone: both selects test the linked entity
        [
            self.linking_branch(relationship_id, guard_types, types, outer, inner.clone(), single),
            self.linking_branch(relationship_id, guard_types, types, outer, inner, several),
        ]
    }

    /// One select of [`Self::linking_branches`]: the Relationship's standing
    /// instances `names_linked` joins to the linked entity.
    fn linking_branch(
        &self,
        relationship_id: AttributeNameKey,
        guard_types: &[AttributeName],
        types: &ResolvedNames<EntityTypeName>,
        outer: &EntityRowRefs,
        inner: Condition,
        names_linked: SimpleExpr,
    ) -> SelectStatement {
        let hop = || Alias::new(self.attributes.as_str());
        let mut branch = Query::select();
        branch
            .column((hop(), CurrentAttributes::EntityKey))
            .from(outer.table(Entities::Table).alias(Alias::new(self.linked_entity.as_str())))
            .join_as(
                JoinType::Join,
                outer.table(CurrentAttributes::Table),
                hop(),
                Condition::all()
                    .add(SeaExpr::col((hop(), CurrentAttributes::AttributeNameKey)).eq(relationship_id.get()))
                    .add(names_linked),
            )
            .and_where(SeaExpr::col((hop(), CurrentAttributes::AttributeKind)).is_in(relationship_kinds()))
            .and_where(instance_stands(SeaExpr::col((hop(), CurrentAttributes::ExpiresAt))));
        self.completed(branch, guard_types, types, GuardedRow::Search(outer.schema()), inner)
    }

    /// One probe over the Relationship's instances, completed with the type
    /// guard, read from `guarded`, and the inner predicate on the entity each
    /// instance names.
    fn completed(
        &self,
        mut probe: SelectStatement,
        guard_types: &[AttributeName],
        types: &ResolvedNames<EntityTypeName>,
        guarded: GuardedRow<'_>,
        inner: Condition,
    ) -> SelectStatement {
        let linked = || Alias::new(self.linked_entity.as_str());
        // One identity row serves both APIs, so a hop over current state keeps
        // the linked entity only where it holds a current state; a hop over a
        // window lands on the Evolutions the history source reads, which hold
        // one already.
        match guarded {
            GuardedRow::Search(_) => {
                probe.and_where(holds_current_state(linked()));
            }
            GuardedRow::Catalogue => {}
        }
        // A multi-type guard is a disjunction; adding it as one condition keeps it
        // parenthesised so a second alternative cannot escape the correlation and
        // the attribute-name test to match any entity in the tenant that carries it.
        // The guard is the only reader of the linked entity's search row, so the
        // row is joined only when there is one.
        if !guard_types.is_empty() {
            let entity_type_name_keys = match guarded {
                GuardedRow::Search(schema) => {
                    let search = || Alias::new(self.linked_search.as_str());
                    probe.join_as(
                        JoinType::Join,
                        qualified(schema, EntitySearch::Table),
                        search(),
                        SeaExpr::col((search(), EntitySearch::EntityKey)).equals((linked(), Entities::EntityKey)),
                    );
                    SeaExpr::col((search(), EntitySearch::EntityTypeNameKeys))
                }
                GuardedRow::Catalogue => SeaExpr::col((linked(), Entities::EvolutionTypeNameKeys)),
            };
            probe.and_where(type_guard(guard_types, types, &entity_type_name_keys));
        }
        // The inner predicate is added as its own condition for the same reason,
        // before any future compilation of it renders a top-level `OR`.
        probe.cond_where(inner);
        probe
    }

    /// The Relationship's current instances joined to the entities they name,
    /// correlated against `outer`.
    fn current_objects(&self, relationship_id: AttributeNameKey, outer: &EntityRowRefs) -> SelectStatement {
        // Fresh alias handles rather than clones: each names the same relation
        // the hop introduced.
        let hop = || Alias::new(self.attributes.as_str());
        let single: SimpleExpr = Func::coalesce([
            SeaExpr::col((hop(), CurrentAttributes::ValueUri)),
            SeaExpr::col((hop(), CurrentAttributes::ValueLongUri)),
        ])
        .into();
        let mut probe = Query::select();
        probe.expr(int_literal(1)).from(outer.table(CurrentAttributes::Table).alias(hop()));
        self.join_linked_entities(
            &mut probe,
            SeaExpr::col((hop(), CurrentAttributes::ValueObject)),
            single,
            outer.table(Entities::Table),
        );
        probe
            // The correlation is added first: it binds nothing, so the
            // Relationship's dictionary key is still the probe's first bound
            // value, ahead of the guard's types and the inner predicate's operands.
            .and_where(SeaExpr::col((hop(), CurrentAttributes::EntityKey)).eq(outer.entity_key_column()))
            .and_where(SeaExpr::col((hop(), CurrentAttributes::AttributeNameKey)).eq(relationship_id.get()))
            .and_where(SeaExpr::col((hop(), CurrentAttributes::AttributeKind)).is_in(relationship_kinds()))
            // Clause 4.22: a past-due Relationship instance links nothing.
            .and_where(instance_stands(SeaExpr::col((hop(), CurrentAttributes::ExpiresAt))));
        probe
    }

    /// The Temporal Evolution this hop lands on over a window, which the inner
    /// predicate reads the history of.
    pub(crate) fn linked_evolution(&self) -> HistoryOf {
        HistoryOf::Entity(SeaExpr::col((Alias::new(self.linked_entity.as_str()), Entities::Id)))
    }

    /// The Relationship's instances valid within the window joined to the
    /// Temporal Evolutions they name in the window's branch, correlated
    /// against the window's temporal entity.
    ///
    /// History keeps a Relationship's objects as current state does, a single
    /// one in `value_uri` and several in `value_object`, so each instance
    /// expands the same way.
    fn windowed_objects(&self, relationship_id: AttributeNameKey, window: &HistoryWindow) -> WindowProbes {
        let aliases = HistoryAliases::at_depth(self.depth);
        window.probes_of(relationship_id, &aliases).map(|mut probe| {
            self.join_linked_entities(
                &mut probe,
                aliases.instance_column(AttributeHistory::ValueObject),
                aliases.instance_column(AttributeHistory::ValueUri),
                window.evolutions(Alias::new(self.linked_entity.as_str())),
            );
            probe.and_where(SeaExpr::expr(aliases.instance_column(AttributeHistory::AttributeKind)).is_in(relationship_kinds()));
            probe
        })
    }

    /// Join to `probe` one row per object its instance holds and the entity
    /// each object names in `entities`, a relation keyed by the Entity's `id`.
    ///
    /// Clause 4.9 follows every object: a Relationship holding one object
    /// keeps it in `single_object`, and one holding several, like a
    /// `ListRelationship`, keeps them in `value_object` as a JSON array of URIs
    /// in written order. Both expand to one row per object through
    /// `jsonb_array_elements_text`, the single object wrapped in a one-element
    /// array. The caller restricts the instance to [`relationship_kinds`]: the
    /// same attribute name may hold another family's `value_object`, which is
    /// no array of objects.
    fn join_linked_entities(&self, probe: &mut SelectStatement, value_object: SimpleExpr, single_object: SimpleExpr, entities: TableRef) {
        let linked = || Alias::new(self.linked_entity.as_str());
        let objects = || Alias::new(self.objects.as_str());
        let every_object: SimpleExpr = Func::coalesce([value_object, jsonb_build_array(single_object)]).into();
        probe
            .join(
                JoinType::Join,
                TableRef::FunctionCall(jsonb_array_elements_text(every_object), objects().into()),
                Condition::all(),
            )
            // The object is the linked Entity's URI (clause 4.5.3).
            .join_as(
                JoinType::Join,
                entities,
                linked(),
                SeaExpr::col((linked(), Entities::Id)).eq(SeaExpr::col(objects())),
            );
    }
}

/// Where a hop's type guard reads the linked entity's types.
#[derive(Debug, Clone, Copy)]
enum GuardedRow<'s> {
    /// The linked entity's `entity_search` row, in the branch schema given:
    /// a hop over current state, which lands on an Entity holding one.
    Search(Option<&'s DynIden>),
    /// The linked entity's Temporal Evolution the hop already lands on: a hop
    /// over a temporal window, which lands on an Entity holding one.
    Catalogue,
}

/// The attribute kinds whose instances hold objects a hop follows.
fn relationship_kinds() -> [SimpleExpr; 2] {
    [AttributeKind::Relationship, AttributeKind::ListRelationship].map(|kind| int_literal(i64::from(kind.code())))
}

/// The hop's entity-type constraint against the linked entity's
/// `entity_type_name_keys` column, wherever the hop reads it.
///
/// Clause 4.9 lists the guard's types as alternatives, so the constraint is a
/// disjunction of array-membership tests on the resolved dictionary keys, the
/// same leaf shape a `type` selection renders (clause 4.17), built through the
/// same [`array_contains`] helper so the two cannot drift. A guard type the
/// dictionary never recorded was never carried by any entity, so its arm is
/// constant `FALSE`.
///
/// An array-overlap operator would state the disjunction in one term instead of
/// one arm per type, but it cannot be written here: `&& ARRAY[$1, $2]` puts the
/// placeholders inside square brackets, and sea-query's tokeniser reads a
/// bracketed span as a quoted identifier and copies it out verbatim, leaving the
/// markers unsubstituted and silently aliased onto other binds. Each arm is
/// still a containment test the `entity_type_name_keys` GIN index answers, so the cost of
/// the disjunction is an extra index probe per type rather than a scan.
fn type_guard(guard_types: &[AttributeName], types: &ResolvedNames<EntityTypeName>, entity_type_name_keys: &SimpleExpr) -> SimpleExpr {
    guard_types
        .iter()
        .fold(Condition::any(), |guard, guard_type| match types.get_str(guard_type.as_str()) {
            // clone: every arm names the same column
            Some(id) => guard.add(array_contains_key(entity_type_name_keys.clone(), id.get())),
            None => guard.add(false_literal()),
        })
        .into()
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_key::AttributeNameKey,
        dictionary::resolved_names::ResolvedNames,
        entity_type_name_key::EntityTypeNameKey,
        history_source::{HistoryBranch, HistoryOf},
        q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, traversal::LinkedHop},
        window_reach::WindowReach,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, time::date_time::NgsiLdDateTime};
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query};

    /// Clause 5.7.4.4 with clause 4.11: a hop in a temporal `q` follows every
    /// target the Relationship named while valid within the window, the one in
    /// effect when the window opens included, so each of the window's probes
    /// joins the entity its instance names and correlates on the outer entity.
    #[test]
    fn a_windowed_hop_probes_both_the_window_and_the_instance_in_effect() {
        let start = "2019-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live())
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .axis(AttributeHistory::ObservedAt)
            .interval(TimeInterval::After(start))
            .reach(WindowReach::InsideAndInEffect)
            .build();
        let inner = Condition::all().add(Expr::col((Alias::new("le1"), Alias::new("entity_key"))).eq(5));
        let probe = LinkedHop::at_depth(1).probe(
            AttributeNameKey::from(4),
            &[],
            &ResolvedNames::default(),
            &InstanceScan::History(&window),
            inner,
        );
        let sql = Query::select().expr(Expr::val(1)).cond_where(probe).to_owned().to_string(PostgresQueryBuilder);
        assert_eq!(sql.matches(r#"AS "qh_ln1""#).count(), 2, "{sql}");
        for fragment in [r#") AS "le1" ON "le1"."id" = "lo1""#, r#""le1"."entity_key" = 5"#] {
            assert_eq!(sql.matches(fragment).count(), 2, "{fragment}: {sql}");
        }
        assert!(sql.contains(r#"AS "qe_ln1") AS "qh_ln1" ON TRUE"#), "{sql}");
    }

    /// Clause 4.9: a hop's type guard names the types the linked entity
    /// carries; over a window the hop lands on its Temporal Evolution, whose
    /// types the temporal catalogue holds, so no `entity_search` row is read.
    #[test]
    fn a_windowed_hop_guards_types_from_the_temporal_catalogue() {
        let start = "2019-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live_recorded())
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .axis(AttributeHistory::ObservedAt)
            .interval(TimeInterval::After(start))
            .reach(WindowReach::Inside)
            .build();
        let parking: EntityTypeName = "https://example.com/Parking".parse().expect("a valid type name");
        let guard: Vec<AttributeName> = vec!["https://example.com/Parking".parse().expect("a valid name")];
        let probe = LinkedHop::at_depth(1).probe(
            AttributeNameKey::from(4),
            &guard,
            &ResolvedNames::from_pairs([(parking, EntityTypeNameKey::from(3))]),
            &InstanceScan::History(&window),
            Condition::all(),
        );
        let sql = Query::select().expr(Expr::val(1)).cond_where(probe).to_owned().to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"(SELECT * FROM "entities" WHERE "entities"."evolution_created_at" IS NOT NULL) AS "le1""#),
            "{sql}"
        );
        assert!(sql.contains(r#""le1"."evolution_type_name_keys""#), "{sql}");
        assert!(!sql.contains("entity_search"), "{sql}");
    }

    /// Clause 4.9: a temporal hop follows every object a history instance
    /// holds, the several objects of a Relationship or a `ListRelationship` in
    /// `value_object` and a single one in `value_uri`, in each of the window's
    /// probes, and expands only an instance of those two kinds (2 and 8).
    #[test]
    fn a_windowed_hop_follows_every_object_of_a_history_instance() {
        let start = "2019-01-01T00:00:00Z".parse::<NgsiLdDateTime>().expect("a valid instant").get();
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live())
            .of(HistoryOf::Entity(Expr::col((Alias::new("e"), Alias::new("id")))))
            .axis(AttributeHistory::ObservedAt)
            .interval(TimeInterval::After(start))
            .reach(WindowReach::InsideAndInEffect)
            .build();
        let probe = LinkedHop::at_depth(1).probe(
            AttributeNameKey::from(4),
            &[],
            &ResolvedNames::default(),
            &InstanceScan::History(&window),
            Condition::all(),
        );
        let sql = Query::select().expr(Expr::val(1)).cond_where(probe).to_owned().to_string(PostgresQueryBuilder);
        for fragment in [
            r#"JOIN jsonb_array_elements_text(COALESCE("qh_ln1"."value_object", jsonb_build_array("qh_ln1"."value_uri"))) AS "lo1" ON TRUE"#,
            r#""qh_ln1"."attribute_kind" IN (2, 8)"#,
        ] {
            assert_eq!(sql.matches(fragment).count(), 2, "{fragment}: {sql}");
        }
        assert!(!sql.contains(r#""le1"."id" = "qh_ln1"."value_uri""#), "{sql}");
    }
}
