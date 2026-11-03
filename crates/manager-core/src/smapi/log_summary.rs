//! A read-only summary of a SMAPI log.
//!
//! The parser is deliberately conservative: it recognises the shapes SMAPI
//! actually prints and reports only what it matched, with the line number, so a
//! reader can jump to the evidence. A log it cannot interpret yields an empty
//! summary, never a claim that the session was healthy.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedMod {
    pub name: String,
    pub version: Option<String>,
    pub reason: String,
    /// Mod UniqueIDs the reason says are missing, when it says so.
    pub missing_dependencies: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModUpdateNotice {
    pub name: String,
    pub current_version: String,
    pub available_version: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceCount {
    /// The mod or component that wrote the lines (`SMAPI` for SMAPI itself).
    pub source: String,
    pub errors: usize,
    pub warnings: usize,
    /// Line of the first error from this source.
    pub first_error_line: Option<usize>,
}

/// One ERROR line, with a rough kind read from its wording.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogError {
    pub line: usize,
    pub source: String,
    /// "missing_dependency", "content_pack", "patch", "exception", "file" or
    /// "other". A guess from wording, never a diagnosis.
    pub kind: String,
    pub message: String,
}

/// How many error lines are kept; the counts per source cover all of them.
pub const MAX_LOG_ERRORS: usize = 200;

/// A rough kind for an error message, from words SMAPI and mods commonly use.
pub fn classify_error(message: &str) -> &'static str {
    let lower = message.to_lowercase();
    if lower.contains("needs mod") || lower.contains("missing dependenc") {
        "missing_dependency"
    } else if lower.contains("harmony")
        || lower.contains("patch failed")
        || lower.contains("failed to patch")
    {
        "patch"
    } else if lower.contains("content pack") || lower.contains("contentpack") {
        "content_pack"
    } else if lower.contains("could not find file")
        || lower.contains("file not found")
        || lower.contains("filenotfound")
        || lower.contains("access to the path")
    {
        "file"
    } else if lower.contains("exception") {
        "exception"
    } else {
        "other"
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSummary {
    pub smapi_version: Option<String>,
    pub game_version: Option<String>,
    pub loaded_mod_count: Option<usize>,
    pub skipped_mods: Vec<SkippedMod>,
    pub update_notices: Vec<ModUpdateNotice>,
    pub sources: Vec<SourceCount>,
    pub total_lines: usize,
    /// The first `MAX_LOG_ERRORS` error lines, each with a rough kind.
    #[serde(default)]
    pub errors: Vec<LogError>,
}

struct Line<'a> {
    level: &'a str,
    source: &'a str,
    message: &'a str,
}

fn parse_line(raw: &str) -> Option<Line<'_>> {
    let trimmed = raw.trim_start();
    let rest = trimmed.strip_prefix('[')?;
    let close = rest.find(']')?;
    let inside = &rest[..close];
    let mut parts = inside.splitn(3, ' ');
    let time = parts.next()?;
    let bytes = time.as_bytes();
    if bytes.len() != 8 || bytes[2] != b':' || bytes[5] != b':' {
        return None;
    }
    let level = parts.next()?.trim();
    let source = parts.next().unwrap_or("").trim();
    let message = &rest[close + 1..];
    Some(Line {
        level,
        source,
        message: message.strip_prefix(' ').unwrap_or(message),
    })
}

fn split_name_version(text: &str) -> (String, Option<String>) {
    let text = text.trim();
    if let Some((name, last)) = text.rsplit_once(' ') {
        if last.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            return (name.trim().to_string(), Some(last.to_string()));
        }
    }
    (text.to_string(), None)
}

