//! The data type a literal's declaration gives it, and the one spelling the
//! broker holds and serves a declared value in.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.9 compares a target only with an operand of
//! its own data type. A value's data type is its declared datatype when it has
//! one, and its JSON type when it has none; the attribute's `valueType`
//! decides before the literal's own `@type`. A JSON string declared as one of
//! the XSD numeric datatypes (`xsd:decimal`, every datatype derived from it,
//! `xsd:float` and `xsd:double`) is a number, one declared as `xsd:boolean` a
//! Boolean, and one declared as a clause 4.6.3 time value that time value.
//! Every other declaration leaves it a String.
//!
//! A declared value keeps what makes it a typed literal, so it is served as a
//! JSON string with its datatype, never as a JSON number or boolean (a JSON-LD
//! native number is another literal). It is spelled in the canonical lexical
//! form of its datatype (XML Schema Definition Language 1.1 Part 2): a
//! decimal or an integer in plain decimal notation with no exponent, no
//! leading `+`, no leading zeros and no trailing fractional zeros; a float or
//! a double in the broker's number spelling, or `INF`, `-INF` or `NaN`; a
//! Boolean as `true` or `false`.

use crate::{
    datatype::{
        declaration::LiteralDeclaration,
        primitive_literal::PrimitiveLiteral,
        recognised::{DatatypeError, RecognisedDatatype},
    },
    reference::NgsiLdReference,
    time::value::{TimeValue, TimeValueError},
};
use andromeda_ngsi_ld_number::declared_number::DeclaredNumber;
use derive_more::Display;
use iri_rs::Iri;
use xsd_rs::{
    Datatype,
    XSD_BOOLEAN,
    XSD_BYTE,
    XSD_DECIMAL,
    XSD_DOUBLE,
    XSD_FLOAT,
    XSD_INT,
    XSD_INTEGER,
    XSD_LONG,
    XSD_NEGATIVE_INTEGER,
    XSD_NON_NEGATIVE_INTEGER,
    XSD_NON_POSITIVE_INTEGER,
    XSD_POSITIVE_INTEGER,
    XSD_SHORT,
    XSD_UNSIGNED_BYTE,
    XSD_UNSIGNED_INT,
    XSD_UNSIGNED_LONG,
    XSD_UNSIGNED_SHORT,
};

/// The IRI of every XSD numeric datatype: `xsd:decimal`, the datatypes
/// derived from it (XML Schema Definition Language 1.1 Part 2, section 3.4),
/// `xsd:float` and `xsd:double`.
///
/// A store that reads a declaration from the datatype IRI it holds beside a
/// value, rather than from the literal, names them all.
pub const NUMERIC_DATATYPES: [Iri<&'static str>; 16] = [
    XSD_DECIMAL,
    XSD_INTEGER,
    XSD_LONG,
    XSD_INT,
    XSD_SHORT,
    XSD_BYTE,
    XSD_NON_NEGATIVE_INTEGER,
    XSD_POSITIVE_INTEGER,
    XSD_NON_POSITIVE_INTEGER,
    XSD_NEGATIVE_INTEGER,
    XSD_UNSIGNED_LONG,
    XSD_UNSIGNED_INT,
    XSD_UNSIGNED_SHORT,
    XSD_UNSIGNED_BYTE,
    XSD_FLOAT,
    XSD_DOUBLE,
];

/// The IRI of `xsd:boolean`, the one datatype that declares a Boolean.
pub const BOOLEAN_DATATYPE: Iri<&'static str> = XSD_BOOLEAN;

/// The canonical lexical form a declared number is spelled in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberSpelling {
    /// `xsd:decimal` and the integer datatypes derived from it: plain decimal
    /// notation.
    Decimal,
    /// `xsd:float` and `xsd:double`: the broker's number spelling, or the XSD
    /// lexical form of a value beyond the decimals.
    Floating,
}

impl NumberSpelling {
    /// The spelling of the numeric datatype `datatype` names, `None` when it
    /// names no numeric datatype.
    #[must_use]
    pub fn named_by(datatype: &NgsiLdReference) -> Option<NumberSpelling> {
        match RecognisedDatatype::of(datatype) {
            Ok(Some(RecognisedDatatype::Xsd(xsd))) => NumberSpelling::of_xsd(xsd),
            Ok(Some(RecognisedDatatype::Time(_) | RecognisedDatatype::LangString | RecognisedDatatype::Json) | None) | Err(_) => None,
        }
    }

