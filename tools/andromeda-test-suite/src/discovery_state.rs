//! The state discovery reports for the vocabulary `https://example.org/<ns>/random/`, compared with the state a test
//! expects after a run of writes.
//!
//! Discovery is exact (clauses 5.7.5 to 5.7.10): the type list, the detailed type list, each type's information, the
//! attribute list, the detailed attribute list and each attribute's information all follow what the Entities then
//! hold. One expectation states the whole state, and one pass reads all six documents and names every difference.

use crate::{
    broker::BrokerClient,
    namespace::Namespace,
    wire::{WireError, request_to},
};
use http::{HeaderValue, Method, header::ACCEPT};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::headers::Accept,
    model::{
        AttributeKind,
        DocumentError,
        NgsiLdMediaType,
        discovery::{AttributeInfo, EntityTypeInfo},
    },
};
use std::{num::ParseIntError, string::FromUtf8Error};

/// One entity type discovery must report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedType {
    /// The type's short name (`T2`), below the namespace's vocabulary.
    pub name: String,
    /// The number of Entities of the type.
    pub entity_count: u64,
    /// The attributes of the type's Entities, each short name with the kinds its instances declare.
    pub attributes: Vec<(String, Vec<AttributeKind>)>,
}

/// One attribute discovery must report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedAttribute {
    /// The attribute's short name (`a1`), below the namespace's vocabulary.
    pub name: String,
    /// The number of instances of the attribute.
    pub count: u64,
    /// The kinds of the attribute's instances.
    pub kinds: Vec<AttributeKind>,
    /// The short names of the types whose Entities carry the attribute.
    pub types: Vec<String>,
}

/// Everything discovery must report after a run of writes: the types are the whole type list, the attributes the
/// whole attribute list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedDiscovery {
    /// The types.
    pub types: Vec<ExpectedType>,
    /// The attributes.
    pub attributes: Vec<ExpectedAttribute>,
}

/// Why an expectation text is not an expectation.
#[derive(Debug, thiserror::Error)]
pub enum ExpectationError {
    /// A line starts with neither `type` nor `attribute`.
    #[error("line {line} is neither a type nor an attribute")]
    Line {
        /// The one-based number of the line.
        line: usize,
    },
    /// A line lacks one of its fields.
    #[error("line {line} lacks a field")]
    Field {
        /// The one-based number of the line.
        line: usize,
    },
    /// A count is not a number.
    #[error("line {line} holds a count that is not a number")]
    Count {
        /// The one-based number of the line.
        line: usize,
        /// Why the count is not a number.
        #[source]
        source: ParseIntError,
    },
    /// A kind names no attribute kind.
    #[error("line {line} names a kind that is not an attribute kind")]
    Kind {
        /// The one-based number of the line.
        line: usize,
        /// Why the kind names no attribute kind.
        #[source]
        source: strum::ParseError,
    },
}

