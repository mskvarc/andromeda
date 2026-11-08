//! The place of a value in a compared document, written as a JSON path.

use std::fmt;

/// The place of a value in a compared document: the root, then each member or list item below it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentPath(Vec<PathStep>);

/// One step below a value.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PathStep {
    /// The member of an object with this name.
    Member(String),
    /// The item of a list whose `id` is this identifier.
    ItemWithId(String),
    /// Any item of a list.
    AnyItem,
}

impl DocumentPath {
    /// Whether the path names the compared root.
    #[must_use]
    pub const fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// The member `name` of the object at this path.
    pub(crate) fn member(&self, name: &str) -> DocumentPath {
        self.followed_by(PathStep::Member(name.to_owned()))
    }

    /// The item with the `id` `id` of the list at this path.
    pub(crate) fn item_with_id(&self, id: &str) -> DocumentPath {
        self.followed_by(PathStep::ItemWithId(id.to_owned()))
    }

    /// Any item of the list at this path.
    pub(crate) fn any_item(&self) -> DocumentPath {
        self.followed_by(PathStep::AnyItem)
    }

    /// This path with `step` after its last step.
    fn followed_by(&self, step: PathStep) -> DocumentPath {
        // clone: this path is borrowed, so the new path takes its own copy of the steps
        let mut steps = self.0.clone();
        steps.push(step);
        DocumentPath(steps)
    }
}

// Hand-written: the steps are written in JSON path syntax.
impl fmt::Display for DocumentPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("$")?;
        self.0.iter().try_for_each(|step| match step {
            PathStep::Member(name) => write!(formatter, "['{name}']"),
            PathStep::ItemWithId(id) => write!(formatter, "[?(@.id=='{id}')]"),
            PathStep::AnyItem => formatter.write_str("[*]"),
        })
    }
}
