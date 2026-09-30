//! The `format` and `options` parameters, and when their grammar is applied.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.20 answers "parameters that are
//! incompatible with the operation, or ... values of the options parameter that
//! are not supported by the operation" with `InvalidRequest`, while clauses
//! 5.7.2.4 and 5.10.2.4 answer invalid *data* inside a parameter with
//! `BadRequestData`. Tables 6.3.7-1 and 6.3.12-1 make `format` and `options`
//! two spellings of one choice, so an unsupported value of either is the same
//! clause 6.3.20 failure.
//!
//! `serde_urlencoded` cannot carry that distinction: every failure it reports
//! is one opaque message, and a deserialiser that applied the grammar would
//! leave the binding layer to guess the error type from the message text. The
//! two parameters are therefore deserialised as the request wrote them and have
//! their grammar applied by the parameter set's `validate`, which fails with a
//! typed [`ParameterError`] the classification reads directly.

use crate::{error::ParameterError, unsupported_representation::UnsupportedRepresentation};
use derive_more::Display;
use serde::{Deserialize, Deserializer};

/// One of the two parameters that select a representation.
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq)]
pub enum RepresentationParameter {
    /// `format`, which names one representation (Tables 6.3.7-1, 6.3.12-1).
    #[display("format")]
    Format,
    /// `options`, the deprecated list of representation tokens.
    #[display("options")]
    Options,
}

/// A value outside the grammar its operation defines for the parameter.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct UnsupportedValue {
    /// The parameter the value was written for.
    parameter: RepresentationParameter,
    /// Why the operation does not support it.
    reason: UnsupportedRepresentation,
}

impl UnsupportedValue {
    /// Record a value the operation's grammar does not admit.
    #[must_use]
    pub const fn new(parameter: RepresentationParameter, reason: UnsupportedRepresentation) -> UnsupportedValue {
        UnsupportedValue { parameter, reason }
    }
}

// Clause 6.3.20 gives both parameters the same answer, and `ParameterError`
// keeps one variant per parameter so the message names the one at fault.
impl From<UnsupportedValue> for ParameterError {
    fn from(value: UnsupportedValue) -> ParameterError {
        match value.parameter {
            RepresentationParameter::Format => ParameterError::FormatInvalid(value.reason),
            RepresentationParameter::Options => ParameterError::OptionsInvalid(value.reason),
        }
    }
}

/// How a representation parameter's written value parses for one operation.
///
/// Implementors are the values the two parameters resolve to: a rendering for
/// `format`, and an operation's own set of `options` tokens.
pub trait RepresentationGrammar: Sized {
    /// The parameter whose grammar this is.
    const PARAMETER: RepresentationParameter;

    /// Parse a value as this operation's grammar defines it.
    ///
    /// # Errors
    ///
    /// Returns why the operation does not support the value, which becomes
    /// the cause of the clause 6.3.20 rejection.
    fn parse(written: &str) -> Result<Self, UnsupportedRepresentation>;
}

/// A representation parameter as the request wrote it.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum WrittenValue<T> {
    /// A value the operation supports.
    Supported(T),
    /// A value it does not, kept so `validate` can refuse it.
    ///
    /// Boxed because the rejection is the rare case and every parameter set
    /// carries this type inline: an unboxed rejection would widen every one of
    /// them, and with them the futures the request handlers are.
    Unsupported(Box<UnsupportedValue>),
}

impl<T> WrittenValue<T> {
    /// The value the operation supports.
    ///
    /// # Errors
    ///
    /// Returns the clause 6.3.20 rejection of what the request wrote.
    pub fn supported(&self) -> Result<&T, ParameterError> {
        match self {
            WrittenValue::Supported(value) => Ok(value),
            WrittenValue::Unsupported(unsupported) => Err(ParameterError::from(**unsupported)),
        }
    }

    /// The value when the operation supports it, and nothing when it does not.
    ///
    /// Every parameter set is validated before anything acts on it, so an
    /// unsupported value never reaches a caller of this. It exists for the
    /// readings that have no request left to fail -- building a forwarded query
    /// or a compilation cache key -- where "the request named nothing the
    /// operation can act on" is the only answer.
    #[must_use]
    pub const fn if_supported(&self) -> Option<&T> {
        match self {
            WrittenValue::Supported(value) => Some(value),
            WrittenValue::Unsupported(_) => None,
        }
    }
}

impl<T: Copy> WrittenValue<T> {
    /// The value the operation supports, copied out of the parameter set.
    ///
    /// # Errors
    ///
    /// Returns the clause 6.3.20 rejection of what the request wrote.
    pub fn supported_value(&self) -> Result<T, ParameterError> {
        self.supported().copied()
    }
}

// Written, not parsed: the grammar decides nothing here beyond which of the two
// shapes the value takes, because failing now would erase the clause 6.3.20
// distinction (see the module documentation).
impl<'de, T: RepresentationGrammar> Deserialize<'de> for WrittenValue<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<WrittenValue<T>, D::Error> {
        let written = String::deserialize(deserializer)?;
        Ok(match T::parse(&written) {
            Ok(value) => WrittenValue::Supported(value),
            Err(reason) => WrittenValue::Unsupported(Box::new(UnsupportedValue::new(T::PARAMETER, reason))),
        })
    }
}

