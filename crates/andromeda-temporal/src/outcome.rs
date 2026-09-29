//! Explicit residual and pagination outcomes returned by temporal readers.

use andromeda_entity::{query_residual::ResidualFilter, residual_filter};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    entity_selection::EntitySelection,
    pagination::{CountPolicy, Pagination},
};
use serde_json::Value;

/// Entity-selection predicates a temporal reader did not apply.
#[derive(Debug, Clone, Default)]
pub struct TemporalResidual(EntitySelection);

impl TemporalResidual {
    /// Construct a residual from the exact compiled selection vocabulary.
    #[must_use]
    pub const fn over(selection: EntitySelection) -> TemporalResidual {
        TemporalResidual(selection)
    }

    /// Borrow the unapplied entity selection.
    #[must_use]
    pub const fn selection(&self) -> &EntitySelection {
        &self.0
    }

    /// Whether every selection predicate was applied by the reader.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        let selection = &self.0;
        !selection.selectors_constrain()
            && selection.attrs.is_empty()
            && selection.q_expr.is_none()
            && selection.geo.is_none()
            && selection.scope_q.is_none()
            && !selection.projected_members.constrains()
    }
}

/// What a temporal reader did with the requested result window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalPaginationOutcome {
    /// The reader applied the page after all predicates and optionally counted every match.
    Applied {
        /// Total matching entities when count was requested.
        total: Option<i64>,
    },
    /// The reader returned all candidates so policy can filter before windowing.
    Deferred,
}

/// Stage-one identifiers together with explicit residual and pagination behavior.
#[derive(Debug)]
pub struct TemporalEntityIdOutcome {
    /// Candidate entities in deterministic result order.
    pub candidates: Vec<TemporalEntityCandidate>,
    /// Predicates the reader declined.
    pub residual: TemporalResidual,
    /// Whether pagination and count were applied.
    pub pagination: TemporalPaginationOutcome,
}

impl TemporalEntityIdOutcome {
    /// Validate the outcome before policy consumes it.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidTemporalOutcome::ResidualAfterPagination`] when the
    /// reader paginated while leaving a predicate for policy to evaluate, and
    /// [`InvalidTemporalOutcome::ResidualWithoutSelectionValue`] when a
    /// residual predicate remains but a candidate carries no expanded
    /// selection value to evaluate it against.
    pub fn validate(&self) -> Result<(), InvalidTemporalOutcome> {
        if !self.residual.is_empty() && matches!(self.pagination, TemporalPaginationOutcome::Applied { .. }) {
            return Err(InvalidTemporalOutcome::ResidualAfterPagination);
        }
        if !self.residual.is_empty() && self.candidates.iter().any(|candidate| candidate.selection_value.is_none()) {
            return Err(InvalidTemporalOutcome::ResidualWithoutSelectionValue);
        }
        Ok(())
    }

    /// Complete residual evaluation and deferred pagination, yielding identifiers and optional total count.
    ///
    /// # Errors
    ///
    /// Returns whatever [`TemporalEntityIdOutcome::validate`] refuses: an
    /// outcome that paginated ahead of a residual predicate, or one whose
    /// candidates cannot be evaluated against the predicate that remains; and
    /// [`InvalidTemporalOutcome::UnreadableCandidate`] for a candidate whose
    /// identity the residual predicate cannot read.
    pub fn complete(self, pagination: Pagination) -> Result<(Vec<Identifier>, Option<i64>), InvalidTemporalOutcome> {
        self.validate()?;
        let mut candidates = self.candidates;
        if !self.residual.is_empty() {
            let residual = ResidualFilter::over(self.residual.0);
            let mut admitted = Vec::with_capacity(candidates.len());
            for candidate in candidates {
                let Some(selection_value) = candidate.selection_value.as_ref() else {
                    continue;
                };
                if residual_filter::matches(&residual, selection_value).map_err(|_| InvalidTemporalOutcome::UnreadableCandidate)? {
                    admitted.push(candidate);
                }
            }
            candidates = admitted;
        }
        let identifiers = candidates.into_iter().map(|candidate| candidate.entity_id).collect();
        match self.pagination {
            TemporalPaginationOutcome::Applied { total } => Ok((identifiers, total)),
            TemporalPaginationOutcome::Deferred => Ok(apply_deferred_page(identifiers, pagination)),
        }
    }
}

/// One temporal stage-one candidate and the neutral expanded value used only for residual evaluation.
#[derive(Debug)]
pub struct TemporalEntityCandidate {
    /// Candidate entity identifier.
    pub entity_id: Identifier,
    /// Expanded selection value for the requested temporal window when any predicate remains residual.
    pub selection_value: Option<Value>,
}

/// A temporal reader reported a semantically impossible outcome.
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum InvalidTemporalOutcome {
    /// Pagination cannot precede a predicate that may remove candidates.
    #[error("temporal reader applied pagination while returning an unapplied predicate")]
    ResidualAfterPagination,
    /// Residual evaluation requires a neutral expanded selection value for every candidate.
    #[error("temporal reader returned a residual predicate without a selection value")]
    ResidualWithoutSelectionValue,
    /// A candidate's selection value carries an `@id` or `@type` the residual
    /// predicate cannot read.
    #[error("temporal reader returned a candidate whose identity the residual predicate cannot read")]
    UnreadableCandidate,
}

