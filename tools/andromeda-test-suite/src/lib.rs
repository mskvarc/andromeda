//! The support library of Andromeda's black-box tests.
//!
//! Every test runs in a process of its own under `cargo nextest`, against a stack of brokers it reaches over the
//! network. Each test draws a [`namespace::Namespace`], speaks to a broker through a client of the `ngsi-ld` crate
//! ([`broker`]), receives notifications and forwarded requests in servers of its own ([`notification_sink`],
//! [`context_source`], [`mqtt_observer`]), and compares what it reads with the matchers of [`comparison`],
//! [`problem`] and [`wire`].

#![deny(missing_docs)]
#![deny(clippy::missing_docs_in_private_items)]

pub mod aggregation_matcher;
pub mod answered_status;
pub mod attribute_check;
pub mod attribute_matcher;
pub mod attribute_reading;
pub mod body_pattern;
pub mod broker;
pub mod callback_server;
pub mod cleanup;
pub mod clock_wait;
pub mod comparison;
pub mod config;
pub mod context_source;
pub mod core_context;
pub mod entity_matcher;
pub mod equivalence;
pub mod eventually;
pub mod evolution_matcher;
pub mod exchange_log;
pub mod forwarded_body;
pub mod forwarded_head;
pub mod forwarded_query;
pub mod geometry;
pub mod geometry_matcher;
pub mod header_condition;
pub mod header_matcher;
pub mod hosted_context;
pub mod identity_matcher;
pub mod instance_time;
pub mod link_matcher;
pub mod linked_entity;
pub mod mqtt_observer;
pub mod namespace;
pub mod ngsild_date;
pub mod notification_sink;
pub mod nth;
pub mod outcome;
pub mod page_link;
pub mod partial_outcome;
pub mod problem;
pub mod received_status;
pub mod remote_context;
pub mod report_matcher;
pub mod robot_literal;
pub mod sequence;
pub mod simplified_matcher;
pub mod source_answer;
pub mod source_page;
pub mod source_request;
pub mod spelling_matcher;
pub mod system_time;
pub mod teardown;
pub mod tls_connection;
pub mod type_matcher;
pub mod unchecked_document;
pub mod value;
pub mod values_matcher;
pub mod vocabulary;
pub mod wire;
