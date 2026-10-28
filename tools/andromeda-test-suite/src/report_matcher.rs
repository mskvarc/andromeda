//! Matchers on the entries of a partial success report (clause 6.3.3): an entity in error of a
//! `BatchOperationResult` (clause 5.2.17) and an attribute left unchanged of an `UpdateResult` (clause 5.2.19).
//!
//! A report is matched as its lists, so googletest's list matchers (`len`, `contains`) and [`nth`](crate::nth::nth)
//! apply with one of these matchers per entry.

use googletest::{
    description::Description,
    matcher::{Matcher, MatcherBase, MatcherResult},
};
use http::StatusCode;
use ngsi_ld::{
    http::{BatchEntityError, NotUpdatedDetails},
    model::{EntityId, ErrorType, Uri},
    registration::RegistrationId,
};

/// Whether the registration a report entry names is `expected`.
fn names_registration(actual: Option<&Uri>, expected: Option<&RegistrationId>) -> bool {
    expected.is_none_or(|expected| actual == Some(expected.as_uri()))
}

/// The requirements on one entity in error.
#[derive(Debug, Clone, Default, MatcherBase)]
pub struct BatchErrorMatcher {
    /// The entity, when required.
    entity: Option<EntityId>,
    /// The status of its problem, when required.
    status: Option<StatusCode>,
    /// The problem type, when required.
    error_type: Option<ErrorType>,
    /// The registration of the failed distributed operation, when required.
    registration: Option<RegistrationId>,
    /// Texts the problem's `detail` must contain.
    detail_contains: Vec<String>,
    /// The `detail` the problem must carry, when the test names it whole.
    detail: Option<String>,
}

/// An entity in error, whatever its entity and problem.
#[must_use]
pub fn batch_error() -> BatchErrorMatcher {
    BatchErrorMatcher::default()
}

impl BatchErrorMatcher {
    /// Requires the entity in error to be `id`.
    #[must_use]
    pub fn entity(self, id: &EntityId) -> BatchErrorMatcher {
        BatchErrorMatcher {
            entity: Some(id.clone()),
            ..self
        }
    }

    /// Requires the problem to carry the status `status`.
    #[must_use]
    pub fn status(self, status: StatusCode) -> BatchErrorMatcher {
        BatchErrorMatcher { status: Some(status), ..self }
    }

    /// Requires the problem to be of `error_type`.
    #[must_use]
    pub fn problem(self, error_type: ErrorType) -> BatchErrorMatcher {
        BatchErrorMatcher {
            error_type: Some(error_type),
            ..self
        }
    }

    /// Requires the failed distributed operation to be the one of the registration `id`.
    #[must_use]
    pub fn registration(self, id: &RegistrationId) -> BatchErrorMatcher {
        BatchErrorMatcher {
            registration: Some(id.clone()),
            ..self
        }
    }

    /// Requires the problem's `detail` to contain `text`.
    #[must_use]
    pub fn with_detail_containing(mut self, text: &str) -> BatchErrorMatcher {
        self.detail_contains.push(text.to_owned());
        self
    }

    /// Also requires the problem's `detail` to be `text`.
    #[must_use]
    pub fn with_detail(self, text: &str) -> BatchErrorMatcher {
        BatchErrorMatcher {
            detail: Some(text.to_owned()),
            ..self
        }
    }

    /// Whether `actual` meets every requirement.
    fn holds(&self, actual: &BatchEntityError) -> bool {
        let problem = &actual.error;
        self.entity.as_ref().is_none_or(|id| &actual.entity_id == id)
            && self.status.is_none_or(|status| problem.status == Some(status.as_u16()))
            && self
                .error_type
                .as_ref()
                .is_none_or(|error_type| problem.error_type.as_ref() == Some(error_type))
            && names_registration(actual.registration_id.as_ref(), self.registration.as_ref())
            && self
                .detail_contains
                .iter()
                .all(|text| problem.detail.as_deref().is_some_and(|detail| detail.contains(text.as_str())))
            && self.detail.as_deref().is_none_or(|expected| problem.detail.as_deref() == Some(expected))
    }
}