/// The representation an `options` list selects, when its operation has one.
///
/// Tables 6.3.7-1 and 6.3.12-1 both describe `options` as "an alternative
/// mechanism to include the format parameter", and Table 6.3.12-1 settles the
/// conflict: "If both format and options are present, the value of the format
/// parameter shall take precedence."
pub trait SelectedRepresentation {
    /// The renderings this operation's `format` parameter names.
    type Representation: Copy + Default;

    /// The rendering the list named, if it named one.
    fn representation(&self) -> Option<Self::Representation>;
}

/// The representation a request selected through `format` or `options`.
///
/// # Errors
///
/// Returns the clause 6.3.20 rejection when either parameter carried a value
/// the operation does not support -- both are read, so a supported `format`
/// does not excuse an `options` value the operation cannot act on.
pub fn resolve_representation<F, O>(format: Option<&WrittenValue<F>>, options: Option<&WrittenValue<O>>) -> Result<F, ParameterError>
where
    F: Copy + Default,
    O: SelectedRepresentation<Representation = F>,
{
    let selected = options
        .map(WrittenValue::supported)
        .transpose()?
        .and_then(SelectedRepresentation::representation);
    let named = supported_option(format)?;
    Ok(named.or(selected).unwrap_or_default())
}

/// The representation a validated request selected.
///
/// The infallible reading of [`resolve_representation`], for the callers that
/// have no request left to fail: a parameter set is validated before anything
/// compiles a representation from it, so an unsupported value cannot reach
/// here and names no rendering when it somehow does.
pub fn selected_representation<F, O>(format: Option<&WrittenValue<F>>, options: Option<&WrittenValue<O>>) -> F
where
    F: Copy + Default,
    O: SelectedRepresentation<Representation = F>,
{
    format
        .and_then(WrittenValue::if_supported)
        .copied()
        .or_else(|| options.and_then(WrittenValue::if_supported).and_then(SelectedRepresentation::representation))
        .unwrap_or_default()
}

/// The value an optional representation parameter names, if it named one.
///
/// # Errors
///
/// Returns the clause 6.3.20 rejection when the parameter was present with a
/// value the operation does not support.
pub fn supported_option<T: Copy>(written: Option<&WrittenValue<T>>) -> Result<Option<T>, ParameterError> {
    written.map(WrittenValue::supported_value).transpose()
}

#[cfg(test)]
mod tests {
    use crate::{
        error::ParameterError,
        representation_parameter::{RepresentationGrammar, RepresentationParameter, WrittenValue, supported_option},
        unsupported_representation::UnsupportedRepresentation,
    };
    use serde::Deserialize;

    /// A one-token grammar standing in for the operation-specific ones.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    struct OnlyToken;

    impl RepresentationGrammar for OnlyToken {
        const PARAMETER: RepresentationParameter = RepresentationParameter::Options;

        fn parse(written: &str) -> Result<OnlyToken, UnsupportedRepresentation> {
            if written == "token" {
                Ok(OnlyToken)
            } else {
                Err(UnsupportedRepresentation::UnknownOptionsToken)
            }
        }
    }

    #[derive(Deserialize)]
    struct Wire {
        options: Option<WrittenValue<OnlyToken>>,
    }

    /// A supported value deserialises into the value itself.
    #[test]
    fn a_supported_value_carries_the_value() {
        let wire: Wire = serde_urlencoded::from_str("options=token").expect("deserialises");
        pretty_assertions::assert_eq!(supported_option(wire.options.as_ref()).expect("supported"), Some(OnlyToken));
    }

    /// Clause 6.3.20: an unsupported value survives deserialisation and is
    /// refused as an `options` failure, which classifies as `InvalidRequest`.
    #[test]
    fn an_unsupported_value_survives_deserialisation_and_is_refused_by_name() {
        let wire: Wire = serde_urlencoded::from_str("options=nonsense").expect("deserialises");
        let refused = supported_option(wire.options.as_ref()).expect_err("refused");
        assert!(
            matches!(refused, ParameterError::OptionsInvalid(UnsupportedRepresentation::UnknownOptionsToken)),
            "{refused}"
        );
    }

    /// An absent parameter names nothing and is no failure.
    #[test]
    fn an_absent_parameter_names_nothing() {
        let wire: Wire = serde_urlencoded::from_str("").expect("deserialises");
        pretty_assertions::assert_eq!(supported_option(wire.options.as_ref()).expect("absent"), None);
    }

    /// The `format` parameter reports its own failures under its own name.
    #[test]
    fn a_format_failure_is_reported_as_a_format_failure() {
        #[derive(Debug)]
        struct OnlyFormat;

        impl RepresentationGrammar for OnlyFormat {
            const PARAMETER: RepresentationParameter = RepresentationParameter::Format;

            fn parse(_written: &str) -> Result<OnlyFormat, UnsupportedRepresentation> {
                Err(UnsupportedRepresentation::UnknownFormat)
            }
        }

        #[derive(Deserialize)]
        struct FormatWire {
            format: Option<WrittenValue<OnlyFormat>>,
        }

        let wire: FormatWire = serde_urlencoded::from_str("format=nonsense").expect("deserialises");
        let refused = wire.format.as_ref().expect("present").supported().expect_err("refused");
        assert!(matches!(refused, ParameterError::FormatInvalid(_)), "{refused}");
    }
}
