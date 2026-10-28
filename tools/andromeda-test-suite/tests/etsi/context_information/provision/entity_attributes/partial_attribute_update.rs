//! Partial Attribute Update (clauses 5.6.4 and 6.7.3.1): the members of an attribute that a partial update changes and
//! keeps, the refusals of a missing or malformed entity id, of a body that breaks the rules, and of an unknown entity
//! or attribute, the deletion of an attribute, of one of its instances and of a sub-attribute with the NGSI-LD Null in
//! the shape of its kind, and the partial update of a `JsonProperty` and of a `VocabProperty`.

mod e012_01;
mod e012_02;
mod e012_03;
mod e012_04;
mod e012_05;
mod e012_06;
mod e012_07;
mod e012_08;
mod e012_09;
mod e012_10;
