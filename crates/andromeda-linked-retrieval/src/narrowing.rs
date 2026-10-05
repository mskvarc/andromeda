//! How the clause 4.21 projection narrows each Linked Entity: a plain list
//! "applies to all Entities in the payload in the case of Linked Entity
//! retrieval" (clauses 5.7.1.5, 5.7.2.5 and 5.8.6), and a Linked Entity Term
//! addresses the entity its Relationship reaches (clause 4.21).

pub mod encounter;
pub mod list;
pub mod list_terms;
pub mod member;
