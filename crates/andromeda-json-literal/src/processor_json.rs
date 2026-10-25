//! Handing a literal to the JSON-LD processor and reading one back.
//!
//! The processor holds an `@json` value in its own JSON type; that type is
//! built here from the literal's text and printed back into one, and never
//! held anywhere else. A value the processor wrote -- the compaction of one
//! value object -- is handed on through [`ProcessorJsonReader`], which builds
//! the reader's own form of it without naming the processor's type.

use crate::{JsonLiteral, admission::JsonLiteralError};
use jstrict::{NumberBuf, Print, Value, parse::parse_str_value};

impl JsonLiteral {
    /// The literal as the JSON-LD processor holds an `@json` value.
    ///
    /// # Errors
    ///
    /// Returns [`JsonLiteralError::ProcessorText`] when the processor's reader
    /// refuses the admitted text.
    pub fn to_processor_json(&self) -> Result<Value, JsonLiteralError> {
        parse_str_value(self.text()).map_err(|source| JsonLiteralError::ProcessorText { source })
    }

    /// Admit the `@json` value the JSON-LD processor holds.
    ///
    /// # Errors
    ///
    /// As [`JsonLiteral::admit`].
    pub fn from_processor_json(value: &Value) -> Result<JsonLiteral, JsonLiteralError> {
        JsonLiteral::admit(&value.compact_print().to_string())
    }
}

/// The JSON string `text` as the JSON-LD processor holds it.
#[must_use]
pub fn processor_string(text: &str) -> Value {
    Value::String(text.into())
}

/// What a reader builds from a JSON value the JSON-LD processor wrote, one
/// level at a time.
pub trait ProcessorJsonReader {
    /// The reader's form of one JSON value.
    type Read;

    /// `null`.
    fn null(&mut self) -> Self::Read;
    /// `true` or `false`.
    fn boolean(&mut self, flag: bool) -> Self::Read;
    /// A number, in the processor's spelling.
    fn number(&mut self, number: NumberBuf) -> Self::Read;
    /// A string.
    fn string(&mut self, text: &str) -> Self::Read;
    /// An array of values already read, in order.
    fn array(&mut self, items: Vec<Self::Read>) -> Self::Read;
    /// An object of members already read, in written order.
    fn object(&mut self, members: Vec<(String, Self::Read)>) -> Self::Read;
}

/// One container being read: its finished children, and those still to read.
enum Frame<R> {
    /// An array.
    Array {
        /// The items read so far.
        read: Vec<R>,
        /// The items not read yet, last first.
        pending: Vec<Value>,
    },
    /// An object.
    Object {
        /// The members read so far.
        read: Vec<(String, R)>,
        /// The key of the member being read.
        current: Option<String>,
        /// The members not read yet, last first.
        pending: Vec<(String, Value)>,
    },
}

/// Read `value`, which the JSON-LD processor wrote, into `reader`'s form.
///
/// The walk keeps its own stack of open containers, so a value of any depth
/// is read without recursing.
pub fn read_processor_json<R: ProcessorJsonReader>(value: Value, reader: &mut R) -> R::Read {
    let mut stack: Vec<Frame<R::Read>> = Vec::new();
    let mut next = Some(value);
    loop {
        let finished = match next.take() {
            Some(Value::Null) => Some(reader.null()),
            Some(Value::Boolean(flag)) => Some(reader.boolean(flag)),
            Some(Value::Number(number)) => Some(reader.number(number)),
            Some(Value::String(text)) => Some(reader.string(&text)),
            Some(Value::Array(items)) => {
                let mut pending = items.into_vec();
                pending.reverse();
                stack.push(Frame::Array { read: Vec::new(), pending });
                None
            }
            Some(Value::Object(members)) => {
                let mut pending: Vec<(String, Value)> = members.into_iter().map(|entry| (entry.key.to_string(), entry.value)).collect();
                pending.reverse();
                stack.push(Frame::Object {
                    read: Vec::new(),
                    current: None,
                    pending,
                });
                None
            }
            None => None,
        };
        let mut finished = finished;
        loop {
            let Some(frame) = stack.last_mut() else {
                if let Some(done) = finished {
                    return done;
                }
                break;
            };
            match frame {
                Frame::Array { read, pending } => {
                    if let Some(done) = finished.take() {
                        read.push(done);
                    }
                    if let Some(item) = pending.pop() {
                        next = Some(item);
                        break;
                    }
                }
                Frame::Object { read, current, pending } => {
                    if let (Some(done), Some(key)) = (finished.take(), current.take()) {
                        read.push((key, done));
                    }
                    if let Some((key, value)) = pending.pop() {
                        *current = Some(key);
                        next = Some(value);
                        break;
                    }
                }
            }
            finished = match stack.pop() {
                Some(Frame::Array { read, .. }) => Some(reader.array(read)),
                Some(Frame::Object { read, .. }) => Some(reader.object(read)),
                None => None,
            };
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
        JsonLiteral,
        json_text::quoted,
        processor_json::{ProcessorJsonReader, processor_string, read_processor_json},
    };
    use jstrict::{NumberBuf, Print, parse::parse_str_value};
    use std::thread;

    /// Writes what it reads back as compact JSON text.
    struct Writer;

    impl ProcessorJsonReader for Writer {
        type Read = String;

        fn null(&mut self) -> String {
            "null".to_owned()
        }

        fn boolean(&mut self, flag: bool) -> String {
            flag.to_string()
        }

        fn number(&mut self, number: NumberBuf) -> String {
            number.as_str().to_owned()
        }

        fn string(&mut self, text: &str) -> String {
            quoted(text)
        }

        fn array(&mut self, items: Vec<String>) -> String {
            format!("[{}]", items.join(","))
        }

        fn object(&mut self, members: Vec<(String, String)>) -> String {
            let members: Vec<String> = members.into_iter().map(|(key, value)| format!("{}:{value}", quoted(&key))).collect();
            format!("{{{}}}", members.join(","))
        }
    }

    #[test]
    fn a_processor_value_is_read_member_by_member_in_written_order() {
        let text = r#"{"b":[1,2.50,{"c":null}],"a":true,"s":"x\"y","e":{},"l":[]}"#;
        let value = parse_str_value(text).expect("JSON");
        pretty_assertions::assert_eq!(read_processor_json(value, &mut Writer), text);
    }

    #[test]
    fn a_deep_processor_value_is_read_without_recursing() {
        let depth = 100_000;
        let text = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let value = parse_str_value(&text).expect("JSON");
        let read = thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || read_processor_json(value, &mut Writer))
            .expect("the thread starts")
            .join()
            .expect("the read does not overflow its stack");
        assert_eq!(read, text);
    }

    #[test]
    fn a_processor_string_prints_as_the_json_string() {
        pretty_assertions::assert_eq!(processor_string("urn:ngsi-ld:null").compact_print().to_string(), r#""urn:ngsi-ld:null""#);
        assert!(JsonLiteral::from_processor_json(&processor_string("a")).is_ok());
    }
}
