//! The `unitCode` of a quantity-bearing attribute instance (clause 4.5.2.2,
//! Table 5.2.5-1).
//!
//! The member is a UN/CEFACT common code the unit catalog registers. Holding
//! both the catalog identity and the token means the store binds the identity
//! and the codec writes the token, neither consulting the catalog again.

use andromeda_unit_catalog::{
    expanded_unit_code::{ExpandedUnitCodeError, UnitCodeRefusal, unit_code_identity},
    unit_code_id::UnitCodeId,
    unit_code_registry::{UnitCodeSnapshotMalformed, UnitCodeUnknown, registry},
    unit_code_token::UnitCodeToken,
};
use derive_more::Display;
use thiserror::Error;

/// A stored catalog identity that names no unit code.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum StoredUnitCodeError {
    /// The catalog holds no code under the identity, or the code is withdrawn.
    #[error(transparent)]
    Unknown(#[from] UnitCodeUnknown),
    /// The compiled-in catalog snapshot did not parse; the build's fault.
    #[error(transparent)]
    Catalog(#[from] UnitCodeSnapshotMalformed),
}

/// A registered UN/CEFACT common code.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
#[display("{token}")]
pub struct UnitCode {
    /// The persisted identity the catalog assigned the code.
    id: UnitCodeId,
    /// The code as written on the wire.
    token: UnitCodeToken,
}

impl UnitCode {
    /// Resolve a written common code through the unit catalog.
    ///
    /// # Errors
    ///
    /// Returns [`ExpandedUnitCodeError::Refused`] when `code` names no common
    /// code the catalog registers, and [`ExpandedUnitCodeError::Catalog`] when
    /// the compiled-in catalog does not parse.
    pub fn resolve(code: &str) -> Result<UnitCode, ExpandedUnitCodeError> {
        let id = unit_code_identity(code)?;
        // An identity the registry just assigned always names an entry; a
        // catalog that cannot name it registers no such code.
        let entry = registry()?
            .entry(id)
            .map_err(|_| ExpandedUnitCodeError::Refused(UnitCodeRefusal::NotACommonCode))?;
        Ok(UnitCode { id, token: entry.token() })
    }

    /// The code the catalog registers under `id`, the identity a store keeps
    /// in place of the token.
    ///
    /// # Errors
    ///
    /// Returns [`StoredUnitCodeError`] when the catalog holds no code under
    /// `id`, or when the compiled-in snapshot does not parse.
    pub fn from_id(id: UnitCodeId) -> Result<UnitCode, StoredUnitCodeError> {
        Ok(UnitCode {
            id,
            token: registry()?.entry(id)?.token(),
        })
    }

    /// The persisted catalog identity.
    #[must_use]
    pub const fn id(self) -> UnitCodeId {
        self.id
    }

    /// The code as written on the wire.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.token.as_str()
    }
}

#[cfg(test)]
mod tests {
    use crate::unit_code::UnitCode;
    use andromeda_unit_catalog::expanded_unit_code::{ExpandedUnitCodeError, UnitCodeRefusal};

    #[test]
    fn a_registered_code_resolves_to_its_token_and_identity() {
        let unit = UnitCode::resolve("KMH").expect("a registered code");
        pretty_assertions::assert_eq!(unit.as_str(), "KMH");
        pretty_assertions::assert_eq!(unit.to_string(), "KMH");
    }

    #[test]
    fn a_code_that_is_no_common_code_is_refused() {
        pretty_assertions::assert_eq!(UnitCode::resolve("km/h"), Err(ExpandedUnitCodeError::Refused(UnitCodeRefusal::NotACommonCode)));
    }
}
