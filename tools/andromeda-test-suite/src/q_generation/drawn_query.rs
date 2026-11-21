//! A drawn `q`, or a part of one, and what the request must carry beside it for the text to read as drawn.

use crate::q_generation::drawn_pool::NAMES;
use andromeda_q_corpus::corpus::Traversal;

/// A drawn `q` or a part of one.
#[derive(Debug, Clone)]
pub struct DrawnQuery {
    /// The text, writing the placeholders of the case for its identifiers and its type.
    pub text: String,
    /// The Attributes `jsonKeys` must name, in the order of [`NAMES`].
    pub json_keys: Vec<&'static str>,
    /// The Attributes `expandValues` must name, in the order of [`NAMES`].
    pub expand_values: Vec<&'static str>,
    /// Whether the text reads through a Relationship.
    pub traversal: Traversal,
}

impl DrawnQuery {
    /// The part `text`, which reads the Entity's own Attributes and needs no request member beside it.
    #[must_use]
    pub const fn local(text: String) -> DrawnQuery {
        DrawnQuery {
            text,
            json_keys: Vec::new(),
            expand_values: Vec::new(),
            traversal: Traversal::Local,
        }
    }

    /// `members` joined by `operator` in one group, needing what any member needs. A member that repeats an earlier
    /// one adds nothing to the group and is left out, and a group left with one member is that member.
    #[must_use]
    pub fn grouped(members: &[DrawnQuery], operator: &str) -> DrawnQuery {
        let mut texts: Vec<&str> = Vec::with_capacity(members.len());
        for member in members {
            if !texts.contains(&member.text.as_str()) {
                texts.push(member.text.as_str());
            }
        }
        let text = match texts.as_slice() {
            [single] => (*single).to_owned(),
            several => format!("({})", several.join(operator)),
        };
        let traversal = if members.iter().any(|member| member.traversal == Traversal::OneHop) {
            Traversal::OneHop
        } else {
            Traversal::Local
        };
        DrawnQuery {
            text,
            json_keys: union(members.iter().flat_map(|member| member.json_keys.iter().copied())),
            expand_values: union(members.iter().flat_map(|member| member.expand_values.iter().copied())),
            traversal,
        }
    }
}

/// Every name of [`NAMES`] that `names` holds, each once, in the order of [`NAMES`], so that two filters naming the
/// same Attributes compare equal.
pub fn union<'n>(names: impl IntoIterator<Item = &'n str>) -> Vec<&'static str> {
    let drawn: Vec<&str> = names.into_iter().collect();
    NAMES.into_iter().filter(|name| drawn.contains(name)).collect()
}
