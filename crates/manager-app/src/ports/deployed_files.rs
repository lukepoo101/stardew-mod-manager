use crate::error::AppResult;
use manager_core::ids::ProfileId;

/// One file in a deployed mod folder, relative to that folder with `/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployedFile {
    pub relative_path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

/// Reads the files the manager deployed for a mod, wherever the mod currently
/// is (live or disabled). Read only.
pub trait DeployedFilesPort: Send + Sync {
    /// None when the folder is not where the manager put it.
    fn read_folder(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Option<Vec<DeployedFile>>>;

    /// Like `read_folder`, but only lists paths and sizes; `sha256` is empty.
    /// Cheap enough to run without being asked.
    fn read_folder_sizes(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Option<Vec<DeployedFile>>> {
        self.read_folder(profile_id, root_relative_path)
            .map(|files| {
                files.map(|files| {
                    files
                        .into_iter()
                        .map(|f| DeployedFile {
                            sha256: String::new(),
                            ..f
                        })
                        .collect()
                })
            })
    }

    /// The contents of every `config.json` in the folder, by relative path.
    fn read_configs(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
    ) -> AppResult<Vec<(String, Vec<u8>)>>;

    /// The contents of every `config.json` in the copy of a folder that a
    /// removal moved into the profile's recovery folder, by relative path.
    /// None when that copy is gone (for example cleaned up since).
    fn read_removed_configs(
        &self,
        _profile_id: &ProfileId,
        _removal: &manager_core::ids::OperationId,
        _root_relative_path: &str,
    ) -> AppResult<Option<Vec<(String, Vec<u8>)>>> {
        Ok(None)
    }

    /// Writes files back into the folder at their relative paths.
    fn write_files(
        &self,
        profile_id: &ProfileId,
        root_relative_path: &str,
        files: &[(String, Vec<u8>)],
    ) -> AppResult<()>;
}