    /// The spelling of the XSD datatype `xsd`, `None` when it is not numeric.
    const fn of_xsd(xsd: Datatype) -> Option<NumberSpelling> {
        match xsd {
            Datatype::Decimal(_) => Some(NumberSpelling::Decimal),
            Datatype::Float | Datatype::Double => Some(NumberSpelling::Floating),
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
            | Datatype::QName => None,
        }
    }

    /// `number` in this spelling.
    #[must_use]
    pub fn spell(self, number: &DeclaredNumber) -> String {
        match (self, number) {
            (NumberSpelling::Decimal, DeclaredNumber::Finite(finite)) => finite.decimal().to_plain_string(),
            (NumberSpelling::Floating, DeclaredNumber::Finite(_)) | (NumberSpelling::Decimal | NumberSpelling::Floating, DeclaredNumber::NonFinite(_)) => {
                number.to_string()
            }
        }
    }
}

/// A literal read as the data type its declaration gives it, displayed in
/// the one spelling the broker holds and serves it in.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub enum DeclaredValue {
    /// A clause 4.6.3 time value.
    #[display("{_0}")]
    Time(TimeValue),
    /// A number, declared by an XSD numeric datatype.
    #[display("{}", spelling.spell(number))]
    Number {
        /// The number.
        number: DeclaredNumber,
        /// The canonical lexical form of its datatype.
        spelling: NumberSpelling,
    },
    /// A Boolean, declared as `xsd:boolean`.
    #[display("{_0}")]
    Boolean(bool),
}

impl DeclaredValue {
    /// The value `literal` is under `declaration`, `None` when the declaration
    /// gives it no data type of its own and it is the plain JSON value it is.
    ///
    /// A time value form is declared by either declaration, which must then
    /// agree. A number or a Boolean is declared by the deciding datatype
    /// ([`LiteralDeclaration::deciding`]) on a JSON string only: a JSON number
    /// or boolean already is one.
    ///
    /// A declared datatype is what the client said the value is, so a literal
    /// it reads no value from is an error, never a string.
    #[must_use]
    pub fn of(declaration: LiteralDeclaration<'_>, literal: PrimitiveLiteral<'_>) -> Option<Result<DeclaredValue, DatatypeError>> {
        match declaration.time_form() {
            Ok(Some(form)) => {
                return Some(match literal {
                    PrimitiveLiteral::String(text) => form.parse(text).map(DeclaredValue::Time).map_err(DatatypeError::Time),
                    PrimitiveLiteral::Number(_) | PrimitiveLiteral::Boolean(_) => Err(DatatypeError::Time(TimeValueError::NotAString)),
                });
            }
            Ok(None) => {}
            Err(conflict) => return Some(Err(DatatypeError::Time(conflict))),
        }
        let PrimitiveLiteral::String(text) = literal else {
            return None;
        };
        let xsd = match RecognisedDatatype::of(declaration.deciding()?) {
            Ok(Some(RecognisedDatatype::Xsd(xsd))) => xsd,
            Ok(Some(RecognisedDatatype::Time(_) | RecognisedDatatype::LangString | RecognisedDatatype::Json) | None) => return None,
            Err(refusal) => return Some(Err(refusal)),
        };
        if let Some(spelling) = NumberSpelling::of_xsd(xsd) {
            return Some(numeric(xsd, text).map(|number| DeclaredValue::Number { number, spelling }));
        }
        match xsd {
            Datatype::Boolean => Some(boolean(text)),
            Datatype::Decimal(_)
            | Datatype::Float
            | Datatype::Double
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
            | Datatype::QName => None,
        }
    }
}

/// The number `text` names under the numeric datatype `xsd`.
fn numeric(xsd: Datatype, text: &str) -> Result<DeclaredNumber, DatatypeError> {
    xsd.parse(text).map_err(|_| DatatypeError::NotAValue(xsd.iri()))?;
    Ok(DeclaredNumber::from_xsd_lexical(text)?)
}

