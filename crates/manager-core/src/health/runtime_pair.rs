//! Whether the installed SMAPI and game versions are a pair this manager was
//! tested with. "Tested" is a statement about this manager's testing, not a
//! guarantee, and an unknown version is never reported as tested.

use crate::version::SmapiVersion;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimePair {
    /// The SMAPI this manager pins, on a game at least as new as it needs.
    Tested,
    /// The game is older than the minimum the tested SMAPI supports.
    GameTooOld { minimum: String },
    /// A SMAPI other than the tested one; not known to be broken.
    Untested,
    /// A version could not be read or compared.
    Unknown,
}

/// `tested_game` is the policy's game requirement, such as "1.6.9+".
pub fn assess_runtime_pair(
    smapi: &str,
    game: Option<&str>,
    tested_smapi: &str,
    tested_game: &str,
) -> RuntimePair {
    let minimum = tested_game.trim_end_matches('+').trim();
    let (Some(game), Ok(min), Ok(have_smapi), Ok(want_smapi)) = (
        game,
        SmapiVersion::parse(minimum),
        SmapiVersion::parse(smapi),
        SmapiVersion::parse(tested_smapi),
    ) else {
        return RuntimePair::Unknown;
    };
    let Ok(have_game) = SmapiVersion::parse(game) else {
        return RuntimePair::Unknown;
    };
    if have_smapi != want_smapi {
        return RuntimePair::Untested;
    }
    if have_game < min {
        return RuntimePair::GameTooOld {
            minimum: minimum.to_string(),
        };
    }
    RuntimePair::Tested
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_are_told_apart_and_unknowns_are_never_tested() {
        let t = |smapi, game| assess_runtime_pair(smapi, game, "4.1.10", "1.6.9+");
        assert_eq!(t("4.1.10", Some("1.6.15")), RuntimePair::Tested);
        assert_eq!(
            t("4.1.10", Some("1.6.8")),
            RuntimePair::GameTooOld {
                minimum: "1.6.9".to_string()
            }
        );
        assert_eq!(t("4.2.0", Some("1.6.15")), RuntimePair::Untested);
        assert_eq!(t("4.1.10", None), RuntimePair::Unknown);
        assert_eq!(t("4.1.10", Some("weird")), RuntimePair::Unknown);
    }
}
