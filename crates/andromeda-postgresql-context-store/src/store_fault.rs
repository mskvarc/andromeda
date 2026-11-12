//! How the context store's own failures reach the ports it implements.
//!
//! A port cannot name this adapter's error types, so every failure crossing a
//! port boundary is classified into the backend-neutral
//! [`PersistenceFailure`] and recorded for operators at the point it is
//! classified. The classification is one trait so every port helper classifies
//! the same failure the same way, instead of each recovering a concrete type by
//! downcast.

use crate::{error::ContextStoreError, jsonld_context::mapper::ContextMapperError};
use andromeda_json_literal::admission::JsonLiteralError;
use andromeda_ngsi_ld_lexicon::{identifier::IdentifierParseError, ngsi_ld_urn::JsonLdContextIdParseError};
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_codes::registry::UnknownCompactCode;
use andromeda_postgresql_encoding::ewkb::EwkbError;
use andromeda_postgresql_name_dictionary::name_lookup::NameLookupError;
use andromeda_postgresql_pool::persistence_classification::classify;
use andromeda_postgresql_query::error::RenderError;
use andromeda_postgresql_seaquery::committing_statement::BoundCommittingStatement;
use sea_query::error::Error as SeaQueryError;
use std::{fmt::Display, num::TryFromIntError};

/// An adapter failure the context store can classify for a port.
pub(crate) trait StoreFault: Display {
    /// The backend-neutral class of this failure.
    fn failure_class(&self) -> PersistenceFailure;
}

impl StoreFault for sqlx::Error {
    fn failure_class(&self) -> PersistenceFailure {
        classify(self)
    }
}

impl StoreFault for ContextStoreError {
    fn failure_class(&self) -> PersistenceFailure {
        ContextStoreError::failure_class(self)
    }
}

impl StoreFault for NameLookupError {
    fn failure_class(&self) -> PersistenceFailure {
        NameLookupError::failure_class(self)
    }
}

impl StoreFault for serde_json::Error {
    fn failure_class(&self) -> PersistenceFailure {
        PersistenceFailure::Undecodable
    }
}

impl<T: StoreFault> StoreFault for &T {
    fn failure_class(&self) -> PersistenceFailure {
        T::failure_class(self)
    }
}

/// Implement [`StoreFault`] for adapter failures that always fall in one class.
macro_rules! store_fault_class {
    ($($error:ty => $class:ident),+ $(,)?) => {
        $(
            impl StoreFault for $error {
                fn failure_class(&self) -> PersistenceFailure {
                    PersistenceFailure::$class
                }
            }
        )+
    };
}

store_fault_class! {
    // A stored row whose value no longer reads as the domain value it was written from.
    ContextMapperError => Undecodable,
    IdentifierParseError => Undecodable,
    TryFromIntError => Undecodable,
    UnknownCompactCode => Undecodable,
    // A domain value the store's representation cannot hold.
    EwkbError => Unencodable,
    JsonLiteralError => Unencodable,
    // A statement the adapter could not assemble.
    BoundCommittingStatement => StatementUnbuildable,
    RenderError => StatementUnbuildable,
    SeaQueryError => StatementUnbuildable,
    // An identifier the adapter minted that its own typed identifier refuses.
    JsonLdContextIdParseError => ContractBroken,
}

/// Classify an adapter failure and record it for operators at the caller's
/// location.
#[track_caller]
pub(crate) fn persistence_failure(error: &impl StoreFault) -> PersistenceFailure {
    error.failure_class().record(error)
}

/// Classify an adapter failure at a port boundary into the port's own error
/// type `E`: backpressure keeps the port's retryable category and every fault
/// becomes its persistence failure.
///
/// The failure is recorded at the caller's location. Name the port's error
/// type with a turbofish where the surrounding `?` leaves it open.
#[track_caller]
pub(crate) fn persistence_error<E: From<PersistenceFailure>>(error: impl StoreFault) -> E {
    E::from(persistence_failure(&error))
}

/// Record a stored value that does not read back as the domain value it was
/// written from.
#[track_caller]
pub(crate) fn undecodable(error: &impl Display) -> PersistenceFailure {
    PersistenceFailure::Undecodable.record(error)
}

/// Record a row the store should have returned and did not, or returned in a
/// shape its contract with this adapter rules out.
#[track_caller]
pub(crate) fn contract_broken() -> PersistenceFailure {
    PersistenceFailure::ContractBroken.record_bare()
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ContextStoreError,
        store_fault::{StoreFault, persistence_failure},
    };
    use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_pool_timeout_is_saturation_however_it_arrives() {
        assert_eq!(persistence_failure(&sqlx::Error::PoolTimedOut), PersistenceFailure::Saturated);
        assert_eq!(
            ContextStoreError::from(sqlx::Error::PoolTimedOut).failure_class(),
            PersistenceFailure::Saturated
        );
    }

    #[test]
    fn a_missing_row_is_a_missing_record() {
        assert_eq!(sqlx::Error::RowNotFound.failure_class(), PersistenceFailure::RecordMissing);
    }

    #[test]
    fn an_undecodable_json_document_is_undecodable() {
        let error = serde_json::from_str::<serde_json::Value>("{").expect_err("an unterminated object");
        assert_eq!(error.failure_class(), PersistenceFailure::Undecodable);
    }
}
