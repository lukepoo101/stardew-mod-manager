pub mod model;
pub mod policy;

pub use model::{
    ManagedSmapiInstallation, SmapiObservation, SmapiPlatformPolicy, SmapiReleaseInfo,
    SmapiReleasePolicy,
};
pub use policy::{
    default_release_policy, get_pinned_smapi_release, PINNED_INSTALLER_INTERNAL_PATH,
    PINNED_SMAPI_COMMIT, PINNED_SMAPI_GAME_VERSION, PINNED_SMAPI_SHA256, PINNED_SMAPI_TAG,
    PINNED_SMAPI_URL, PINNED_SMAPI_VERSION, SMAPI_EXECUTABLE_NAME, SMAPI_LAUNCHER_SCRIPT_NAME,
};
