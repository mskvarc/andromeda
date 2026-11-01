//! The recognised datatypes, and the check a literal declared as one is held to.
//!
//! The NGSI-LD `DateTime`, `Date` and `Time` are read by the clause 4.6.3
//! readers of [`crate::time`]. An XSD datatype is read by its XSD 1.1 lexical
//! space. `xsd:dateTime`, `xsd:date` and `xsd:time` are XSD datatypes with
//! their own, wider lexical spaces (a zone offset, any fractional width): the
//! NGSI-LD core context names `ngsi-ld:DateTime`, `ngsi-ld:Date` and
//! `ngsi-ld:Time` and maps none of them to XSD, so a literal declared as one
//! of those XSD datatypes is held to XSD and is no NGSI-LD time value.

use crate::{
    datatype::primitive_literal::PrimitiveLiteral,
    reference::NgsiLdReference,
    time::value::{TimeValueError, TimeValueForm},
};
use andromeda_ngsi_ld_number::JsonNumberError;
use iri_rs::Iri;
use rdfx::{RDF_JSON, RDF_LANG_STRING};
use thiserror::Error;
use xsd_rs::{Datatype, NCNameDatatype, NameDatatype, NormalizedStringDatatype, StringDatatype, TokenDatatype};

/// A datatype whose values the broker can tell from other literals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognisedDatatype {
    /// An NGSI-LD time value data type (clause 4.6.3).
    Time(TimeValueForm),
    /// An XSD datatype RDF 1.1 Concepts section 5.1 lists as compatible.
    Xsd(Datatype),
    /// `rdf:langString`, the datatype of a language-tagged string.
    LangString,
    /// `rdf:JSON`, the datatype of a JSON literal.
    Json,
}

impl RecognisedDatatype {
    /// The recognised datatype `datatype` names, `None` for any other datatype
    /// IRI, which is valid and carries nothing to check (RDF 1.1 Concepts
    /// section 5.3).
    ///
    /// # Errors
    ///
    /// Returns [`DatatypeError::NotAnIri`] when `datatype` is not an absolute
    /// IRI, which every datatype is (clause 4.5.2: `valueType` "shall be type
    /// coerced into a datatype URI").
    pub fn of(datatype: &NgsiLdReference) -> Result<Option<RecognisedDatatype>, DatatypeError> {
        // clone: the refusal owns the text it reports.
        let iri = Iri::parse(datatype.as_str()).map_err(|_| DatatypeError::NotAnIri(datatype.clone()))?;
        if let Some(form) = TimeValueForm::named_by(iri.as_str()) {
            return Ok(Some(RecognisedDatatype::Time(form)));
        }
        if iri == RDF_LANG_STRING {
            return Ok(Some(RecognisedDatatype::LangString));
        }
        if iri == RDF_JSON {
            return Ok(Some(RecognisedDatatype::Json));
        }
        Ok(Datatype::from_iri(&iri).filter(|xsd| is_rdf_compatible(*xsd)).map(RecognisedDatatype::Xsd))
    }

    /// Whether `literal` is a value of this datatype.
    ///
    /// A string is read as the datatype's lexical form. A number is a value of
    /// a numeric datatype when its exact decimal is in the value space, so
    /// `1.5` is no `xsd:integer`. A boolean is a value of `xsd:boolean` only.
    ///
    /// # Errors
    ///
    /// Returns [`DatatypeError`] naming why the literal is not a value of this
    /// datatype.
    pub fn admits(self, literal: PrimitiveLiteral<'_>) -> Result<(), DatatypeError> {
        match self {
            RecognisedDatatype::Time(form) => match literal {
                PrimitiveLiteral::String(text) => form.parse(text).map(drop).map_err(DatatypeError::Time),
                PrimitiveLiteral::Number(_) | PrimitiveLiteral::Boolean(_) => Err(DatatypeError::Time(TimeValueError::NotAString)),
            },
            RecognisedDatatype::Xsd(xsd) => admits_xsd(xsd, literal),
            RecognisedDatatype::LangString => match literal {
                PrimitiveLiteral::String(_) => Ok(()),
                PrimitiveLiteral::Number(_) | PrimitiveLiteral::Boolean(_) => Err(DatatypeError::NotAValue(RDF_LANG_STRING)),
            },
            RecognisedDatatype::Json => Ok(()),
        }
    }
}