impl Matcher<&BatchEntityError> for BatchErrorMatcher {
    fn matches(&self, actual: &BatchEntityError) -> MatcherResult {
        self.holds(actual).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is an entity in error meeting {self:?}").into(),
            MatcherResult::NoMatch => format!("isn't an entity in error meeting {self:?}").into(),
        }
    }
}

/// The requirements on one attribute left unchanged.
#[derive(Debug, Clone, Default, MatcherBase)]
pub struct NotUpdatedMatcher {
    /// The attribute's short name, when required.
    attribute: Option<String>,
    /// The registration of the failed distributed operation, when required.
    registration: Option<RegistrationId>,
}

/// An attribute left unchanged, whatever its name and reason.
#[must_use]
pub fn not_updated() -> NotUpdatedMatcher {
    NotUpdatedMatcher::default()
}

impl NotUpdatedMatcher {
    /// Requires the attribute to be `name`, as a term or as an IRI ending in it: a 207 answer carries no
    /// `@context` (clause 6.3.6), so a broker may write the name expanded.
    #[must_use]
    pub fn attribute(self, name: &str) -> NotUpdatedMatcher {
        NotUpdatedMatcher {
            attribute: Some(name.to_owned()),
            ..self
        }
    }

    /// Requires the failed distributed operation to be the one of the registration `id`.
    #[must_use]
    pub fn registration(self, id: &RegistrationId) -> NotUpdatedMatcher {
        NotUpdatedMatcher {
            registration: Some(id.clone()),
            ..self
        }
    }

    /// Whether `actual` meets every requirement.
    fn holds(&self, actual: &NotUpdatedDetails) -> bool {
        let written = actual.attribute_name.to_string();
        self.attribute
            .as_ref()
            .is_none_or(|name| written == *name || written.ends_with(&format!("/{name}")))
            && names_registration(actual.registration_id.as_ref(), self.registration.as_ref())
    }
}

impl Matcher<&NotUpdatedDetails> for NotUpdatedMatcher {
    fn matches(&self, actual: &NotUpdatedDetails) -> MatcherResult {
        self.holds(actual).into()
    }

    fn describe(&self, result: MatcherResult) -> Description {
        match result {
            MatcherResult::Match => format!("is an attribute left unchanged meeting {self:?}").into(),
            MatcherResult::NoMatch => format!("isn't an attribute left unchanged meeting {self:?}").into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        nth::nth,
        report_matcher::{batch_error, not_updated},
    };
    use googletest::prelude::*;
    use http::StatusCode;
    use ngsi_ld::{
        http::{BatchOperationResult, UpdateResult},
        model::ErrorType,
    };

    #[gtest]
    fn an_entity_in_error_is_matched_by_its_entity_problem_and_registration() -> Result<()> {
        let report: BatchOperationResult = serde_json::from_str(
            r#"{"success":[],"errors":[{"entityId":"urn:ngsi-ld:Vehicle:1","registrationId":"urn:ngsi-ld:ContextSourceRegistration:1",
            "error":{"type":"https://uri.etsi.org/ngsi-ld/errors/BadRequestData","title":"t","detail":"refused at speed","status":400}}]}"#,
        )?;
        let vehicle = "urn:ngsi-ld:Vehicle:1".parse()?;
        let registration = "urn:ngsi-ld:ContextSourceRegistration:1".parse()?;
        expect_that!(
            &report.errors,
            nth(
                0,
                batch_error()
                    .entity(&vehicle)
                    .status(StatusCode::BAD_REQUEST)
                    .problem(ErrorType::BadRequestData)
                    .registration(&registration)
                    .with_detail_containing("speed")
                    .with_detail("refused at speed")
            )
        );
        expect_that!(&report.errors, not(contains(batch_error().with_detail("refused"))));
        verify_that!(&report.errors, not(contains(batch_error().status(StatusCode::CONFLICT))))
    }

    #[gtest]
    fn an_attribute_left_unchanged_is_matched_by_its_term_or_its_iri() -> Result<()> {
        let report: UpdateResult = serde_json::from_str(r#"{"updated":[],"notUpdated":[{"attributeName":"https://example.org/speed","reason":"r"}]}"#)?;
        expect_that!(&report.not_updated, contains(not_updated().attribute("speed")));
        verify_that!(&report.not_updated, not(contains(not_updated().attribute("peed"))))
    }
}