/// Apply a deferred page after policy finishes residual evaluation.
#[must_use]
pub fn apply_deferred_page<T>(rows: Vec<T>, pagination: Pagination) -> (Vec<T>, Option<i64>) {
    let total = (pagination.count == CountPolicy::Emit).then(|| i64::try_from(rows.len()).unwrap_or(i64::MAX));
    let offset = usize::try_from(pagination.offset).unwrap_or(usize::MAX);
    let limit = usize::try_from(pagination.limit).unwrap_or(usize::MAX);
    (rows.into_iter().skip(offset).take(limit).collect(), total)
}

#[cfg(test)]
mod tests {
    use crate::outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual};
    use andromeda_entity::{query_residual::ResidualFilter, residual_filter};
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::parser,
        geo::query::GeoQuery,
        pagination::{CountPolicy, Pagination},
        scope::query::ScopeQuery,
    };
    use serde_json::{Value, json};
    use std::str::FromStr;

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const TEMPERATURE: &str = "https://example.com/temperature";
    const SCOPE: &str = "https://uri.etsi.org/ngsi-ld/scope";

    fn entity(identifier: &str, temperature: f64, longitude: f64, scope: &str) -> Value {
        json!([{
            "@id": identifier,
            TEMPERATURE: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}],
            LOCATION: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [longitude, 52.5]}}]}],
            SCOPE: [{"@value": scope}]
        }])
    }

    fn combined_selection() -> EntitySelection {
        EntitySelection::builder()
            .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q")))
            .geo(Some(GeoQuery {
                georel: GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                    min_distance: None,
                },
                geometry: Geometry::from_geojson_value(&json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid point"),
                geoproperty: LOCATION.parse().expect("valid location attribute name"),
            }))
            .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scope query")))
            .build()
    }

    fn candidates(values: &[(Identifier, Value)], pushed: &EntitySelection) -> Vec<TemporalEntityCandidate> {
        let pushed = ResidualFilter::over(pushed.clone());
        values
            .iter()
            .filter(|(_, value)| residual_filter::matches(&pushed, value).expect("the fixtures carry readable identities"))
            .map(|(entity_id, value)| TemporalEntityCandidate {
                entity_id: entity_id.clone(),
                selection_value: Some(value.clone()),
            })
            .collect()
    }

    #[test]
    fn q_geo_and_scope_residuals_match_full_pushdown_before_pagination() {
        let values = [
            (
                "urn:ngsi-ld:Entity:1".parse().expect("valid id"),
                entity("urn:ngsi-ld:Entity:1", 30.0, 13.4, "/A/B"),
            ),
            (
                "urn:ngsi-ld:Entity:2".parse().expect("valid id"),
                entity("urn:ngsi-ld:Entity:2", 20.0, 13.4, "/A/B"),
            ),
            (
                "urn:ngsi-ld:Entity:3".parse().expect("valid id"),
                entity("urn:ngsi-ld:Entity:3", 40.0, 15.4, "/A/B"),
            ),
            (
                "urn:ngsi-ld:Entity:4".parse().expect("valid id"),
                entity("urn:ngsi-ld:Entity:4", 50.0, 13.4, "/A/C"),
            ),
            (
                "urn:ngsi-ld:Entity:5".parse().expect("valid id"),
                entity("urn:ngsi-ld:Entity:5", 60.0, 13.4, "/A/D"),
            ),
            (
                "urn:ngsi-ld:Entity:6".parse().expect("valid id"),
                entity("urn:ngsi-ld:Entity:6", 70.0, 13.4, "/B"),
            ),
        ];
        let full = combined_selection();
        let pagination = Pagination {
            limit: 1,
            offset: 1,
            count: CountPolicy::Emit,
        };
        let full_matches = candidates(&values, &full);
        let full_pushdown = TemporalEntityIdOutcome {
            candidates: vec![TemporalEntityCandidate {
                entity_id: full_matches[1].entity_id.clone(),
                selection_value: None,
            }],
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Applied { total: Some(3) },
        }
        .complete(pagination)
        .expect("valid full-pushdown outcome");

        for residual in [
            EntitySelection::builder().q_expr(full.q_expr.clone()).build(),
            EntitySelection::builder().geo(full.geo.clone()).build(),
            EntitySelection::builder().scope_q(full.scope_q.clone()).build(),
            full.clone(),
        ] {
            let mut pushed = full.clone();
            ResidualFilter::over(residual.clone()).strip_from(&mut pushed);
            let residual_outcome = TemporalEntityIdOutcome {
                candidates: candidates(&values, &pushed),
                residual: TemporalResidual::over(residual),
                pagination: TemporalPaginationOutcome::Deferred,
            }
            .complete(pagination)
            .expect("valid residual outcome");
            pretty_assertions::assert_eq!(residual_outcome, full_pushdown);
        }

        let selected_observations = full_pushdown.0.iter().map(|identifier| format!("observation:{identifier}")).collect::<Vec<_>>();
        pretty_assertions::assert_eq!(selected_observations, vec!["observation:urn:ngsi-ld:Entity:4"]);
    }
}
