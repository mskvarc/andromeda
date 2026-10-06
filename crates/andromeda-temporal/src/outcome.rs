//! Explicit residual and pagination outcomes returned by temporal readers.

use crate::observed_selection::ObservedSelection;
use andromeda_entity::query_residual::ResidualFilter;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity_selection::EntitySelection, page_window::PageWindow};

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
    /// The reader returned every candidate, in the query's order, so policy
    /// filters, counts and cuts the window itself.
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
    /// residual predicate remains but a candidate carries no observed entity
    /// to evaluate it against.
    pub fn validate(&self) -> Result<(), InvalidTemporalOutcome> {
        if !self.residual.is_empty() && matches!(self.pagination, TemporalPaginationOutcome::Applied { .. }) {
            return Err(InvalidTemporalOutcome::ResidualAfterPagination);
        }
        if !self.residual.is_empty() && self.candidates.iter().any(|candidate| candidate.selection.is_none()) {
            return Err(InvalidTemporalOutcome::ResidualWithoutSelectionValue);
        }
        Ok(())
    }

    /// Complete residual evaluation and deferred pagination, yielding the
    /// identifiers `window` selects and, when `count` asks for it, the total.
    ///
    /// A deferred outcome holds every candidate in the query's order, so the
    /// clause 6.3.13 total is the count of every one that survived the residual
    /// and the window -- offset or cursor -- is cut from all of them.
    ///
    /// # Errors
    ///
    /// Returns whatever [`TemporalEntityIdOutcome::validate`] refuses: an
    /// outcome that paginated ahead of a residual predicate, or one whose
    /// candidates cannot be evaluated against the predicate that remains.
    pub fn complete(self, window: &PageWindow, count: CountPolicy) -> Result<(Vec<Identifier>, Option<i64>), InvalidTemporalOutcome> {
        self.validate()?;
        let mut candidates = self.candidates;
        if !self.residual.is_empty() {
            let residual = ResidualFilter::over(self.residual.0);
            candidates.retain(|candidate| candidate.selection.as_ref().is_some_and(|observed| observed.matches(&residual)));
        }
        let identifiers: Vec<Identifier> = candidates.into_iter().map(|candidate| candidate.entity_id).collect();
        match self.pagination {
            TemporalPaginationOutcome::Applied { total } => Ok((identifiers, total)),
            TemporalPaginationOutcome::Deferred => {
                let total = match count {
                    CountPolicy::Emit => Some(i64::try_from(identifiers.len()).unwrap_or(i64::MAX)),
                    CountPolicy::Omit => None,
                };
                Ok((window.cut(identifiers, Identifier::as_str), total))
            }
        }
    }
}

/// One temporal stage-one candidate and what was observed of it, used only for
/// residual evaluation.
#[derive(Debug)]
pub struct TemporalEntityCandidate {
    /// Candidate entity identifier.
    pub entity_id: Identifier,
    /// What was observed of the entity in the requested temporal window, when
    /// any predicate remains residual.
    pub selection: Option<ObservedSelection>,
}

/// A temporal reader reported a semantically impossible outcome.
#[derive(Debug, Clone, Copy, thiserror::Error, PartialEq, Eq)]
pub enum InvalidTemporalOutcome {
    /// Pagination cannot precede a predicate that may remove candidates.
    #[error("temporal reader applied pagination while returning an unapplied predicate")]
    ResidualAfterPagination,
    /// Residual evaluation requires an observed entity for every candidate.
    #[error("temporal reader returned a residual predicate without a selection value")]
    ResidualWithoutSelectionValue,
}

