//! The clause 4.9 linked-entity traversal axis compiled to a correlated probe.
//!
//! `rel{attr}=="x"` asks about an attribute of the entity a Relationship points
//! at, so answering it means leaving the row being filtered. One hop becomes one
//! `EXISTS`: the Relationship's instances are read from `current_attributes`,
//! each object they hold is expanded to one row and joined to `entities.id` to
//! reach the entity it names, and the rest of the predicate is compiled against
//! that entity. Hops nest, so the probes nest with them.
//!
//! The join stays on `entities.id` because a Relationship's object is the
//! Entity's URI (clause 4.5.3); it is the surrogate `entity_key` of the row that
//! join lands on that the inner predicate then correlates against. The linked
//! entity's `entity_search` row is joined only when the hop carries a type
//! guard, since that is the one thing a hop reads from it.
//!
//! This lives apart from the comparison rendering next door because it changes
//! *which row* is being asked about rather than *what is asked* of it; the two
//! change for unrelated reasons.

use crate::{
    attribute_name_key::AttributeNameKey,
    dictionary::resolved_names::ResolvedNames,
    entity_row_refs::EntityRowRefs,
    instance_expiry::instance_stands,
    q_predicate::{
        history_window::{HistoryAliases, HistoryWindow, WindowProbes},
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
    /// to `entities.id` is that table's unique external key.
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
                Condition::all().add(SeaExpr::exists(self.completed(probe, guard_types, types, outer, inner)))
            }
            InstanceScan::History(window) => self
                .windowed_objects(relationship_id, window)
                .map(|probe| self.completed(probe, guard_types, types, outer, inner.clone())) // clone: each of the window's probes tests the linked entity
                .into_exists(),
        }
    }

    /// One probe over the Relationship's instances, completed with the type
    /// guard and the inner predicate on the entity each instance names.
    fn completed(
        &self,
        mut probe: SelectStatement,
        guard_types: &[AttributeName],
        types: &ResolvedNames<EntityTypeName>,
        outer: &InstanceScan<'_>,
        inner: Condition,
    ) -> SelectStatement {
        let linked = || Alias::new(self.linked_entity.as_str());
        // A multi-type guard is a disjunction; adding it as one condition keeps it
        // parenthesised so a second alternative cannot escape the correlation and
        // the attribute-name test to match any entity in the tenant that carries it.
        // The guard is the only reader of the linked entity's search row, so the
        // row is joined only when there is one.
        if !guard_types.is_empty() {
            let search = || Alias::new(self.linked_search.as_str());
            probe
                .join_as(
                    JoinType::Join,
                    qualified(outer.schema(), EntitySearch::Table),
                    search(),
                    SeaExpr::col((search(), EntitySearch::EntityKey)).equals((linked(), Entities::EntityKey)),
                )
                .and_where(type_guard(guard_types, types, self.linked_search.as_str()));
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

    /// The Relationship's instances valid within the window joined to the
    /// entities they name, correlated against the window's temporal entity.
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
                qualified(window.schema(), Entities::Table),
            );
            probe.and_where(SeaExpr::expr(aliases.instance_column(AttributeHistory::AttributeKind)).is_in(relationship_kinds()));
            probe
        })
    }

    /// Join to `probe` one row per object its instance holds and the entity
    /// each object names in `entities`.
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

/// The attribute kinds whose instances hold objects a hop follows.
fn relationship_kinds() -> [SimpleExpr; 2] {
    [AttributeKind::Relationship, AttributeKind::ListRelationship].map(|kind| int_literal(i64::from(kind.code())))
}

/// The hop's entity-type constraint against the linked entity's `entity_type_name_keys`
/// column.
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
fn type_guard(guard_types: &[AttributeName], types: &ResolvedNames<EntityTypeName>, linked_search_alias: &str) -> SimpleExpr {
    let entity_type_name_keys: SimpleExpr = SeaExpr::col((Alias::new(linked_search_alias), EntitySearch::EntityTypeNameKeys));
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
        history_source::HistoryBranch,
        q_predicate::{history_window::HistoryWindow, instance_scan::InstanceScan, traversal::LinkedHop},
    };
    use andromeda_ngsi_ld_query::temporal::window::TimeInterval;
    use andromeda_postgresql_schema::temporal::AttributeHistory;
    use chrono::{DateTime, Utc};
    use sea_query::{Alias, Condition, Expr, ExprTrait, PostgresQueryBuilder, Query};

    /// Clause 5.7.4.4 with clause 4.11: a hop in a temporal `q` follows every
    /// target the Relationship named while valid within the window, the one in
    /// effect when the window opens included, so each of the window's probes
    /// joins the entity its instance names and correlates on the outer entity.
    #[test]
    fn a_windowed_hop_probes_both_the_window_and_the_instance_in_effect() {
        let start = DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live())
            .entity_id(Expr::col((Alias::new("e"), Alias::new("id"))))
            .axis(AttributeHistory::ObservedAt)
            .interval(TimeInterval::After(start))
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
        for fragment in [r#"JOIN "entities" AS "le1" ON "le1"."id" = "lo1""#, r#""le1"."entity_key" = 5"#] {
            assert_eq!(sql.matches(fragment).count(), 2, "{fragment}: {sql}");
        }
        assert!(sql.contains(r#"AS "qe_ln1" ON TRUE"#), "{sql}");
    }

    /// Clause 4.9: a temporal hop follows every object a history instance
    /// holds, the several objects of a Relationship or a `ListRelationship` in
    /// `value_object` and a single one in `value_uri`, in each of the window's
    /// probes, and expands only an instance of those two kinds (2 and 8).
    #[test]
    fn a_windowed_hop_follows_every_object_of_a_history_instance() {
        let start = DateTime::parse_from_rfc3339("2019-01-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        let window = HistoryWindow::builder()
            .branch(HistoryBranch::live())
            .entity_id(Expr::col((Alias::new("e"), Alias::new("id"))))
            .axis(AttributeHistory::ObservedAt)
            .interval(TimeInterval::After(start))
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