/// Extracts `Some.Id` tokens that follow the words "needs mod".
fn missing_dependencies(reason: &str) -> Vec<String> {
    let lower = reason.to_lowercase();
    let mut found = Vec::new();
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find("needs mod ") {
        let start = search_from + offset + "needs mod ".len();
        let token: String = reason[start..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-'))
            .collect();
        if !token.is_empty() {
            found.push(token);
        }
        search_from = start;
    }
    // SMAPI 4: "it requires mods which aren't installed (Name: url, Some.Id)."
    // Each entry is the mod's name when SMAPI knows it, else its ID.
    if let Some(at) = lower.find("requires mods which aren't installed (") {
        let start = at + "requires mods which aren't installed (".len();
        if let Some(end) = reason[start..].rfind(')') {
            for entry in reason[start..start + end].split(", ") {
                let entry = entry.split(": http").next().unwrap_or(entry).trim();
                if !entry.is_empty() {
                    found.push(entry.to_string());
                }
            }
        }
    }
    found
}

/// One mod line under SMAPI's "You can update N mods:" header, as
/// (name, current version, available version).
///
/// SMAPI 4 prints `Name 1.1.0: https://... (you have 1.0.0)`. Older logs
/// and some tools write `Name 1.0 → 1.1: https://...`, which is read too.
fn parse_update_notice(content: &str) -> Option<(String, String, String)> {
    let before_url = |text: &'_ str| -> String {
        text.split_once(": http")
            .map(|(head, _)| head)
            .or_else(|| text.rsplit_once(": ").map(|(head, _)| head))
            .unwrap_or(text)
            .trim()
            .to_string()
    };

    if let Some((head, current)) = content
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" (you have "))
    {
        let (name, available) = split_name_version(&before_url(head));
        let current = current.trim();
        return match available {
            Some(available) if !current.is_empty() => Some((name, current.to_string(), available)),
            _ => None,
        };
    }

    let text = before_url(content);
    let (left, right) = text
        .split_once('\u{2192}')
        .or_else(|| text.split_once("->"))?;
    let (name, current) = split_name_version(left);
    let available = right.trim().trim_end_matches(':').trim().to_string();
    match current {
        Some(current) if !available.is_empty() => Some((name, current, available)),
        _ => None,
    }
}

