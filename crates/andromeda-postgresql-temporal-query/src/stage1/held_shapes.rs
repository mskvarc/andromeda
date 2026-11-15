//! Which disqualifying combinations of attribute, family and value shape the
//! instances an aggregated temporal representation aggregates carry.
//!
//! Clause 5.7.4.4 refuses an aggregated representation when a requested
//! Attribute of the selected Entities is not eligible for a requested method,
//! and the representation aggregates only the instances inside the temporal
//! window on its axis and of the datasets `datasetId` names (step S8). The
//! statement looks for each combination among exactly those instances, through
//! the predicate the aggregation statement reads them with, and stops at the
//! first instance carrying it.
//!
//! A query looks among the Entities stage 1 selects, unpaginated, so a page
//! never answers what another page of the same request refuses. The selection
//! is narrowed to the candidates first: the Evolutions holding a recorded
//! stream of one of the combinations in a requested dataset, reached through
//! the distinct shape masks holding it, and, where the branch reads its
//! queue, every Entity a queued intent names, whose entries no catalogue row
//! holds yet. The work therefore follows the streams carrying a refusing
//! shape, not the size of the selection.

use crate::{
    alias::{Ah, Rs},
    history::represented_instances,
    recorded_shape::recorded_shape,
    shape_mask::holds_combination,
    shape_mask_walk::{HeldMasks, held_masks},
    stage1::{
        entity_conditions::{PatternTest, build_entity_conditions_for_temporal},
        evolution_walk::{Among, evolution_rows},
        queued_candidates::{queued_candidates, stream_holder},
        window_match::temporal_match_subquery,
    },
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{dataset::DatasetFilter, temporal::compiled::CompiledTemporalQuery};
use andromeda_postgresql_query::{
    dataset_predicate::dataset_condition,
    error::RenderError,
    history_source::{BesideComparison, EvolutionColumns, HistoryBranch, HistoryOf, PendingEntries, attribute_rows::attribute_rows},
    one_time_gate::{Candidates, materialized},
    query_names::QueryNames,
};
use andromeda_postgresql_schema::{
    current::{E, Entities},
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{branch::qualified, literal::int_literal};
use andromeda_postgresql_stored_form::{history_grain::HistoryGrain, tombstone::carries_a_value};
use andromeda_postgresql_stored_values::{attribute_kind::AttributeKind, attribute_name_key::AttributeNameKey, recorded_shape::RecordedShape};
use sea_query::{Condition, Expr, ExprTrait, Iden, JoinType, Query, SelectStatement, UnionType, WithClause};

/// The name the statement projects the position of a carried combination
/// under.
#[derive(Iden, Clone, Copy)]
#[iden = "combination"]
pub struct CombinationColumn;

/// One combination of attribute, family and value shape the statement looks
/// for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeCombination {
    /// The attribute, by its dictionary key.
    pub attribute: AttributeNameKey,
    /// The family an instance was written as.
    pub kind: AttributeKind,
    /// The refined shape its value was recorded in.
    pub shape: RecordedShape,
}

/// The Entities whose instances the statement looks among.
#[derive(Debug, Clone, Copy)]
pub enum ShapeHolders<'a> {
    /// The one Entity a retrieval names (clause 5.7.3).
    Entity(&'a Identifier),
    /// Every Entity stage 1 selects.
    Selected,
}

/// The statement answering, under [`CombinationColumn`], the position in
/// `combinations` of each one an instance the representation `compiled`
/// aggregates carries, for the Entities `holders` names, read with the names
/// the query resolved; `None` when there is nothing to look for.
///
/// # Errors
///
/// Fails as rendering the entity conditions of stage 1 fails.
pub fn build_held_shapes_query(
    branch: &HistoryBranch,
    holders: ShapeHolders<'_>,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    combinations: &[ShapeCombination],
) -> Result<Option<SelectStatement>, RenderError> {
    let mut branches = Vec::with_capacity(combinations.len());
    for (position, combination) in (0_i64..).zip(combinations.iter().copied()) {
        let carried = match holders {
            ShapeHolders::Entity(entity_id) => carrying_instance(branch, &HistoryOf::Entity(Expr::val(entity_id.as_str())), compiled, names, combination)?,
            ShapeHolders::Selected => selected_holder(branch, compiled, names, combination)?,
        };
        let mut held = Query::select();
        held.expr_as(int_literal(position), CombinationColumn).and_where(Expr::exists(carried));
        branches.push(held.take());
    }
    let mut branches = branches.into_iter();
    let Some(mut statement) = branches.next() else {
        return Ok(None);
    };
    for held in branches {
        statement.union(UnionType::All, held);
    }
    match holders {
        ShapeHolders::Entity(_) => {}
        ShapeHolders::Selected => {
            statement.with_cte(
                WithClause::new()
                    .cte(materialized(candidates(branch, compiled, combinations), Candidates))
                    .to_owned(),
            );
        }
    }
    Ok(Some(statement))
}

/// `SELECT 1` from an Evolution stage 1 selects among the candidates whose
/// instances `combination` is carried by.
///
/// The Evolutions are those the count of stage 1 reads, narrowed to the
/// candidates, under every filter and the step S1 window match, and none is
/// paged.
fn selected_holder(
    branch: &HistoryBranch,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    combination: ShapeCombination,
) -> Result<SelectStatement, RenderError> {
    let narrowed = evolution_rows(branch, compiled, names, Among::Candidates);
    let conditions = build_entity_conditions_for_temporal(branch, compiled, names, narrowed.type_test, PatternTest::Tested)?;
    let evolution = HistoryOf::Evolution(EvolutionColumns {
        id: Expr::col((E, Entities::Id)),
        key: Expr::col((E, Entities::EntityKey)),
    });
    let mut holder = Query::select();
    holder
        .expr(int_literal(1))
        .from(narrowed.counted)
        .cond_where(conditions)
        .and_where(Expr::exists(temporal_match_subquery(branch, compiled, names, BesideComparison::Key)))
        .and_where(Expr::exists(carrying_instance(branch, &evolution, compiled, names, combination)?));
    Ok(holder.take())
}

/// `SELECT 1` from an instance of the Evolution `of` names that the
/// representation `compiled` aggregates and that carries `combination`.
///
/// Tombstones carry no value and so no shape.
fn carrying_instance(
    branch: &HistoryBranch,
    of: &HistoryOf,
    compiled: &CompiledTemporalQuery,
    names: &QueryNames,
    combination: ShapeCombination,
) -> Result<SelectStatement, RenderError> {
    let mut instance = Query::select();
    instance.expr(int_literal(1));
    instance
        .from(attribute_rows(branch, of, Ah))
        .cond_where(represented_instances(branch, of, compiled, names)?)
        .and_where(Expr::col((Ah, HistoryGrain::AttributeNameKey)).eq(combination.attribute.get()))
        .and_where(Expr::col((Ah, AttributeHistory::AttributeKind)).eq(combination.kind.code()))
        .and_where(carries_a_value(Ah))
        .and_where(recorded_shape().eq(combination.shape.code()));
    Ok(instance.take())
}

/// The Evolutions holding a recorded stream of one of `combinations` in a
/// dataset `compiled` names: their surrogate keys over recorded rows alone;
/// where the branch reads its queue, their identifiers beside every queued
/// Entity, whose queued entries no catalogue row holds.
///
/// The streams are reached through the distinct masks of the combinations'
/// attributes that hold one of them, one index range each, so a combination
/// no stream holds costs the walk of its attribute's masks alone.
fn candidates(branch: &HistoryBranch, compiled: &CompiledTemporalQuery, combinations: &[ShapeCombination]) -> SelectStatement {
    let held = |column: TemporalAttributeStreams| Expr::col((HeldMasks, column));
    let carried = combinations.iter().fold(Condition::any(), |carried, combination| {
        carried.add(
            Condition::all()
                .add(held(TemporalAttributeStreams::AttributeNameKey).eq(combination.attribute.get()))
                .add(holds_combination(
                    held(TemporalAttributeStreams::ShapeMask),
                    combination.kind,
                    combination.shape,
                )),
        )
    });
    let mut attributes: Vec<i32> = combinations.iter().map(|combination| combination.attribute.get()).collect();
    attributes.sort_unstable();
    attributes.dedup();
    let attributes = attributes;
    let mut candidates = Query::select();
    candidates
        .from(HeldMasks)
        .join_as(
            JoinType::Join,
            qualified(branch.schema(), TemporalAttributeStreams::Table),
            Rs,
            Expr::col((Rs, TemporalAttributeStreams::AttributeNameKey))
                .equals((HeldMasks, TemporalAttributeStreams::AttributeNameKey))
                .and(Expr::col((Rs, TemporalAttributeStreams::ShapeMask)).equals((HeldMasks, TemporalAttributeStreams::ShapeMask))),
        )
        .cond_where(carried)
        .cond_where(dataset_condition(
            &Expr::col((Rs, TemporalAttributeStreams::DatasetKey)),
            &DatasetFilter::from_selectors(&compiled.instances.dataset_id_filter),
        ))
        .with_cte(held_masks(branch, &attributes, None));
    stream_holder(&mut candidates, branch);
    match branch.pending() {
        PendingEntries::Absent => {}
        PendingEntries::Unioned => {
            candidates.union(UnionType::All, queued_candidates(branch));
        }
    }
    candidates
}

#[cfg(test)]
mod tests {
    use crate::stage1::{
        held_shapes::{ShapeCombination, ShapeHolders, build_held_shapes_query},
        query_fixture::{instant, minimal_compiled, typed, vehicle_is_known, vehicle_selector},
    };
    use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
    use andromeda_ngsi_ld_query::{
        dataset::DatasetSelector,
        temporal::{compiled::CompiledTemporalQuery, window::TimeInterval},
    };
    use andromeda_postgresql_query::{history_source::HistoryBranch, query_names::QueryNames};
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_kind::AttributeKind, attribute_name_key::AttributeNameKey, recorded_shape::RecordedShape};
    use sea_query::{Alias, IntoIden, SelectStatement};

    /// A `String` recorded as a `Property` under the attribute keyed 7.
    fn string_load() -> ShapeCombination {
        ShapeCombination {
            attribute: AttributeNameKey::from(7),
            kind: AttributeKind::Property,
            shape: RecordedShape::String,
        }
    }

    fn truck() -> Identifier {
        "urn:ngsi-ld:Truck:1".parse().expect("valid URI")
    }

    /// A query with every filter absent but a window after 2024 on `axis`.
    fn after_2024(axis: TimeProp) -> CompiledTemporalQuery {
        let mut compiled = minimal_compiled();
        compiled.window.timeproperty = axis;
        compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
        compiled
    }

    fn rendered(
        branch: &HistoryBranch,
        holders: ShapeHolders<'_>,
        names: &QueryNames,
        compiled: &CompiledTemporalQuery,
        combinations: &[ShapeCombination],
    ) -> SelectStatement {
        build_held_shapes_query(branch, holders, compiled, names, combinations)
            .expect("renderable")
            .expect("a combination to look for")
    }

    #[test]
    fn nothing_to_look_for_builds_no_statement() {
        let built = build_held_shapes_query(
            &HistoryBranch::live_recorded(),
            ShapeHolders::Entity(&truck()),
            &minimal_compiled(),
            &QueryNames::default(),
            &[],
        )
        .expect("renderable");
        assert!(built.is_none());
    }

    /// Clause 5.7.3: a retrieval aggregates the instances of the one Entity it
    /// names inside the window, so no catalogue of any other Entity is read.
    #[test]
    fn a_retrieval_looks_among_the_instances_of_its_entity_inside_the_window() {
        let statement = rendered(
            &HistoryBranch::live_recorded(),
            ShapeHolders::Entity(&truck()),
            &QueryNames::default(),
            &after_2024(TimeProp::Observed),
            &[string_load()],
        );
        assert_statement_snapshot!("held-shapes-a-retrieval-looks-among-the-instances-of-its-entity-inside-the-window", statement);
    }

    /// Clause 5.2.21 places an instance on the axis the request names, so the
    /// window is read on that axis alone.
    #[test]
    fn the_window_is_read_on_the_requested_axis() {
        let statement = rendered(
            &HistoryBranch::live_recorded(),
            ShapeHolders::Entity(&truck()),
            &QueryNames::default(),
            &after_2024(TimeProp::Modified),
            &[string_load()],
        );
        assert_statement_snapshot!("held-shapes-the-window-is-read-on-the-requested-axis", statement);
    }

    #[test]
    fn each_combination_answers_its_own_position() {
        let array = ShapeCombination {
            shape: RecordedShape::Array,
            ..string_load()
        };
        let statement = rendered(
            &HistoryBranch::live_recorded(),
            ShapeHolders::Entity(&truck()),
            &QueryNames::default(),
            &after_2024(TimeProp::Observed),
            &[string_load(), array],
        );
        assert_statement_snapshot!("held-shapes-each-combination-answers-its-own-position", statement);
    }

    /// Clause 5.7.4.4 judges the Entities the query selects: the selection is
    /// read narrowed to the Evolutions holding a stream of the combination,
    /// under every filter stage 1 applies, and unpaginated.
    #[test]
    fn a_query_looks_among_the_selected_holders_of_the_combination() {
        let statement = rendered(
            &HistoryBranch::live_recorded(),
            ShapeHolders::Selected,
            &vehicle_is_known(),
            &{
                let mut compiled = typed(vec![vehicle_selector()]);
                compiled.window.interval = TimeInterval::After(instant("2024-01-01T00:00:00Z"));
                compiled
            },
            &[string_load()],
        );
        assert_statement_snapshot!("held-shapes-a-query-looks-among-the-selected-holders-of-the-combination", statement);
    }

    /// Clause 5.7.4.4 step S8: the instances of the datasets `datasetId` names
    /// are the ones aggregated, so both the candidates and the instances are
    /// read in those datasets.
    #[test]
    fn the_dataset_parameter_narrows_the_candidates_and_the_instances() {
        let mut compiled = after_2024(TimeProp::Observed);
        compiled.instances.dataset_id_filter = vec![DatasetSelector::parse("urn:ngsi-ld:Dataset:one").expect("a valid dataset selector")];
        let statement = rendered(
            &HistoryBranch::live_recorded(),
            ShapeHolders::Selected,
            &QueryNames::default(),
            &compiled,
            &[string_load()],
        );
        assert_statement_snapshot!("held-shapes-the-dataset-parameter-narrows-the-candidates-and-the-instances", statement);
    }

    /// No catalogue row holds a queued entry yet, so every Entity a queued
    /// intent names is a candidate, and its instances are read with the queued
    /// entries the application would write.
    #[test]
    fn a_queued_branch_adds_every_queued_entity_to_the_candidates() {
        let statement = rendered(
            &HistoryBranch::live(),
            ShapeHolders::Selected,
            &QueryNames::default(),
            &after_2024(TimeProp::Observed),
            &[string_load()],
        );
        assert_statement_snapshot!("held-shapes-a-queued-branch-adds-every-queued-entity-to-the-candidates", statement);
    }

    #[test]
    fn a_snapshot_branch_reads_its_own_catalogue() {
        let snapshot = HistoryBranch::recorded_in(Alias::new("snap_1").into_iden());
        let statement = rendered(
            &snapshot,
            ShapeHolders::Selected,
            &QueryNames::default(),
            &after_2024(TimeProp::Observed),
            &[string_load()],
        );
        assert_statement_snapshot!("held-shapes-a-snapshot-branch-reads-its-own-catalogue", statement);
    }
}
