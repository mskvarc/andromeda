//! Applying and reverting the ETSI patch sets. The endpoint patch corrects a
//! suite-infrastructure fault and is always applied; the corrected-material set
//! adjudicates expectation defects and is applied unless the run asks for the
//! material as-shipped. A guard reverts everything when the run ends, so an
//! inspected checkout is left pristine.

use crate::{error::XtaskError, etsi};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Whether the corrected-material patch set is applied on top of the
/// always-applied endpoint patch.
#[derive(Clone, Copy)]
pub(crate) enum Material {
    /// Apply the corrected-material patches: the default, spec-adjudicated run.
    Corrected,
    /// Skip them, judging the broker against the material exactly as it ships.
    AsShipped,
}

/// Direction a unified diff is applied in.
#[derive(Clone, Copy)]
enum Direction {
    /// Apply the patch.
    Forward,
    /// Reverse a previously applied patch.
    Reverse,
}

/// Applies the selected patches on construction and reverts them, in reverse order,
/// on drop.
pub(crate) struct PatchGuard {
    /// Patch files applied to the checkout, in application order.
    applied: Vec<PathBuf>,
}

impl PatchGuard {
    /// Apply the endpoint patch and, unless the run is as-shipped, the
    /// corrected-material set, recording each for reversal on drop.
    ///
    /// # Errors
    ///
    /// Returns [`XtaskError::PatchApply`] for the first patch that does not apply
    /// cleanly, and an I/O error if the patch directory cannot be read.
    pub(crate) fn apply(material: Material) -> Result<PatchGuard, XtaskError> {
        let mut guard = PatchGuard { applied: Vec::new() };
        guard.apply_one(etsi::resource_dir().join("endpoint.patch"))?;
        match material {
            Material::Corrected => {
                for patch in material_patches()? {
                    guard.apply_one(patch)?;
                }
            }
            Material::AsShipped => {}
        }
        Ok(guard)
    }

    /// Apply one patch to the checkout, recording it once it applies cleanly.
    fn apply_one(&mut self, patch: PathBuf) -> Result<(), XtaskError> {
        if git_apply(&patch, Direction::Forward)? {
            self.applied.push(patch);
            Ok(())
        } else {
            Err(XtaskError::PatchApply(patch.display().to_string()))
        }
    }
}

impl Drop for PatchGuard {
    fn drop(&mut self) {
        for patch in self.applied.iter().rev() {
            // Best-effort: the next run hard-resets the checkout regardless, so a
            // failed revert here cannot leak into a subsequent run.
            let _reverted = git_apply(patch, Direction::Reverse);
        }
    }
}

/// The corrected-material patch files, sorted by file name so their application
/// order is deterministic across platforms.
fn material_patches() -> Result<Vec<PathBuf>, XtaskError> {
    let dir = etsi::resource_dir().join("patches");
    let mut patches: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "patch"))
        .collect();
    patches.sort();
    Ok(patches)
}

/// Apply or reverse one unified diff against the checkout with `git apply`,
/// returning whether it applied cleanly. Both plain and `diff --git` unified diffs
/// are stripped at one leading path component, matching how the patches are
/// authored, and `git apply` leaves the tree untouched when a patch does not apply.
fn git_apply(patch: &Path, direction: Direction) -> Result<bool, XtaskError> {
    let mut command = Command::new("git");
    command.arg("-C").arg(etsi::checkout_dir()).args(["apply", "-p1"]);
    match direction {
        Direction::Forward => {}
        Direction::Reverse => {
            command.arg("--reverse");
        }
    }
    let status = command.arg(patch).status()?;
    Ok(status.success())
}

#[cfg(test)]
mod tests {
    use crate::{etsi, etsi::patch::material_patches};

    #[test]
    fn the_endpoint_patch_ships_beside_the_material_patch_directory() {
        assert!(etsi::resource_dir().join("endpoint.patch").is_file());
    }

    #[test]
    fn every_material_patch_is_a_dot_patch_file_in_sorted_order() {
        let patches = material_patches().expect("the patch directory is readable");
        // The crate ships exactly the corrected-material set the matrix records.
        assert_eq!(patches.len(), 22);
        assert!(patches.iter().all(|path| path.extension().is_some_and(|extension| extension == "patch")));
        assert!(patches.is_sorted());
    }
}
