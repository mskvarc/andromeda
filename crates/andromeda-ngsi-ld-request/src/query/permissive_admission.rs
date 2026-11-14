//! The admission the request tests validate queries under.

use andromeda_ngsi_ld_query::filter::{compilation::FilterAdmission, complexity::FilterComplexityLimits, language_targeting::LanguageTargeting};

/// A filter admission bounding no complexity, at the `q_target_languages`
/// level the specification states (clause 4.9).
pub(crate) const PERMISSIVE: FilterAdmission = FilterAdmission {
    complexity: FilterComplexityLimits::PARSED,
    language_targeting: LanguageTargeting::Specification,
};