#[cfg(test)]
mod tests {
    use crate::{
        observed_selection::ObservedSelection,
        outcome::{TemporalEntityCandidate, TemporalEntityIdOutcome, TemporalPaginationOutcome, TemporalResidual},
    };
    use andromeda_entity::query_residual::ResidualFilter;
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_geometry::{geo_rel::GeoRel, geometry::Geometry, positive_distance::PositiveDistance};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        entity_selection::EntitySelection,
        filter::parser,
        geo::query::GeoQuery,
        page_window::{PageStart, PageWindow},
        scope::query::ScopeQuery,
    };
    use serde_json::json;
    use std::{str::FromStr, sync::Arc};

    const LOCATION: &str = "https://uri.etsi.org/ngsi-ld/location";
    const TEMPERATURE: &str = "https://example.com/temperature";
    const SCOPE: &str = "https://uri.etsi.org/ngsi-ld/scope";

    fn entity(identifier: &str, temperature: f64, longitude: f64, scope: &str) -> Arc<EntityState> {
        let stored = json!([{
            "@id": identifier,
            "@type": ["https://example.com/Sensor"],
            TEMPERATURE: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}],
            LOCATION: [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": longitude}, {"@value": 52.5}]}]
                }]
            }],
            SCOPE: [{"@value": scope}]
        }]);
        Arc::new(EntityState::from_slice(&serde_json::to_vec(&stored).expect("JSON")).expect("a stored entity decodes"))
    }

    fn combined_selection() -> EntitySelection {
        EntitySelection::builder()
            .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q")))
            .geo(Some(GeoQuery {
                georel: GeoRel::Near {
                    max_distance: Some(PositiveDistance::from_str("1000").expect("positive distance")),
                    min_distance: None,
                },
                geometry: serde_json::from_value::<Geometry>(json!({"type": "Point", "coordinates": [13.4, 52.5]})).expect("valid point"),
                geoproperty: LOCATION.parse().expect("valid location attribute name"),
            }))
            .scope_q(Some(ScopeQuery::parse("/A/#").expect("valid scope query")))
            .build()
    }

    fn candidates(states: &[(Identifier, Arc<EntityState>)], pushed: &EntitySelection) -> Vec<TemporalEntityCandidate> {
        let pushed = ResidualFilter::over(pushed.clone());
        states
            .iter()
            .map(|(entity_id, state)| (entity_id, ObservedSelection::Observed(Arc::clone(state))))
            .filter(|(_, observed)| observed.matches(&pushed))
            .map(|(entity_id, observed)| TemporalEntityCandidate {
                entity_id: entity_id.clone(),
                selection: Some(observed),
            })
            .collect()
    }

    #[test]
    fn q_geo_and_scope_residuals_match_full_pushdown_before_pagination() {
        let states = [
            ("urn:ngsi-ld:Entity:1", 30.0, 13.4, "/A/B"),
            ("urn:ngsi-ld:Entity:2", 20.0, 13.4, "/A/B"),
            ("urn:ngsi-ld:Entity:3", 40.0, 15.4, "/A/B"),
            ("urn:ngsi-ld:Entity:4", 50.0, 13.4, "/A/C"),
            ("urn:ngsi-ld:Entity:5", 60.0, 13.4, "/A/D"),
            ("urn:ngsi-ld:Entity:6", 70.0, 13.4, "/B"),
        ]
        .map(|(id, temperature, longitude, scope)| (id.parse::<Identifier>().expect("valid id"), entity(id, temperature, longitude, scope)));
        let full = combined_selection();
        let window = PageWindow::offset(1, 1);
        let full_matches = candidates(&states, &full);
        let full_pushdown = TemporalEntityIdOutcome {
            candidates: vec![TemporalEntityCandidate {
                entity_id: full_matches[1].entity_id.clone(),
                selection: None,
            }],
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Applied { total: Some(3) },
        }
        .complete(&window, CountPolicy::Emit)
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
                candidates: candidates(&states, &pushed),
                residual: TemporalResidual::over(residual),
                pagination: TemporalPaginationOutcome::Deferred,
            }
            .complete(&window, CountPolicy::Emit)
            .expect("valid residual outcome");
            pretty_assertions::assert_eq!(residual_outcome, full_pushdown);
        }

        let selected_observations = full_pushdown.0.iter().map(|identifier| format!("observation:{identifier}")).collect::<Vec<_>>();
        pretty_assertions::assert_eq!(selected_observations, vec!["observation:urn:ngsi-ld:Entity:4"]);
    }

    /// Clause 5.5.9.1: a deferred outcome holds every candidate, so a backward
    /// cursor takes the candidates closest before its boundary and the total
    /// is the whole surviving set.
    #[test]
    fn a_deferred_backward_cursor_takes_the_rows_before_its_boundary() {
        let identifier = |index: u32| format!("urn:ngsi-ld:Entity:{index}").parse::<Identifier>().expect("valid id");
        let outcome = TemporalEntityIdOutcome {
            candidates: (0..10)
                .map(|index| TemporalEntityCandidate {
                    entity_id: identifier(index),
                    selection: None,
                })
                .collect(),
            residual: TemporalResidual::default(),
            pagination: TemporalPaginationOutcome::Deferred,
        };
        let window = PageWindow {
            limit: 3,
            start: PageStart::Cursor(Some(PageCursor::Before(identifier(6)))),
        };
        let (identifiers, total) = outcome.complete(&window, CountPolicy::Emit).expect("valid deferred outcome");
        pretty_assertions::assert_eq!(identifiers, vec![identifier(3), identifier(4), identifier(5)]);
        pretty_assertions::assert_eq!(total, Some(10));
    }
}
