//! The file names a snapshot name written in source can stand for.

/// A snapshot name as the source writes it: a string literal, or the format
/// string of a `format!` whose placeholders stand for any text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct NamePattern {
    /// The literal text between the placeholders, one more segment than there
    /// are placeholders. A name without a placeholder is one segment.
    segments: Vec<String>,
}

impl NamePattern {
    /// The pattern of a string literal or a format string, `{{` and `}}`
    /// standing for the braces themselves.
    pub(super) fn parse(format: &str) -> Self {
        let mut segments = vec![String::new()];
        let mut characters = format.chars().peekable();
        while let Some(character) = characters.next() {
            match character {
                '{' if characters.peek() == Some(&'{') => {
                    characters.next();
                    push_character(&mut segments, '{');
                }
                '}' if characters.peek() == Some(&'}') => {
                    characters.next();
                    push_character(&mut segments, '}');
                }
                '{' => {
                    for inside in characters.by_ref() {
                        if inside == '}' {
                            break;
                        }
                    }
                    segments.push(String::new());
                }
                other => push_character(&mut segments, other),
            }
        }
        Self { segments }
    }

    /// Whether `name` is a text this pattern can produce.
    pub(super) fn matches(&self, name: &str) -> bool {
        let [first, rest @ ..] = self.segments.as_slice() else {
            return false;
        };
        let Some((last, middle)) = rest.split_last() else {
            return name == first;
        };
        let Some(mut remainder) = name.strip_prefix(first.as_str()) else {
            return false;
        };
        for segment in middle {
            let Some(position) = remainder.find(segment.as_str()) else {
                return false;
            };
            remainder = &remainder[position + segment.len()..];
        }
        remainder.ends_with(last.as_str())
    }
}

/// Append `character` to the last segment.
fn push_character(segments: &mut [String], character: char) {
    if let Some(last) = segments.last_mut() {
        last.push(character);
    }
}
