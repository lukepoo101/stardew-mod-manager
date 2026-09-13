use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

macro_rules! define_uuid_id {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        #[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
        #[cfg_attr(feature = "ts-export", ts(export, type = "string"))]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub fn from_uuid(uuid: Uuid) -> Self {
                Self(uuid)
            }

            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }
    };
}

define_uuid_id!(
    GameInstallationId,
    "Strong identifier for a game installation."
);
define_uuid_id!(ProfileId, "Strong identifier for a modding profile.");
define_uuid_id!(
    AcquisitionId,
    "Strong identifier for a package acquisition event."
);
define_uuid_id!(
    PackageComponentId,
    "Strong identifier for a mod component inside a package."
);
define_uuid_id!(DeploymentId, "Strong identifier for a profile deployment.");
define_uuid_id!(
    ProfileComponentId,
    "Strong identifier for a component participating in a profile."
);
define_uuid_id!(OperationId, "Strong identifier for an operation.");
define_uuid_id!(LaunchSessionId, "Strong identifier for a launch session.");
define_uuid_id!(
    FindingId,
    "Strong identifier for a health or diagnostic finding."
);

/// Canonical content-addressed SHA-256 identity for immutable `PackageArtifact`s.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, type = "string"))]
pub struct ArtifactHash(pub String);

impl ArtifactHash {
    pub fn new(sha256_hex: impl Into<String>) -> Self {
        Self(sha256_hex.into().to_ascii_lowercase())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ArtifactHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ArtifactHash {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

impl From<&str> for ArtifactHash {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ArtifactHash {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

/// Official SMAPI UniqueID (e.g. `Pathoschild.Automate`).
///
/// This is strictly distinct from internal manager IDs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "ts-export", derive(ts_rs::TS))]
#[cfg_attr(feature = "ts-export", ts(export, type = "string"))]
pub struct ModUniqueId(pub String);

impl ModUniqueId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ModUniqueId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ModUniqueId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

impl From<&str> for ModUniqueId {
    fn from(s: &str) -> Self {
        Self::new(s)
    }
}

impl From<String> for ModUniqueId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid_ids_generation_and_serialization() {
        let profile_id = ProfileId::new();
        let str_rep = profile_id.to_string();
        let parsed: ProfileId = str_rep.parse().expect("valid uuid parse");
        assert_eq!(profile_id, parsed);

        let json = serde_json::to_string(&profile_id).unwrap();
        assert_eq!(json, format!("\"{}\"", str_rep));
        let deserialized: ProfileId = serde_json::from_str(&json).unwrap();
        assert_eq!(profile_id, deserialized);
    }

    #[test]
    fn test_artifact_hash_normalization() {
        let hash = ArtifactHash::new("4B1D9A7C5F");
        assert_eq!(hash.as_str(), "4b1d9a7c5f");
        assert_eq!(hash.to_string(), "4b1d9a7c5f");
    }

    #[test]
    fn test_mod_unique_id() {
        let id = ModUniqueId::new("Pathoschild.Automate");
        assert_eq!(id.as_str(), "Pathoschild.Automate");
        assert_eq!(id.to_string(), "Pathoschild.Automate");
    }
}