/// Hold the `valueType` of a `LanguageProperty`, which "shall be equal to
/// `langString`" (Table 5.2.32-1).
///
/// # Errors
///
/// Returns [`DatatypeError::NotLangString`] for any other datatype, and
/// [`DatatypeError::NotAnIri`] for a `valueType` that is not an IRI.
pub fn admit_language_map_value_type(value_type: &NgsiLdReference) -> Result<(), DatatypeError> {
    match RecognisedDatatype::of(value_type)? {
        Some(RecognisedDatatype::LangString) => Ok(()),
        Some(RecognisedDatatype::Time(_) | RecognisedDatatype::Xsd(_) | RecognisedDatatype::Json) | None => {
            // clone: the refusal owns the text it reports.
            Err(DatatypeError::NotLangString(value_type.clone()))
        }
    }
}

/// Whether RDF 1.1 Concepts section 5.1 lists `xsd` as compatible with RDF:
/// every XSD 1.1 built-in datatype except `xsd:QName`, `xsd:ID` and
/// `xsd:IDREF`, which "are not suitable for use in RDF".
const fn is_rdf_compatible(xsd: Datatype) -> bool {
    !matches!(
        xsd,
        Datatype::QName
            | Datatype::String(StringDatatype::NormalizedString(NormalizedStringDatatype::Token(TokenDatatype::Name(
                NameDatatype::NCName(NCNameDatatype::Id | NCNameDatatype::IdRef)
            ))))
    )
}

/// [`RecognisedDatatype::admits`] for an XSD datatype.
fn admits_xsd(xsd: Datatype, literal: PrimitiveLiteral<'_>) -> Result<(), DatatypeError> {
    let lexical = match literal {
        PrimitiveLiteral::String(text) => xsd.parse(text),
        PrimitiveLiteral::Number(number) => match xsd {
            Datatype::Decimal(_) | Datatype::Float | Datatype::Double => xsd.parse(&number.decimal().to_plain_string()),
            Datatype::Boolean
            | Datatype::String(_)
            | Datatype::Duration(_)
            | Datatype::DateTime(_)
            | Datatype::Time
            | Datatype::Date
            | Datatype::GYearMonth
            | Datatype::GYear
            | Datatype::GMonthDay
            | Datatype::GDay
            | Datatype::GMonth
            | Datatype::Base64Binary
            | Datatype::HexBinary
            | Datatype::AnyUri
            | Datatype::QName => return Err(DatatypeError::NotAValue(xsd.iri())),
        },
        PrimitiveLiteral::Boolean(_) => {
            return match xsd {
                Datatype::Boolean => Ok(()),
                Datatype::Float
                | Datatype::Double
                | Datatype::Decimal(_)
                | Datatype::String(_)
                | Datatype::Duration(_)
                | Datatype::DateTime(_)
                | Datatype::Time
                | Datatype::Date
                | Datatype::GYearMonth
                | Datatype::GYear
                | Datatype::GMonthDay
                | Datatype::GDay
                | Datatype::GMonth
                | Datatype::Base64Binary
                | Datatype::HexBinary
                | Datatype::AnyUri
                | Datatype::QName => Err(DatatypeError::NotAValue(xsd.iri())),
            };
        }
    };
    lexical.map(drop).map_err(|_| DatatypeError::NotAValue(xsd.iri()))
}

