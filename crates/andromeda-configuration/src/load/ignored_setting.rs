//! The settings a configuration layer writes that the configuration does not
//! define.
//!
//! A file or an environment written for a later version can carry keys this
//! version does not read. Each one is ignored and reported, so the file still
//! loads and a misspelt key is still seen. A key the configuration defines,
//! holding a value it does not define, is refused instead: the broker cannot
//! tell what that value means.

use crate::{broker_config::BrokerConfig, load::ENVIRONMENT_PREFIX};
use derive_more::Display;
use figment2::{Error as LayerError, Figment, value::Value};
use serde_ignored::Path as IgnoredPath;
use std::{
    fmt,
    path::{Path, PathBuf},
};
use strum::IntoStaticStr;

/// One step from the root of the configuration towards a setting.
#[derive(Debug, Clone, PartialEq, Eq)]
enum KeySegment {
    /// A key of a table, as the layer spells it.
    Key(String),
    /// A position in an array of tables, counted from zero.
    Index(usize),
}

/// Where a setting sits in the configuration, from its root: its keys joined
/// by `.`, and an entry of an array of tables as `[index]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingPath(Vec<KeySegment>);

impl SettingPath {
    /// The environment variable that sets this path.
    fn environment_variable(&self) -> EnvironmentVariable {
        let levels = self
            .0
            .iter()
            .map(|segment| match segment {
                KeySegment::Key(key) => key.to_ascii_uppercase(),
                KeySegment::Index(index) => index.to_string(),
            })
            .collect::<Vec<_>>();
        EnvironmentVariable(format!("{ENVIRONMENT_PREFIX}{}", levels.join("__")))
    }

    /// Whether `root` holds a value at this path.
    fn is_held_by(&self, root: &Value) -> bool {
        self.0
            .iter()
            .try_fold(root, |value, segment| match (value, segment) {
                (Value::Dict(_, table), KeySegment::Key(key)) => table.get(key),
                (Value::Array(_, items), KeySegment::Index(index)) => items.get(*index),
                (Value::Dict(..), KeySegment::Index(_))
                | (Value::Array(..), KeySegment::Key(_))
                | (Value::String(..) | Value::Char(..) | Value::Bool(..) | Value::Num(..) | Value::Empty(..), _) => None,
            })
            .is_some()
    }
}

impl From<IgnoredPath<'_>> for SettingPath {
    fn from(path: IgnoredPath<'_>) -> SettingPath {
        let mut segments = Vec::new();
        push_segments(&path, &mut segments);
        SettingPath(segments)
    }
}

/// Append the steps from the root to `path` to `segments`, outermost first.
/// Option and newtype wrappers add no step: they hold the value they wrap.
fn push_segments(path: &IgnoredPath<'_>, segments: &mut Vec<KeySegment>) {
    match path {
        IgnoredPath::Root => {}
        IgnoredPath::Seq { parent, index } => {
            push_segments(parent, segments);
            segments.push(KeySegment::Index(*index));
        }
        IgnoredPath::Map { parent, key } => {
            push_segments(parent, segments);
            // clone: the parents are borrowed from the deserializer's stack.
            segments.push(KeySegment::Key(key.clone()));
        }
        IgnoredPath::Some { parent } | IgnoredPath::NewtypeStruct { parent } | IgnoredPath::NewtypeVariant { parent } => push_segments(parent, segments),
    }
}

// Display joins a variable number of segments, which no derive expresses.
impl fmt::Display for SettingPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().enumerate().try_for_each(|(position, segment)| match segment {
            KeySegment::Key(key) if position == 0 => formatter.write_str(key),
            KeySegment::Key(key) => write!(formatter, ".{key}"),
            KeySegment::Index(index) => write!(formatter, "[{index}]"),
        })
    }
}

/// The name of an environment variable the configuration reads.
#[derive(Debug, Clone, PartialEq, Eq, Display)]
pub struct EnvironmentVariable(String);

/// A setting a layer writes that the configuration does not define, so the
/// broker ignores it.
#[derive(Debug, Clone, PartialEq, Eq, Display, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum IgnoredSetting {
    /// The configuration file writes the setting.
    #[display("`{path}` in {} is not a setting this broker reads, so it is ignored", file.display())]
    File {
        /// The configuration file.
        file: PathBuf,
        /// Where the setting sits.
        path: SettingPath,
    },
    /// An `ANDROMEDA__` environment variable writes the setting.
    #[display("{variable} sets `{path}`, which is not a setting this broker reads, so it is ignored")]
    Environment {
        /// The variable.
        variable: EnvironmentVariable,
        /// Where the setting sits.
        path: SettingPath,
    },
}

impl IgnoredSetting {
    /// Where the ignored setting sits.
    #[must_use]
    pub const fn path(&self) -> &SettingPath {
        match self {
            IgnoredSetting::File { path, .. } | IgnoredSetting::Environment { path, .. } => path,
        }
    }
}

/// The layers that can write a key the configuration does not define: the
/// defaults and the listener overrides write defined keys alone.
pub(crate) struct WritingLayers<'a> {
    /// The configuration file, and the layer it contributes.
    pub(crate) file: Option<(&'a Path, Figment)>,
    /// The `ANDROMEDA__` environment layer.
    pub(crate) environment: Figment,
}

/// Every setting the layers merged into `merged` write that the configuration
/// does not define, once for each layer that writes it.
///
/// The strict extraction has already accepted `merged`; this pass reads it a
/// second time through a deserializer that reports what it skips, because
/// the extraction's own deserializer cannot be wrapped.
///
/// # Errors
///
/// Returns the layer error when a layer cannot be read.
pub(crate) fn ignored_settings(merged: &Figment, layers: &WritingLayers<'_>) -> Result<Vec<IgnoredSetting>, Box<LayerError>> {
    let mut paths = Vec::new();
    serde_ignored::deserialize::<_, _, BrokerConfig>(&root_of(merged)?, |path| paths.push(SettingPath::from(path))).map_err(Box::new)?;
    let file = layers.file.as_ref().map(|(file, layer)| root_of(layer).map(|root| (*file, root))).transpose()?;
    let environment = root_of(&layers.environment)?;
    Ok(paths
        .into_iter()
        .flat_map(|path| {
            let in_file = file.as_ref().filter(|(_, root)| path.is_held_by(root)).map(|(file, _)| IgnoredSetting::File {
                file: file.to_path_buf(),
                // clone: both layers may write the same ignored key, and each report owns its path.
                path: path.clone(),
            });
            let in_environment = path.is_held_by(&environment).then(|| IgnoredSetting::Environment {
                variable: path.environment_variable(),
                path,
            });
            in_file.into_iter().chain(in_environment)
        })
        .collect())
}

/// The whole tree `layer` holds.
fn root_of(layer: &Figment) -> Result<Value, Box<LayerError>> {
    layer.find_value("").map_err(Box::new)
}
