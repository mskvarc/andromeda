//! Resolving the written `unitCode` of an attribute instance to the catalog
//! identity of its code, and why one cannot be.
//!
//! Table 5.2.5-1 restricts `unitCode` to a UN/CEFACT common code, and clause
//! 4.5.2.2 makes it "a string representing the measurement unit", so the only
//! usable member is one plain string naming a code this catalog registers. The
//! rule lives here, beside the catalog it consults, so that the write admission
//! refusing an entity and the store persisting its unit hold the member to the
//! same rule.

use crate::{
    unit_code_id::UnitCodeId,
    unit_code_registry::{UnitCodeSnapshotMalformed, registry},
};
use cefact_units::UnitCode;
use thiserror::Error;

/// Why an expanded `unitCode` member is not usable.
///
/// Every variant describes what the client wrote, because the refusal reaches
/// the client as the detail of a `BadRequestData` problem.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum UnitCodeRefusal {
    /// The expanded member was not a single `@value` string entry.
    #[error("'unitCode' shall be a single string holding a UN/CEFACT common code, not a reified attribute or a repeated member")]
    Malformed,
    /// The member names no UN/CEFACT common code at all.
    #[error("'unitCode' is not a UN/CEFACT common code")]
    NotACommonCode,
    /// The code is a UN/CEFACT common code the catalog has no identity for.
    #[error("the UN/CEFACT common code {code} is not one the unit catalog registers")]
    Unregistered {
        /// The common code the client wrote.
        code: UnitCode,
    },
}

/// Why an expanded `unitCode` member could not be resolved to an identity.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum ExpandedUnitCodeError {
    /// The member is not a usable unit code; the client's fault.
    #[error(transparent)]
    Refused(#[from] UnitCodeRefusal),
    /// The compiled-in catalog snapshot did not parse; the build's fault.
    #[error(transparent)]
    Catalog(#[from] UnitCodeSnapshotMalformed),
}

/// Resolve one written UN/CEFACT common code to the identity the catalog
/// assigned it.
///
/// # Errors
///
/// Returns [`ExpandedUnitCodeError::Refused`] when `code` names no common code
/// or names one the catalog does not register, and
/// [`ExpandedUnitCodeError::Catalog`] when the compiled-in snapshot does not
/// parse.
pub fn unit_code_identity(code: &str) -> Result<UnitCodeId, ExpandedUnitCodeError> {
    let code = UnitCode::from_code(code).ok_or(UnitCodeRefusal::NotACommonCode)?;
    registry()?.id_of(code).map_err(|_| UnitCodeRefusal::Unregistered { code }.into())
}

#[cfg(test)]
mod tests {
    use crate::{
        expanded_unit_code::{ExpandedUnitCodeError, UnitCodeRefusal, unit_code_identity},
        unit_code_registry::registry,
    };
    use cefact_units::UnitCode;

    #[test]
    fn a_registered_code_resolves_to_the_identity_the_registry_gives_it() {
        let code = UnitCode::from_code("KMH").expect("a common code");
        let expected = registry().expect("the snapshot parses").id_of(code).expect("a registered code");
        pretty_assertions::assert_eq!(unit_code_identity("KMH"), Ok(expected));
    }

    /// Recommendation 20 common codes are upper case, so `kgm` names none.
    #[test]
    fn a_string_that_names_no_common_code_is_refused() {
        for raw in ["Celsius", "kgm", "km/h", ""] {
            pretty_assertions::assert_eq!(
                unit_code_identity(raw),
                Err(ExpandedUnitCodeError::Refused(UnitCodeRefusal::NotACommonCode)),
                "{raw}"
            );
        }
    }

    #[test]
    fn a_refusal_describes_what_the_client_wrote() {
        pretty_assertions::assert_eq!(
            UnitCodeRefusal::Malformed.to_string(),
            "'unitCode' shall be a single string holding a UN/CEFACT common code, not a reified attribute or a repeated member"
        );
        pretty_assertions::assert_eq!(UnitCodeRefusal::NotACommonCode.to_string(), "'unitCode' is not a UN/CEFACT common code");
    }
}