/// Ways a literal fails the datatype it is declared as.
#[derive(Debug, Error)]
pub enum DatatypeError {
    /// The declared datatype is not an absolute IRI.
    #[error("the datatype '{0}' is not an absolute IRI")]
    NotAnIri(NgsiLdReference),
    /// The literal is declared an NGSI-LD time value and is not one.
    #[error(transparent)]
    Time(#[from] TimeValueError),
    /// The literal is not a value of the recognised datatype.
    #[error("the value is not a {0}")]
    NotAValue(Iri<&'static str>),
    /// The literal names a number the broker cannot hold.
    #[error(transparent)]
    Number(#[from] JsonNumberError),
    /// A `LanguageProperty` declares a `valueType` other than `rdf:langString`.
    #[error("the valueType of a LanguageProperty is langString, not '{0}'")]
    NotLangString(NgsiLdReference),
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        datatype::{
            primitive_literal::PrimitiveLiteral,
            recognised::{DatatypeError, RecognisedDatatype, admit_language_map_value_type},
        },
        reference::NgsiLdReference,
        time::value::TimeValueForm,
    };
    use andromeda_ngsi_ld_number::JsonNumber;

    fn reference(iri: &str) -> NgsiLdReference {
        iri.parse().expect("a datatype IRI")
    }

    fn recognised(iri: &str) -> RecognisedDatatype {
        RecognisedDatatype::of(&reference(iri)).expect("an IRI").expect("a recognised datatype")
    }

    fn xsd(local: &str) -> RecognisedDatatype {
        recognised(&format!("http://www.w3.org/2001/XMLSchema#{local}"))
    }

    fn number(text: &str) -> JsonNumber {
        serde_json::from_str(text).expect("a number")
    }

    #[test]
    fn an_integer_admits_whole_numbers_and_their_lexical_form() {
        let integer = xsd("integer");
        assert!(integer.admits(PrimitiveLiteral::Number(&number("42"))).is_ok());
        assert!(integer.admits(PrimitiveLiteral::Number(&number("4.2e1"))).is_ok());
        assert!(integer.admits(PrimitiveLiteral::String("42")).is_ok());
        assert!(matches!(
            integer.admits(PrimitiveLiteral::Number(&number("1.5"))),
            Err(DatatypeError::NotAValue(_))
        ));
        assert!(matches!(integer.admits(PrimitiveLiteral::String("abc")), Err(DatatypeError::NotAValue(_))));
        assert!(matches!(integer.admits(PrimitiveLiteral::Boolean(true)), Err(DatatypeError::NotAValue(_))));
    }

    #[test]
    fn a_bounded_integer_holds_a_number_to_its_range() {
        let byte = xsd("byte");
        assert!(byte.admits(PrimitiveLiteral::Number(&number("127"))).is_ok());
        assert!(byte.admits(PrimitiveLiteral::Number(&number("128"))).is_err());
    }

    #[test]
    fn a_boolean_admits_json_booleans_and_its_lexical_form_only() {
        let boolean = xsd("boolean");
        assert!(boolean.admits(PrimitiveLiteral::Boolean(true)).is_ok());
        assert!(boolean.admits(PrimitiveLiteral::String("false")).is_ok());
        assert!(boolean.admits(PrimitiveLiteral::Number(&number("1"))).is_err());
        assert!(boolean.admits(PrimitiveLiteral::String("yes")).is_err());
    }

    #[test]
    fn a_string_admits_any_text_and_no_number() {
        let string = xsd("string");
        assert!(string.admits(PrimitiveLiteral::String("2024-01-01T00:00:00Z")).is_ok());
        assert!(string.admits(PrimitiveLiteral::Number(&number("42"))).is_err());
    }

    #[test]
    fn an_xsd_date_refuses_a_day_the_calendar_does_not_have() {
        assert!(xsd("date").admits(PrimitiveLiteral::String("2024-02-28")).is_ok());
        assert!(xsd("date").admits(PrimitiveLiteral::String("2024-02-30")).is_err());
    }

    /// `xsd:dateTime` admits a zone offset; `ngsi-ld:DateTime` does not
    /// (clause 4.6.3).
    #[test]
    fn an_xsd_date_time_admits_an_offset_the_ngsi_ld_date_time_refuses() {
        let offset = PrimitiveLiteral::String("2024-01-01T01:00:00+01:00");
        assert!(xsd("dateTime").admits(offset).is_ok());
        let ngsi_ld = recognised("https://uri.etsi.org/ngsi-ld/DateTime");
        assert_eq!(ngsi_ld, RecognisedDatatype::Time(TimeValueForm::DateTime));
        assert!(matches!(ngsi_ld.admits(offset), Err(DatatypeError::Time(_))));
    }

    #[test]
    fn a_time_datatype_on_a_number_is_refused() {
        let time = recognised("https://uri.etsi.org/ngsi-ld/Time");
        assert!(matches!(time.admits(PrimitiveLiteral::Number(&number("123"))), Err(DatatypeError::Time(_))));
    }

    /// Annex C's `"valueType": "Integer"` expands into the default context and
    /// C.10's `PostalAddress` into a user vocabulary; RDF 1.1 Concepts section
    /// 5.3 gives an unrecognised datatype nothing to check.
    #[test]
    fn an_unrecognised_datatype_iri_carries_no_check() {
        for iri in [
            "https://uri.etsi.org/ngsi-ld/default-context/Integer",
            "http://example.org/PostalAddress",
            "http://www.w3.org/2001/XMLSchema#String",
            "http://www.w3.org/2001/XMLSchema#QName",
        ] {
            assert!(RecognisedDatatype::of(&reference(iri)).expect("an IRI").is_none(), "{iri}");
        }
    }

    #[test]
    fn a_datatype_that_is_no_absolute_iri_is_refused() {
        assert!(matches!(RecognisedDatatype::of(&reference("Integer")), Err(DatatypeError::NotAnIri(_))));
    }

    /// Table 5.2.32-1: the `valueType` of a `LanguageProperty` "shall be equal
    /// to `langString`".
    #[test]
    fn a_language_map_value_type_is_lang_string_only() {
        assert!(admit_language_map_value_type(&reference("http://www.w3.org/1999/02/22-rdf-syntax-ns#langString")).is_ok());
        assert!(matches!(
            admit_language_map_value_type(&reference("http://www.w3.org/2001/XMLSchema#string")),
            Err(DatatypeError::NotLangString(_))
        ));
    }
}