/// The Boolean `text` names under `xsd:boolean`, whose lexical space is
/// `true`, `false`, `1` and `0`.
fn boolean(text: &str) -> Result<DeclaredValue, DatatypeError> {
    match text {
        "true" | "1" => Ok(DeclaredValue::Boolean(true)),
        "false" | "0" => Ok(DeclaredValue::Boolean(false)),
        _ => Err(DatatypeError::NotAValue(Datatype::Boolean.iri())),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        datatype::{
            declaration::LiteralDeclaration,
            declared_value::{BOOLEAN_DATATYPE, DeclaredValue, NUMERIC_DATATYPES, NumberSpelling},
            primitive_literal::PrimitiveLiteral,
            recognised::DatatypeError,
        },
        reference::NgsiLdReference,
        time::value::{TimeValue, TimeValueError, TimeValueForm},
    };
    use andromeda_ngsi_ld_number::{
        JsonNumber,
        declared_number::{DeclaredNumber, NonFinite},
    };

    fn reference(iri: &str) -> NgsiLdReference {
        iri.parse().expect("a datatype IRI")
    }

    fn xsd(local: &str) -> NgsiLdReference {
        reference(&format!("http://www.w3.org/2001/XMLSchema#{local}"))
    }

    fn declared(datatype: Option<&NgsiLdReference>, value_type: Option<&NgsiLdReference>, text: &str) -> Option<Result<DeclaredValue, DatatypeError>> {
        DeclaredValue::of(LiteralDeclaration::new(datatype, value_type), PrimitiveLiteral::String(text))
    }

    fn spelled(datatype: &NgsiLdReference, text: &str) -> String {
        declared(Some(datatype), None, text).expect("declared").expect("a value").to_string()
    }

    #[test]
    fn a_decimal_or_an_integer_is_spelled_in_plain_decimal_notation() {
        for (local, written, canonical) in [
            ("integer", "01", "1"),
            ("decimal", "+1.50", "1.5"),
            ("decimal", "1.0", "1"),
            ("decimal", "100", "100"),
            ("decimal", "0.0000001", "0.0000001"),
            ("decimal", "-0", "0"),
            ("long", "+42", "42"),
            ("unsignedByte", "007", "7"),
        ] {
            pretty_assertions::assert_eq!(spelled(&xsd(local), written), canonical, "{local} {written}");
        }
    }

    #[test]
    fn a_float_or_a_double_is_spelled_as_the_broker_spells_a_number() {
        for (local, written, canonical) in [
            ("double", "1.0E2", "100"),
            ("double", "0.0000001", "1E-7"),
            ("float", "1.5", "1.5"),
            ("double", "INF", "INF"),
            ("double", "-INF", "-INF"),
            ("float", "NaN", "NaN"),
        ] {
            pretty_assertions::assert_eq!(spelled(&xsd(local), written), canonical, "{local} {written}");
        }
    }

    #[test]
    fn a_declared_boolean_is_spelled_true_or_false() {
        let boolean = xsd("boolean");
        for (written, canonical) in [("1", "true"), ("true", "true"), ("0", "false"), ("false", "false")] {
            pretty_assertions::assert_eq!(spelled(&boolean, written), canonical, "{written}");
        }
    }

    #[test]
    fn a_value_type_declares_a_number_before_the_literal_datatype() {
        let (string, integer) = (xsd("string"), xsd("integer"));
        let value = declared(Some(&string), Some(&integer), "4").expect("declared").expect("a number");
        pretty_assertions::assert_eq!(
            value,
            DeclaredValue::Number {
                number: DeclaredNumber::Finite(JsonNumber::from(4)),
                spelling: NumberSpelling::Decimal,
            }
        );
        assert!(declared(Some(&integer), Some(&string), "4").is_none(), "the valueType makes it a String");
    }

    #[test]
    fn a_double_holds_the_values_beyond_the_decimals() {
        let value = declared(None, Some(&xsd("double")), "-INF").expect("declared").expect("a number");
        assert!(
            matches!(
                value,
                DeclaredValue::Number {
                    number: DeclaredNumber::NonFinite(NonFinite::NegativeInfinity),
                    ..
                }
            ),
            "{value:?}"
        );
    }

    #[test]
    fn a_literal_its_datatype_reads_no_value_from_is_refused() {
        for (local, text) in [("integer", "1.5"), ("decimal", "INF"), ("boolean", "yes"), ("double", "abc")] {
            let refusal = declared(Some(&xsd(local)), None, text).expect("declared");
            assert!(matches!(refusal, Err(DatatypeError::NotAValue(_))), "{local} {text}: {refusal:?}");
        }
    }

    #[test]
    fn another_datatype_or_none_leaves_a_string_a_string() {
        for datatype in [
            None,
            Some(xsd("string")),
            Some(reference("https://uri.etsi.org/ngsi-ld/default-context/Integer")),
        ] {
            assert!(declared(datatype.as_ref(), None, "4").is_none(), "{datatype:?}");
        }
    }

    #[test]
    fn a_json_number_or_boolean_is_no_declared_value() {
        let integer = xsd("integer");
        let number = JsonNumber::from(4);
        assert!(DeclaredValue::of(LiteralDeclaration::new(Some(&integer), None), PrimitiveLiteral::Number(&number)).is_none());
        assert!(DeclaredValue::of(LiteralDeclaration::new(Some(&xsd("boolean")), None), PrimitiveLiteral::Boolean(true)).is_none());
    }

    #[test]
    fn a_literal_typed_date_time_is_a_date_time_in_its_served_spelling() {
        let date_time = reference(TimeValueForm::DateTime.iri().as_str());
        let value = declared(Some(&date_time), None, "2024-01-01T00:00:00.5Z")
            .expect("declared")
            .expect("a DateTime");
        assert!(matches!(value, DeclaredValue::Time(TimeValue::DateTime(_))), "{value:?}");
        pretty_assertions::assert_eq!(value.to_string(), "2024-01-01T00:00:00.500Z");
    }

    /// Annex C.6: a value is a time value only when it is declared as one.
    #[test]
    fn an_undeclared_string_in_date_time_form_is_no_time_value() {
        assert!(declared(None, None, "2024-01-01T00:00:00Z").is_none());
        assert!(declared(None, None, "12:00:00Z").is_none());
    }

    #[test]
    fn a_time_datatype_on_a_number_is_not_a_string() {
        let date_time = reference(TimeValueForm::DateTime.iri().as_str());
        let number = JsonNumber::from(123);
        for literal in [PrimitiveLiteral::Number(&number), PrimitiveLiteral::Boolean(true)] {
            let refusal = DeclaredValue::of(LiteralDeclaration::new(Some(&date_time), None), literal).expect("declared");
            assert!(matches!(refusal, Err(DatatypeError::Time(TimeValueError::NotAString))), "{refusal:?}");
        }
    }

    #[test]
    fn a_numeric_datatype_names_its_spelling() {
        pretty_assertions::assert_eq!(NumberSpelling::named_by(&xsd("int")), Some(NumberSpelling::Decimal));
        pretty_assertions::assert_eq!(NumberSpelling::named_by(&xsd("double")), Some(NumberSpelling::Floating));
        pretty_assertions::assert_eq!(NumberSpelling::named_by(&xsd("boolean")), None);
        pretty_assertions::assert_eq!(NumberSpelling::named_by(&reference("http://example.org/Integer")), None);
    }

    /// XML Schema Definition Language 1.1 Part 2, section 3.4: the datatypes
    /// derived from `xsd:decimal` are the thirteen integer datatypes.
    #[test]
    fn every_numeric_datatype_iri_names_a_number_spelling() {
        for iri in NUMERIC_DATATYPES {
            assert!(NumberSpelling::named_by(&reference(iri.as_str())).is_some(), "{iri}");
        }
        assert!(NumberSpelling::named_by(&reference(BOOLEAN_DATATYPE.as_str())).is_none());
    }

    #[test]
    fn a_non_finite_number_is_spelled_alike_in_either_spelling() {
        let infinity = DeclaredNumber::NonFinite(NonFinite::PositiveInfinity);
        pretty_assertions::assert_eq!(NumberSpelling::Decimal.spell(&infinity), "INF");
        pretty_assertions::assert_eq!(NumberSpelling::Floating.spell(&infinity), "INF");
    }
}
