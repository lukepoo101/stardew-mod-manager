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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSummary {
    pub smapi_version: Option<String>,
    pub game_version: Option<String>,
    pub loaded_mod_count: Option<usize>,
    pub skipped_mods: Vec<SkippedMod>,
    pub update_notices: Vec<ModUpdateNotice>,
    pub sources: Vec<SourceCount>,
    pub total_lines: usize,
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
    found
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
                    let entry = content.trim_start_matches('-').trim();
                    // Skip the ruler and the explanatory sentence.
                    if entry.is_empty()
                        || entry.chars().all(|c| c == '-')
                        || entry.starts_with("These mods could not")
                    {
                        continue;
                    }
                    let (subject, reason) = match entry.split_once(" because ") {
                        Some((subject, reason)) => (subject, reason.trim()),
                        None => (entry, ""),
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
                Section::Updates => {
                    // "Name 1.0 -> 1.1: https://..." (arrow varies by version).
                    let text = content.split(": http").next().unwrap_or(content);
                    let arrow = if text.contains('\u{2192}') {
                        Some('\u{2192}')
                    } else if text.contains("->") {
                        Some('>')
                    } else {
                        None
                    };
                    if let Some(arrow) = arrow {
                        let (left, right) = if arrow == '>' {
                            text.split_once("->").unwrap_or((text, ""))
                        } else {
                            text.split_once('\u{2192}').unwrap_or((text, ""))
                        };
                        let (name, current) = split_name_version(left);
                        let available = right.trim().trim_end_matches(':').trim().to_string();
                        if let (Some(current), false) = (current, available.is_empty()) {
                            summary.update_notices.push(ModUpdateNotice {
                                name,
                                current_version: current,
                                available_version: available,
                                line: number,
                            });
                        }
                    }
                    continue;
                }
                Section::None => {}
            }
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
