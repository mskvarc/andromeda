//! Reading which disqualifying shapes the instances an aggregated temporal
//! representation aggregates carry, which clause 5.7.4.4 decides aggregation
//! eligibility from once the shapes held anywhere in the branch did not.

use crate::{
    error::TemporalStoreError,
    reader::stored_attribute_shape,
    repo::TemporalRepository,
    temporal_query_execution::TemporalStatement,
    temporal_store_invariant::TemporalStoreInvariant,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_postgresql_query::history_source::HistoryBranch;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_temporal_query::stage1::held_shapes::{
    CombinationColumn,
    ShapeCombination,
    ShapeHolders as StatementHolders,
    build_held_shapes_query,
};
use andromeda_temporal::{
    error::TemporalError,
    port::history_reader::{RecordedAttributeShape, ShapeHolders},
};
use sea_query::Iden;
use sqlx::Row;
use std::collections::{HashMap, HashSet};

impl TemporalRepository {
    /// The combinations among `shapes` that an instance `compiled` aggregates
    /// carries, for the Entities `holders` names.
    ///
    /// An attribute the dictionary never keyed has no instance, so none of its
    /// combinations is looked for.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a dictionary lookup fails, the statement
    /// cannot be rendered or fails, or it answers a combination it was not
    /// asked for.
    pub(crate) async fn held_shapes(
        &self,
        branch: &HistoryBranch,
        holders: ShapeHolders<'_>,
        compiled: &CompiledTemporalQuery,
        shapes: &HashMap<AttributeName, HashSet<RecordedAttributeShape>>,
    ) -> Result<HashMap<AttributeName, HashSet<RecordedAttributeShape>>, TemporalError> {
        let names = self.resolve_walk_names(branch, compiled).await?;
        let (asked, combinations): (Vec<(&AttributeName, RecordedAttributeShape)>, Vec<ShapeCombination>) = shapes
            .iter()
            .filter_map(|(attribute, shapes)| names.attributes.get_str(attribute.as_str()).map(|key| (attribute, key, shapes)))
            .flat_map(|(attribute, key, shapes)| {
                shapes.iter().map(move |shape| {
                    let stored = stored_attribute_shape(*shape);
                    (
                        (attribute, *shape),
                        ShapeCombination {
                            attribute: key,
                            kind: stored.kind,
                            shape: stored.shape,
                        },
                    )
                })
            })
            .unzip();
        let holders = match holders {
            ShapeHolders::Entity(entity_id) => StatementHolders::Entity(entity_id),
            ShapeHolders::Selected => StatementHolders::Selected,
        };
        let Some(statement) = build_held_shapes_query(branch, holders, compiled, &names, &combinations).map_err(TemporalStoreError::Render)? else {
            return Ok(HashMap::new());
        };
        // The candidates a combination names range from none to most of the
        // branch, so the plan is chosen for the combinations bound.
        let rows = self
            .execute_temporal_query(TemporalStatement::HeldShapes, &statement, StatementPlanning::BoundValues)
            .await?;
        let mut held: HashMap<AttributeName, HashSet<RecordedAttributeShape>> = HashMap::new();
        for row in rows {
            let position: i32 = row.try_get(Iden::to_string(&CombinationColumn).as_str()).map_err(TemporalStoreError::from)?;
            let (attribute, shape) = usize::try_from(position)
                .ok()
                .and_then(|position| asked.get(position))
                .ok_or(TemporalStoreError::Invariant(TemporalStoreInvariant::HeldShapeUnasked(position)))?;
            // clone: the answer is keyed by the name the request asked about.
            held.entry((*attribute).clone()).or_default().insert(*shape);
        }
        Ok(held)
    }
}
