//! An attribute nesting as deeply as `serde_json`'s reader admits is expanded,
//! by the typed walk and by the processor, without exhausting the native
//! stack.
//!
//! The assembly reads each sub-attribute by the reading of its parent instance,
//! about 30 KiB of stack a level unoptimized. A write is held to its nesting
//! only once its document is assembled, so the assembly has to complete for
//! the deepest attribute a request can carry, on the stack a Tokio worker has
//! left inside a request handler.

use crate::{
    expansion_fixture::{engine, entity},
    normalization::Normalization,
};
use serde_json::{Value, json};
use std::thread;
use tokio::runtime::Builder;

/// The core context's default vocabulary, which every undefined term expands
/// against (clause 5.5.7).
const DEFAULT: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

/// A Property carrying a chain of `levels` sub-attributes, each a Property of a
/// scalar.
fn sub_attribute_chain(levels: usize) -> Value {
    (0..levels).fold(
        json!({"type": "Property", "value": 1}),
        |inner, _| json!({"type": "Property", "value": 1, "s": inner}),
    )
}

/// A chain of 124 sub-attributes, the longest one attribute of an entity holds
/// within the reader's 127 containers, expands both ways on a 512 KiB thread
/// to a document measuring 124 levels.
#[test]
fn a_chain_of_sub_attributes_as_long_as_the_reader_admits_expands_on_a_small_stack() {
    let nesting = thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let runtime = Builder::new_current_thread().enable_all().build().expect("the runtime starts");
            let body = json!({"id": "urn:ngsi-ld:Probe:1", "type": "Probe", "p": sub_attribute_chain(124)});
            let document = runtime
                .block_on(entity(&engine(), body, Normalization::Entity))
                .expect("the fixture expands")
                .agreed();
            let attribute = format!("{DEFAULT}p");
            let member = document.attributes().get(attribute.as_str()).expect("the attribute is written");
            member.instances()[0].nesting().get()
        })
        .expect("the thread starts")
        .join()
        .expect("the expansion stays within the stack");
    pretty_assertions::assert_eq!(nesting, 124);
}
