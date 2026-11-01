//! What one literal is declared as: its own datatype and the `valueType` of
//! the attribute it is a value of.
//!
//! `valueType` declares the datatype of every primitive member of the value it
//! sits beside (ETSI GS CIM 009 v1.9.1 clause 4.5.2): the single value, each
//! value of a multi-valued Property, each item of a `ListProperty`. Inside a
//! structured value only a member's own `@type` declares anything, so a member
//! carries no `valueType` here.

use crate::{
    reference::NgsiLdReference,
    time::value::{TimeValueError, TimeValueForm},
};

/// The two declarations one literal carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LiteralDeclaration<'a> {
    /// The literal's JSON-LD `@type`, expanded.
    pub datatype: Option<&'a NgsiLdReference>,
    /// The `valueType` of the attribute, expanded.
    pub value_type: Option<&'a NgsiLdReference>,
}

impl<'a> LiteralDeclaration<'a> {
    /// The declaration of a literal carrying `datatype` in an attribute whose
    /// `valueType` is `value_type`.
    #[must_use]
    pub const fn new(datatype: Option<&'a NgsiLdReference>, value_type: Option<&'a NgsiLdReference>) -> LiteralDeclaration<'a> {
        LiteralDeclaration { datatype, value_type }
    }

    /// The datatype that decides the literal's data type: the `valueType` when
    /// the attribute declares one, else the literal's own datatype.
    #[must_use]
    pub fn deciding(self) -> Option<&'a NgsiLdReference> {
        self.value_type.or(self.datatype)
    }

    /// The time value form the declaration names, `None` when it names none.
    ///
    /// # Errors
    ///
    /// Returns [`TimeValueError::ConflictingDeclarations`] when the datatype and
    /// the `valueType` are two different datatypes and one of them is a time
    /// value form: a literal has one datatype.
    pub fn time_form(self) -> Result<Option<TimeValueForm>, TimeValueError> {
        let named = |reference: Option<&NgsiLdReference>| reference.and_then(|reference| TimeValueForm::named_by(reference.as_str()));
        let (by_datatype, by_value_type) = (named(self.datatype), named(self.value_type));
        match (self.datatype, self.value_type) {
            (Some(datatype), Some(value_type)) if datatype != value_type && (by_datatype.is_some() || by_value_type.is_some()) => {
                Err(TimeValueError::ConflictingDeclarations)
            }
            (Some(_) | None, Some(_) | None) => Ok(by_datatype.or(by_value_type)),
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        datatype::declaration::LiteralDeclaration,
        reference::NgsiLdReference,
        time::value::{TimeValueError, TimeValueForm},
    };

    fn reference(iri: &str) -> NgsiLdReference {
        iri.parse().expect("a datatype IRI")
    }

    fn form(form: TimeValueForm) -> NgsiLdReference {
        reference(form.iri().as_str())
    }

    #[test]
    fn a_value_type_decides_before_the_literal_datatype() {
        let (integer, decimal) = (
            reference("http://www.w3.org/2001/XMLSchema#integer"),
            reference("http://www.w3.org/2001/XMLSchema#decimal"),
        );
        pretty_assertions::assert_eq!(LiteralDeclaration::new(Some(&integer), Some(&decimal)).deciding(), Some(&decimal));
        pretty_assertions::assert_eq!(LiteralDeclaration::new(Some(&integer), None).deciding(), Some(&integer));
        pretty_assertions::assert_eq!(LiteralDeclaration::new(None, None).deciding(), None);
    }

    #[test]
    fn a_value_type_declares_the_time_form_of_a_plain_literal() {
        let time = form(TimeValueForm::Time);
        pretty_assertions::assert_eq!(
            LiteralDeclaration::new(None, Some(&time)).time_form().expect("one form"),
            Some(TimeValueForm::Time)
        );
    }

    #[test]
    fn a_datatype_and_a_value_type_naming_two_forms_conflict() {
        let (date, time) = (form(TimeValueForm::Date), form(TimeValueForm::Time));
        let refusal = LiteralDeclaration::new(Some(&date), Some(&time)).time_form();
        assert!(matches!(refusal, Err(TimeValueError::ConflictingDeclarations)), "got {refusal:?}");
    }

    #[test]
    fn a_non_time_datatype_beside_a_time_value_type_conflicts() {
        let (string, date_time) = (reference("http://www.w3.org/2001/XMLSchema#string"), form(TimeValueForm::DateTime));
        let refusal = LiteralDeclaration::new(Some(&string), Some(&date_time)).time_form();
        assert!(matches!(refusal, Err(TimeValueError::ConflictingDeclarations)), "got {refusal:?}");
    }

    #[test]
    fn a_datatype_and_a_value_type_naming_one_form_agree() {
        let date_time = form(TimeValueForm::DateTime);
        let named = LiteralDeclaration::new(Some(&date_time), Some(&date_time)).time_form().expect("one form");
        pretty_assertions::assert_eq!(named, Some(TimeValueForm::DateTime));
    }

    #[test]
    fn two_non_time_datatypes_name_no_time_form() {
        let (integer, decimal) = (
            reference("http://www.w3.org/2001/XMLSchema#integer"),
            reference("http://www.w3.org/2001/XMLSchema#decimal"),
        );
        pretty_assertions::assert_eq!(LiteralDeclaration::new(Some(&integer), Some(&decimal)).time_form().expect("no conflict"), None);
    }
}
