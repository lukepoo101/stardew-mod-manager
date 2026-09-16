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

/// Formats a hash digest as a lowercase hexadecimal string.
///
/// `sha2` 0.11 returns `hybrid_array` values that no longer implement
/// `LowerHex`, so digests are rendered here instead of via `{:x}`.
pub fn hash_to_hex(digest: impl AsRef<[u8]>) -> String {
    use std::fmt::Write as _;

    let bytes = digest.as_ref();
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(hex, "{:02x}", byte);
    }
    hex
}

/// Generates a random v4 UUID string for callers that persist an opaque
/// identifier without a strong newtype.
pub fn uuid_v4() -> String {
    Uuid::new_v4().to_string()
}

/// Derives a stable UUID from an arbitrary seed, used to map pre-UUID string
/// identifiers onto the UUID identity space without losing row identity.
pub fn derive_uuid(seed: &str) -> Uuid {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(seed.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

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

    /// Accepts only a complete lowercase-normalised SHA-256 digest.
    pub fn parse(sha256_hex: impl Into<String>) -> Result<Self, InvalidArtifactHash> {
        let candidate = Self::new(sha256_hex);
        if candidate.is_well_formed() {
            Ok(candidate)
        } else {
            Err(InvalidArtifactHash(candidate.0))
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.0.len() == 64 && self.0.bytes().all(|b| b.is_ascii_hexdigit())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidArtifactHash(pub String);

impl fmt::Display for InvalidArtifactHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "'{}' is not a SHA-256 digest", self.0)
    }
}

impl std::error::Error for InvalidArtifactHash {}

impl fmt::Display for ArtifactHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ArtifactHash {
    type Err = InvalidArtifactHash;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
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
        let digest = "4b1d9a7c5f".repeat(6) + "abcd";
        let hash = ArtifactHash::new(digest.to_ascii_uppercase());
        assert_eq!(hash.as_str(), digest);
        assert_eq!(hash.to_string(), digest);
        assert!(hash.is_well_formed());
        assert!(ArtifactHash::parse("4b1d9a7c5f").is_err());
        assert!(ArtifactHash::parse(digest).is_ok());
    }

    #[test]
    fn test_mod_unique_id() {
        let id = ModUniqueId::new("Pathoschild.Automate");
        assert_eq!(id.as_str(), "Pathoschild.Automate");
        assert_eq!(id.to_string(), "Pathoschild.Automate");
    }
}