/// Why discovery could not be read.
#[derive(Debug, thiserror::Error)]
pub enum DiscoveryReadError {
    /// A request failed or was refused.
    #[error(transparent)]
    Client(#[from] ClientError<ReqwestError>),
    /// A request target is not a URI.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// An answer body is not UTF-8 text.
    #[error("an answer is not UTF-8 text")]
    Text(#[from] FromUtf8Error),
    /// An answer is not the discovery document its path stands for.
    #[error("an answer is not a valid discovery document")]
    Document(#[from] DocumentError),
}

/// `name` below the namespace's vocabulary, as discovery names it.
fn full_name(namespace: &Namespace, name: &str) -> String {
    format!("https://example.org/{namespace}/random/{name}")
}

/// The path segment naming `name`: its expanded IRI with the separators percent-encoded.
fn path_segment(namespace: &Namespace, name: &str) -> String {
    format!("https%3A%2F%2Fexample.org%2F{namespace}%2Frandom%2F{name}")
}

/// The kinds as sorted text, so two lists compare as sets.
fn kind_texts(kinds: &[AttributeKind]) -> Vec<String> {
    let mut texts: Vec<String> = kinds.iter().map(ToString::to_string).collect();
    texts.sort();
    texts
}

/// `items` as sorted text.
fn sorted<I: IntoIterator<Item = String>>(items: I) -> Vec<String> {
    let mut texts: Vec<String> = items.into_iter().collect();
    texts.sort();
    texts
}

/// The difference between what was `read` and what was `expected`, named by `what`, when there is one.
fn difference(what: &str, read: &[String], expected: &[String]) -> Option<String> {
    (read != expected).then(|| format!("{what}: read {read:?}, expected {expected:?}"))
}

/// The `GET` of `path` with `Accept: application/json`, answered as text.
async fn get_text(client: &BrokerClient, path: &str) -> Result<String, DiscoveryReadError> {
    let request = request_to(client, Method::GET, path)?.append_header(ACCEPT, HeaderValue::from_static("application/json"));
    let response = client.send_unchecked(request).await?;
    Ok(String::from_utf8(response.into_body().to_vec())?)
}

impl ExpectedDiscovery {
    /// The expectation `text` states, one line per type (`type T2 1 a1=VocabProperty a5=GeoProperty+Property`: name,
    /// entity count, then each attribute with its kinds joined by `+`) and one per attribute (`attribute a1 2
    /// VocabProperty T2,T4`: name, count, kinds joined by `+`, then the types joined by `,`).
    ///
    /// # Errors
    ///
    /// Returns [`ExpectationError`] for a line that is not one of the two, lacks a field, or names a count or a kind
    /// wrongly.
    pub fn parse(text: &str) -> Result<ExpectedDiscovery, ExpectationError> {
        let mut types = Vec::new();
        let mut attributes = Vec::new();
        for (number, line) in text
            .lines()
            .enumerate()
            .map(|(index, line)| (index + 1, line.trim()))
            .filter(|(_, line)| !line.is_empty())
        {
            let mut fields = line.split_whitespace();
            let lacking = || ExpectationError::Field { line: number };
            match fields.next() {
                Some("type") => {
                    let name = fields.next().ok_or_else(lacking)?.to_owned();
                    let entity_count = count(number, fields.next().ok_or_else(lacking)?)?;
                    let attributes = fields
                        .map(|field| {
                            let (attribute, kinds) = field.split_once('=').ok_or_else(lacking)?;
                            Ok((attribute.to_owned(), kinds_of(number, kinds)?))
                        })
                        .collect::<Result<Vec<_>, ExpectationError>>()?;
                    types.push(ExpectedType {
                        name,
                        entity_count,
                        attributes,
                    });
                }
                Some("attribute") => {
                    let name = fields.next().ok_or_else(lacking)?.to_owned();
                    let attribute_count = count(number, fields.next().ok_or_else(lacking)?)?;
                    let kinds = kinds_of(number, fields.next().ok_or_else(lacking)?)?;
                    let type_names = fields.next().ok_or_else(lacking)?.split(',').map(str::to_owned).collect();
                    attributes.push(ExpectedAttribute {
                        name,
                        count: attribute_count,
                        kinds,
                        types: type_names,
                    });
                }
                Some(_) | None => return Err(ExpectationError::Line { line: number }),
            }
        }
        Ok(ExpectedDiscovery { types, attributes })
    }

    /// Reads the six discovery documents of the tenant `client` speaks to and returns every difference with the
    /// expectation; an empty list means discovery is exact.
    ///
    /// # Errors
    ///
    /// Returns [`DiscoveryReadError`] when a read fails or an answer is no discovery document.
    pub async fn differences(&self, client: &BrokerClient, namespace: &Namespace) -> Result<Vec<String>, DiscoveryReadError> {
        let mut found = Vec::new();
        let json = client.with_accept(Accept::new(NgsiLdMediaType::Json));
        let expected_types = sorted(self.types.iter().map(|expected| full_name(namespace, &expected.name)));
        let type_list = json.entity_types().list().send().await?;
        found.extend(difference(
            "typeList",
            &sorted(type_list.type_list.iter().map(ToString::to_string)),
            &expected_types,
        ));
        let type_details = json.entity_types().list_details().send().await?;
        found.extend(difference(
            "types?details=true",
            &sorted(type_details.iter().map(|detail| detail.type_name.to_string())),
            &expected_types,
        ));
        for expected in &self.types {
            let name = full_name(namespace, &expected.name);
            let detail = type_details.iter().find(|detail| detail.type_name.as_str() == name);
            found.extend(detail.and_then(|detail| {
                difference(
                    &format!("attributeNames of {name} in the details"),
                    &sorted(detail.attribute_names.iter().map(ToString::to_string)),
                    &sorted(expected.attributes.iter().map(|(attribute, _)| full_name(namespace, attribute))),
                )
            }));
            let path = format!("types/{}", path_segment(namespace, &expected.name));
            let information = EntityTypeInfo::parse(&get_text(client, &path).await?)?;
            found.extend(Self::type_information_differences(namespace, expected, &information));
        }
        let expected_attributes = sorted(self.attributes.iter().map(|expected| full_name(namespace, &expected.name)));
        let attribute_list = json.attributes().list().send().await?;
        found.extend(difference(
            "attributeList",
            &sorted(attribute_list.attribute_list.iter().map(ToString::to_string)),
            &expected_attributes,
        ));
        let attribute_details = json.attributes().list_details().send().await?;
        found.extend(difference(
            "attributes?details=true",
            &sorted(attribute_details.iter().map(|detail| detail.attribute_name.to_string())),
            &expected_attributes,
        ));
        for expected in &self.attributes {
            let name = full_name(namespace, &expected.name);
            let expected_types = sorted(expected.types.iter().map(|type_name| full_name(namespace, type_name)));
            let detail = attribute_details.iter().find(|detail| detail.attribute_name.as_str() == name);
            found.extend(detail.and_then(|detail| {
                difference(
                    &format!("typeNames of {name} in the details"),
                    &sorted(detail.type_names.iter().flatten().map(ToString::to_string)),
                    &expected_types,
                )
            }));
            let path = format!("attributes/{}", path_segment(namespace, &expected.name));
            let information = AttributeInfo::parse(&get_text(client, &path).await?)?;
            found.extend(Self::attribute_information_differences(&name, expected, &expected_types, &information));
        }
        Ok(found)
    }

    /// The differences between one type's information and `expected`.
    fn type_information_differences(namespace: &Namespace, expected: &ExpectedType, information: &EntityTypeInfo) -> Vec<String> {
        let name = full_name(namespace, &expected.name);
        let mut found = Vec::new();
        if information.entity_count != expected.entity_count {
            found.push(format!(
                "entityCount of {name}: read {}, expected {}",
                information.entity_count, expected.entity_count
            ));
        }
        found.extend(difference(
            &format!("attributeDetails of {name}"),
            &sorted(information.attribute_details.iter().map(|detail| detail.attribute_name.to_string())),
            &sorted(expected.attributes.iter().map(|(attribute, _)| full_name(namespace, attribute))),
        ));
        for (attribute, kinds) in &expected.attributes {
            let attribute_name = full_name(namespace, attribute);
            let detail = information
                .attribute_details
                .iter()
                .find(|detail| detail.attribute_name.as_str() == attribute_name);
            found.extend(detail.and_then(|detail| {
                difference(
                    &format!("attributeTypes of {attribute_name} in {name}"),
                    &kind_texts(detail.attribute_types.as_deref().unwrap_or_default()),
                    &kind_texts(kinds),
                )
            }));
        }
        found
    }

    /// The differences between one attribute's information and `expected`.
    fn attribute_information_differences(name: &str, expected: &ExpectedAttribute, expected_types: &[String], information: &AttributeInfo) -> Vec<String> {
        let mut found = Vec::new();
        if information.attribute_count != Some(expected.count) {
            found.push(format!(
                "attributeCount of {name}: read {:?}, expected {}",
                information.attribute_count, expected.count
            ));
        }
        found.extend(difference(
            &format!("attributeTypes of {name}"),
            &kind_texts(information.attribute_types.as_deref().unwrap_or_default()),
            &kind_texts(&expected.kinds),
        ));
        found.extend(difference(
            &format!("typeNames of {name}"),
            &sorted(information.type_names.iter().flatten().map(ToString::to_string)),
            expected_types,
        ));
        found
    }
}

/// `text` on line `line` as a count.
fn count(line: usize, text: &str) -> Result<u64, ExpectationError> {
    text.parse().map_err(|source| ExpectationError::Count { line, source })
}

/// The kinds `text` on line `line` joins with `+`.
fn kinds_of(line: usize, text: &str) -> Result<Vec<AttributeKind>, ExpectationError> {
    text.split('+')
        .map(|kind| kind.parse().map_err(|source| ExpectationError::Kind { line, source }))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        discovery_state::{ExpectationError, ExpectedDiscovery, difference, full_name, kind_texts, path_segment, sorted},
        namespace::Namespace,
    };
    use googletest::prelude::*;
    use ngsi_ld::model::AttributeKind;

    #[gtest]
    fn names_are_written_below_the_vocabulary_and_encoded_in_a_path() {
        let namespace = Namespace::fresh();
        expect_that!(full_name(&namespace, "T2"), eq(&format!("https://example.org/{namespace}/random/T2")));
        expect_that!(
            path_segment(&namespace, "a1"),
            eq(&format!("https%3A%2F%2Fexample.org%2F{namespace}%2Frandom%2Fa1"))
        );
    }

    #[gtest]
    fn kinds_compare_as_sets_and_a_difference_names_both_sides() {
        let one = kind_texts(&[AttributeKind::Property, AttributeKind::GeoProperty]);
        let other = kind_texts(&[AttributeKind::GeoProperty, AttributeKind::Property]);
        expect_that!(difference("kinds", &one, &other), none());
        let missing = difference("kinds", &one, &sorted([String::from("Property")]));
        expect_that!(missing, some(contains_substring("kinds: read")));
    }

    #[gtest]
    fn an_expectation_text_states_its_types_and_attributes() -> Result<()> {
        let expected = ExpectedDiscovery::parse(
            "type T2 1 a1=VocabProperty a5=GeoProperty+Property
attribute a1 2 VocabProperty T2,T4",
        )?;
        expect_that!(expected.types, len(eq(1)));
        let first = expected.types.first().or_fail()?;
        expect_that!(first.name, eq("T2"));
        expect_that!(first.entity_count, eq(1));
        expect_that!(first.attributes, len(eq(2)));
        let attribute = expected.attributes.first().or_fail()?;
        expect_that!(attribute.count, eq(2));
        expect_that!(attribute.types, elements_are![eq("T2"), eq("T4")]);
        Ok(())
    }

    #[gtest]
    fn an_expectation_text_with_an_unknown_kind_is_refused() {
        let refused = ExpectedDiscovery::parse("attribute a1 2 Nonsense T2");
        expect_that!(
            refused,
            err(predicate(|error: &ExpectationError| matches!(error, ExpectationError::Kind { line: 1, .. })))
        );
    }
}
