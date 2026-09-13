use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmapiVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub build: u32,
    pub prerelease: Option<String>,
}

impl SmapiVersion {
    pub fn parse(s: &str) -> Result<Self, String> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err("Version string cannot be empty".to_string());
        }

        let (core, prerelease) = match trimmed.split_once('-') {
            Some((c, p)) => (c, Some(p.to_string())),
            None => (trimmed, None),
        };

        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 4 {
            return Err(format!("Invalid version segment count in '{}'", s));
        }

        let major = parts[0]
            .parse::<u32>()
            .map_err(|_| format!("Invalid major version '{}'", parts[0]))?;
        let minor = if parts.len() > 1 {
            parts[1]
                .parse::<u32>()
                .map_err(|_| format!("Invalid minor version '{}'", parts[1]))?
        } else {
            0
        };
        let patch = if parts.len() > 2 {
            parts[2]
                .parse::<u32>()
                .map_err(|_| format!("Invalid patch version '{}'", parts[2]))?
        } else {
            0
        };
        let build = if parts.len() > 3 {
            parts[3]
                .parse::<u32>()
                .map_err(|_| format!("Invalid build version '{}'", parts[3]))?
        } else {
            0
        };

        Ok(Self {
            major,
            minor,
            patch,
            build,
            prerelease,
        })
    }

    pub fn is_at_least(&self, min: &SmapiVersion) -> bool {
        self >= min
    }
}

impl PartialOrd for SmapiVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SmapiVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.major.cmp(&other.major) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.minor.cmp(&other.minor) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.patch.cmp(&other.patch) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.build.cmp(&other.build) {
            Ordering::Equal => {}
            ord => return ord,
        }

        // Release is newer than prerelease with same numeric version
        match (&self.prerelease, &other.prerelease) {
            (None, None) => Ordering::Equal,
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(b),
        }
    }
}

impl fmt::Display for SmapiVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.build > 0 {
            write!(
                f,
                "{}.{}.{}.{}",
                self.major, self.minor, self.patch, self.build
            )?;
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        }
        if let Some(ref pre) = self.prerelease {
            write!(f, "-{}", pre)?;
        }
        Ok(())
    }
}

impl FromStr for SmapiVersion {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing_and_ordering() {
        let v1 = SmapiVersion::parse("1.0").unwrap();
        let v2 = SmapiVersion::parse("1.0.0").unwrap();
        assert_eq!(v1, v2);

        let v3 = SmapiVersion::parse("1.2.3").unwrap();
        let v4 = SmapiVersion::parse("1.2.4").unwrap();
        assert!(v4 > v3);

        let v4_pre = SmapiVersion::parse("1.2.4-beta").unwrap();
        assert!(v4 > v4_pre);
        assert!(v4_pre > v3);

        let v_build = SmapiVersion::parse("1.6.9.1").unwrap();
        let v_nobuild = SmapiVersion::parse("1.6.9").unwrap();
        assert!(v_build > v_nobuild);
    }
}
