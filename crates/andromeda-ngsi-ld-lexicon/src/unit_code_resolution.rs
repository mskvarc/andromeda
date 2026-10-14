//! What a written `unitCode` admits, and why a string is refused.
//!
//! Clause 4.5.2.2 makes `unitCode` "a string representing the measurement
//! unit" that "shall be encoded using the UNECE/CEFACT Common Codes for Units
//! of Measurement", and Table 5.2.5-1 repeats the requirement. Reference [15]
//! of ETSI GS CIM 009 v1.9.1 pins Recommendation 20 Revision 9, so a code
//! that a later revision deprecates or deletes is still a conforming value.
//! Every code the `cefact-units` library lists is therefore admitted, whatever
//! its status: the status is informative and plays no part in admission.

use cefact_units::UnitCode;
use thiserror::Error;

/// Why a written `unitCode` member is not usable.
///
/// Every variant describes what the client wrote, because the refusal reaches
/// the client as the detail of a `BadRequestData` problem.
#[derive(Clone, Copy, Debug, Eq, Error, Hash, PartialEq)]
pub enum UnitCodeRefusal {
    /// The expanded member was not a single `@value` string entry.
    #[error("'unitCode' shall be a single string holding a UN/CEFACT common code, not a reified attribute or a repeated member")]
    Malformed,
    /// The member names no UN/CEFACT common code at all.
    #[error("'unitCode' is not a UN/CEFACT common code")]
    NotACommonCode,
}

/// Resolve one written UN/CEFACT common code, matched case-sensitively.
///
/// # Errors
///
/// Returns [`UnitCodeRefusal::NotACommonCode`] when `code` names no code the
/// library lists.
pub fn resolve_unit_code(code: &str) -> Result<UnitCode, UnitCodeRefusal> {
    UnitCode::from_code(code).ok_or(UnitCodeRefusal::NotACommonCode)
}

#[cfg(test)]
mod tests {
    use crate::unit_code_resolution::{UnitCodeRefusal, resolve_unit_code};
    use cefact_units::{Status, UnitCode};

    #[test]
    fn a_current_code_resolves() {
        pretty_assertions::assert_eq!(resolve_unit_code("KMH"), Ok(UnitCode::Kmh));
    }

    #[test]
    fn a_deprecated_code_resolves() {
        pretty_assertions::assert_eq!(resolve_unit_code("A1").map(UnitCode::status), Ok(Status::Deprecated));
    }

    /// Reference [15] pins Revision 9, which still lists `KTM`.
    #[test]
    fn a_deleted_code_resolves() {
        pretty_assertions::assert_eq!(resolve_unit_code("KTM").map(UnitCode::status), Ok(Status::Deleted));
    }

    /// Recommendation 20 common codes are upper case, so `kgm` names none.
    #[test]
    fn a_string_naming_no_common_code_is_refused() {
        for raw in ["Celsius", "kgm", "km/h", "", "NOTACODE"] {
            pretty_assertions::assert_eq!(resolve_unit_code(raw), Err(UnitCodeRefusal::NotACommonCode), "{raw}");
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
