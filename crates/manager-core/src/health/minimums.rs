//! Mods' declared minimum SMAPI or game versions against what is installed.

use crate::version::SmapiVersion;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MinimumCheck {
    /// Mods whose minimum is newer than what is installed: (name, minimum).
    pub too_old_for: Vec<(String, String)>,
    /// The highest minimum among them.
    pub strongest: Option<String>,
    /// Mods whose minimum could not be compared (unreadable versions, or the
    /// installed version is unknown).
    pub unassessed: Vec<String>,
}

/// `mods` are (name, declared minimum). `installed` is the version in place.
pub fn check_minimums(mods: &[(String, String)], installed: Option<&str>) -> MinimumCheck {
    let mut check = MinimumCheck::default();
    let have = installed.and_then(|v| SmapiVersion::parse(v).ok());
    let mut strongest: Option<(SmapiVersion, String)> = None;
    for (name, minimum) in mods {
        match (SmapiVersion::parse(minimum), &have) {
            (Ok(need), Some(have)) => {
                if &need > have {
                    check.too_old_for.push((name.clone(), minimum.clone()));
                    if strongest.as_ref().is_none_or(|(best, _)| need > *best) {
                        strongest = Some((need, minimum.clone()));
                    }
                }
            }
            _ => check.unassessed.push(name.clone()),
        }
    }
    check.too_old_for.sort();
    check.unassessed.sort();
    check.strongest = strongest.map(|(_, text)| text);
    check
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mods(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn names_mods_that_need_newer_and_the_strongest_requirement() {
        let check = check_minimums(
            &mods(&[
                ("A", "4.0.0"),
                ("B", "4.2.0"),
                ("C", "4.3.1"),
                ("D", "not-a-version"),
            ]),
            Some("4.1.10"),
        );
        assert_eq!(
            check.too_old_for,
            vec![("B".into(), "4.2.0".into()), ("C".into(), "4.3.1".into())]
        );
        assert_eq!(check.strongest.as_deref(), Some("4.3.1"));
        assert_eq!(check.unassessed, vec!["D".to_string()]);
    }

    #[test]
    fn an_unknown_installed_version_assesses_nothing_as_fine() {
        let check = check_minimums(&mods(&[("A", "4.0.0")]), None);
        assert!(check.too_old_for.is_empty());
        assert_eq!(check.unassessed, vec!["A".to_string()]);
    }
}