pub fn summarize_log(log: &str) -> LogSummary {
    let mut summary = LogSummary::default();
    let mut counts: BTreeMap<String, SourceCount> = BTreeMap::new();

    #[derive(PartialEq)]
    enum Section {
        None,
        Skipped,
        Updates,
    }
    let mut section = Section::None;

    for (index, raw) in log.lines().enumerate() {
        let number = index + 1;
        summary.total_lines = number;
        let Some(line) = parse_line(raw) else {
            continue;
        };
        let message = line.message;
        let indented = message.starts_with("   ") || message.starts_with('\t');
        let content = message.trim();

        // A new, unindented SMAPI message ends whatever section was open.
        if !indented && !content.is_empty() {
            section = Section::None;
        }

        if line.source.eq_ignore_ascii_case("SMAPI") {
            if let Some(rest) = content.strip_prefix("SMAPI ") {
                if let Some((smapi, game)) = rest.split_once(" with Stardew Valley ") {
                    summary.smapi_version = Some(smapi.trim().to_string());
                    let version = game.split_whitespace().next().unwrap_or("").to_string();
                    if !version.is_empty() {
                        summary.game_version = Some(version);
                    }
                }
            }
            if let Some(rest) = content.strip_prefix("Loaded ") {
                if let Some((count, _)) = rest.split_once(" mods") {
                    if let Ok(count) = count.trim().parse::<usize>() {
                        summary.loaded_mod_count = Some(count);
                    }
                }
            }
            let header = content.trim_end_matches(':').trim();
            if header.eq_ignore_ascii_case("Skipped mods") {
                section = Section::Skipped;
                continue;
            }
            if content.starts_with("You can update ") && content.ends_with(':') {
                section = Section::Updates;
                continue;
            }
        }

        if indented {
            match section {
                Section::Skipped => {
                    // SMAPI 4 writes the block as a heading, a ruler, one
                    // sentence and then ERROR entries `      - Name 1.0.0 because
                    // ...`, each optionally followed by a TRACE detail line
                    // `        (...)`. Warning groups such as "Patched game code"
                    // follow with the same indentation, so anything else ends
                    // the block rather than being read as a skipped mod.
                    if content.chars().all(|c| c == '-')
                        || content.starts_with("These mods could not")
                    {
                        continue;
                    }
                    if line.level.eq_ignore_ascii_case("TRACE") && content.starts_with('(') {
                        continue;
                    }
                    if let Some(entry) = content
                        .strip_prefix("- ")
                        .filter(|_| line.level.eq_ignore_ascii_case("ERROR"))
                    {
                        let entry = entry.trim();
                        let (subject, reason) = match entry.split_once(" because ") {
                            Some((subject, reason)) => (subject, reason.trim()),
                            None => (entry, ""),
                        };
                        // With LogTechnicalDetailsForBrokenMods on, SMAPI adds
                        // " (ID: X, path: Y)" after the version.
                        let subject = match subject.rfind(" (ID: ") {
                            Some(at) if subject.ends_with(')') => &subject[..at],
                            _ => subject,
                        };
                        let (name, version) = split_name_version(subject);
                        summary.skipped_mods.push(SkippedMod {
                            name,
                            version,
                            missing_dependencies: missing_dependencies(reason),
                            reason: reason.to_string(),
                            line: number,
                        });
                        continue;
                    }
                    section = Section::None;
                }
                Section::Updates => {
                    if let Some((name, current, available)) = parse_update_notice(content) {
                        summary.update_notices.push(ModUpdateNotice {
                            name,
                            current_version: current,
                            available_version: available,
                            line: number,
                        });
                    }
                    continue;
                }
                Section::None => {}
            }
        }

        // SMAPI writes blank lines at the level of the block around them (the
        // skipped-mods block is ERROR), so an empty line is spacing, not an
        // error or warning.
        if content.is_empty() {
            continue;
        }

        let is_error = line.level.eq_ignore_ascii_case("ERROR");
        let is_warning = line.level.eq_ignore_ascii_case("WARN");
        if (is_error || is_warning) && !line.source.is_empty() {
            let entry = counts
                .entry(line.source.to_string())
                .or_insert_with(|| SourceCount {
                    source: line.source.to_string(),
                    errors: 0,
                    warnings: 0,
                    first_error_line: None,
                });
            if is_error {
                entry.errors += 1;
                entry.first_error_line.get_or_insert(number);
                if summary.errors.len() < MAX_LOG_ERRORS {
                    summary.errors.push(LogError {
                        line: number,
                        source: line.source.to_string(),
                        kind: classify_error(content).to_string(),
                        message: content.to_string(),
                    });
                }
            } else {
                entry.warnings += 1;
            }
        }
    }

    summary.sources = counts.into_values().collect();
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_lines_are_kept_with_a_rough_kind() {
        let log = "\
[10:00:00 ERROR Content Patcher] Content pack 'X' failed: could not find file 'a.png'
[10:00:01 ERROR Some Mod] Failed to patch method Foo: Harmony error
[10:00:02 ERROR Other] System.NullReferenceException: boom
[10:00:03 WARN  Other] just a warning
";
        let summary = summarize_log(log);
        let kinds: Vec<_> = summary.errors.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, vec!["content_pack", "patch", "exception"]);
        assert_eq!(summary.errors[0].line, 1);
        assert_eq!(summary.errors[1].source, "Some Mod");
    }

    const SAMPLE: &str = "\
[10:00:00 INFO  SMAPI] SMAPI 4.1.10 with Stardew Valley 1.6.15 on Linux
[10:00:00 INFO  SMAPI] Mods go here: /home/x/Mods
[10:00:01 INFO  SMAPI] Loaded 12 mods:
[10:00:01 INFO  SMAPI]    Good Mod 1.0.0 by A | desc
[10:00:02 ERROR SMAPI] Skipped mods
[10:00:02 ERROR SMAPI]    ----------------------------------------
[10:00:02 ERROR SMAPI]    These mods could not be added to your game.
[10:00:02 ERROR SMAPI]
[10:00:02 ERROR SMAPI]    - Needy Mod 2.1 because it needs mod Some.Base which isn't installed.
[10:00:02 ERROR SMAPI]    - Old Mod 0.3 because it's not compatible with the latest version of the game.
[10:00:03 ALERT SMAPI] You can update 2 mods:
[10:00:03 ALERT SMAPI]    Pretty Mod 1.0 \u{2192} 1.1: https://example.com/a
[10:00:03 ALERT SMAPI]    Other Mod 2.0 -> 2.5: https://example.com/b
[10:00:04 ERROR Pretty Mod] Something broke
[10:00:05 ERROR Pretty Mod] Again
[10:00:05 WARN  Pretty Mod] Careful
[10:00:06 ERROR SMAPI] Unrelated SMAPI error
";

    #[test]
    fn versions_and_counts_are_read_from_the_header() {
        let s = summarize_log(SAMPLE);
        assert_eq!(s.smapi_version.as_deref(), Some("4.1.10"));
        assert_eq!(s.game_version.as_deref(), Some("1.6.15"));
        assert_eq!(s.loaded_mod_count, Some(12));
        assert_eq!(s.total_lines, 17);
    }

    #[test]
    fn skipped_mods_carry_reason_line_and_missing_dependencies() {
        let s = summarize_log(SAMPLE);
        assert_eq!(s.skipped_mods.len(), 2);
        let needy = &s.skipped_mods[0];
        assert_eq!(needy.name, "Needy Mod");
        assert_eq!(needy.version.as_deref(), Some("2.1"));
        assert_eq!(needy.missing_dependencies, vec!["Some.Base"]);
        assert_eq!(needy.line, 9);
        let old = &s.skipped_mods[1];
        assert!(old.missing_dependencies.is_empty());
        assert!(old.reason.contains("not compatible"));
    }

    #[test]
    fn update_notices_accept_both_arrow_styles() {
        let s = summarize_log(SAMPLE);
        let names: Vec<_> = s
            .update_notices
            .iter()
            .map(|n| {
                (
                    n.name.as_str(),
                    n.current_version.as_str(),
                    n.available_version.as_str(),
                )
            })
            .collect();
        assert_eq!(
            names,
            vec![("Pretty Mod", "1.0", "1.1"), ("Other Mod", "2.0", "2.5")]
        );
    }

    #[test]
    fn errors_and_warnings_are_counted_per_source_with_the_first_line() {
        let s = summarize_log(SAMPLE);
        let pretty = s.sources.iter().find(|c| c.source == "Pretty Mod").unwrap();
        assert_eq!((pretty.errors, pretty.warnings), (2, 1));
        assert_eq!(pretty.first_error_line, Some(14));
    }

    /// The shape SMAPI 4 prints (its format strings are "You can update ",
    /// " mod" and " (you have "), mixed with the older arrow shape.
    #[test]
    fn update_notices_read_smapis_own_you_have_shape() {
        let log = "\
[21:20:05 ALERT SMAPI] You can update 3 mods:
[21:20:05 ALERT SMAPI]    Chests Anywhere 1.25.1: https://www.nexusmods.com/stardewvalley/mods/518 (you have 1.23.0)
[21:20:05 ALERT SMAPI]    Lookup Anything 1.41.0-beta.2: https://www.nexusmods.com/stardewvalley/mods/541 (you have 1.40.0)
[21:20:05 ALERT SMAPI]    Tractor Mod 4.17.0 \u{2192} 4.18.2: https://www.nexusmods.com/stardewvalley/mods/1401
[21:20:06 INFO  SMAPI] Type 'help' for help, or 'help <cmd>' for a command's usage
";
        let s = summarize_log(log);
        let notices: Vec<_> = s
            .update_notices
            .iter()
            .map(|n| {
                (
                    n.name.as_str(),
                    n.current_version.as_str(),
                    n.available_version.as_str(),
                )
            })
            .collect();
        assert_eq!(
            notices,
            vec![
                ("Chests Anywhere", "1.23.0", "1.25.1"),
                ("Lookup Anything", "1.40.0", "1.41.0-beta.2"),
                ("Tractor Mod", "4.17.0", "4.18.2"),
            ]
        );
    }

    /// A session log in SMAPI 4.1.10's exact shape, rebuilt from its source
    /// (LogManager.LogIntro/LogModInfo/LogModWarnings, SCore.TryLoadMod and the
    /// update check) and the format strings in StardewModdingAPI.dll.
    const SMAPI_4_1_10: &str = include_str!("../../tests/fixtures/SMAPI-4.1.10-session.txt");

    #[test]
    fn a_real_smapi_log_has_one_skipped_mod_not_the_warning_groups_after_it() {
        let s = summarize_log(SMAPI_4_1_10);
        let skipped: Vec<_> = s
            .skipped_mods
            .iter()
            .map(|m| (m.name.as_str(), m.version.as_deref(), m.reason.as_str()))
            .collect();
        assert_eq!(
            skipped,
            vec![("Weather Plus", Some("3.1.0"), "its DLL couldn't be loaded.")]
        );
        assert_eq!(s.smapi_version.as_deref(), Some("4.1.10"));
        assert_eq!(s.game_version.as_deref(), Some("1.6.15"));
        assert_eq!(s.loaded_mod_count, Some(2));
        assert_eq!(s.update_notices.len(), 1);
        assert_eq!(s.update_notices[0].name, "Content Patcher");
        assert_eq!(s.update_notices[0].current_version, "2.1.0");
        assert_eq!(s.update_notices[0].available_version, "2.2.0");
        // The skipped block is not an SMAPI error in itself.
        assert!(s.errors.is_empty(), "{:?}", s.errors);
    }

    /// The same session as SMAPI writes it on Windows: CRLF line endings,
    /// `~\` and backslash paths, an absolute game folder.
    const SMAPI_4_1_10_WINDOWS: &str =
        include_str!("../../tests/fixtures/SMAPI-4.1.10-session-windows.txt");

    #[test]
    fn a_windows_log_summarises_the_same_as_the_posix_one() {
        assert!(SMAPI_4_1_10_WINDOWS.contains("\r\n"));
        let windows = summarize_log(SMAPI_4_1_10_WINDOWS);
        let posix = summarize_log(SMAPI_4_1_10);
        assert_eq!(windows.skipped_mods, posix.skipped_mods);
        assert_eq!(windows.update_notices, posix.update_notices);
        assert_eq!(windows.loaded_mod_count, Some(2));
        assert_eq!(windows.sources, posix.sources);
        assert!(windows
            .skipped_mods
            .iter()
            .all(|m| !m.reason.ends_with('\r')));
    }

    #[test]
    fn skipped_entries_read_smapis_technical_details_and_missing_mod_lists() {
        let log = "\
[10:00:07 ERROR SMAPI]    Skipped mods
[10:00:07 ERROR SMAPI]    --------------------------------------------------
[10:00:07 ERROR SMAPI]       These mods could not be added to your game.

[10:00:07 ERROR SMAPI]       - Romance Plus 1.0.0 (ID: Romance.Plus, path: RomancePlus) because it requires mods which aren't installed (Content Patcher: https://smapi.io/mods, Missing.Dep).
";
        let s = summarize_log(log);
        assert_eq!(s.skipped_mods.len(), 1);
        assert_eq!(s.skipped_mods[0].name, "Romance Plus");
        assert_eq!(s.skipped_mods[0].version.as_deref(), Some("1.0.0"));
        assert_eq!(
            s.skipped_mods[0].missing_dependencies,
            vec!["Content Patcher", "Missing.Dep"]
        );
    }

    #[test]
    fn a_blank_error_line_in_the_skipped_block_is_not_an_error() {
        let s = summarize_log(SAMPLE);
        let smapi = s.sources.iter().find(|c| c.source == "SMAPI").unwrap();
        assert_eq!(smapi.errors, 1, "only 'Unrelated SMAPI error' is an error");
        assert_eq!(smapi.first_error_line, Some(17));
        assert!(s.errors.iter().all(|e| !e.message.is_empty()));
    }

    #[test]
    fn skipped_section_entries_do_not_leak_into_later_lines() {
        let s = summarize_log(SAMPLE);
        assert!(s.skipped_mods.iter().all(|m| m.name != "Pretty Mod"));
    }

    #[test]
    fn a_log_it_cannot_read_yields_an_empty_summary() {
        let s = summarize_log("random text\nnot a smapi log");
        assert_eq!(s.smapi_version, None);
        assert!(s.skipped_mods.is_empty() && s.sources.is_empty());
    }
}
