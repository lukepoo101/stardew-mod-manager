use crate::ids::ModUniqueId;
use crate::manifest::model::{ContentPackFor, Manifest, ModDependency};
use crate::version::SmapiVersion;
use serde_json::Value;

pub fn clean_json_comments(input: &str) -> String {
    let input = input.trim_start_matches('\u{feff}');
    let mut result = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if escape {
            result.push(c);
            escape = false;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape = true;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            continue;
        }

        if !in_string && c == '/' {
            if let Some(&next_c) = chars.peek() {
                if next_c == '/' {
                    chars.next();
                    for nc in chars.by_ref() {
                        if nc == '\n' {
                            result.push('\n');
                            break;
                        }
                    }
                    continue;
                } else if next_c == '*' {
                    chars.next();
                    while let Some(nc) = chars.next() {
                        if nc == '*' {
                            if let Some(&maybe_slash) = chars.peek() {
                                if maybe_slash == '/' {
                                    chars.next();
                                    break;
                                }
                            }
                        }
                    }
                    continue;
                }
            }
        }

        result.push(c);
    }

    clean_trailing_commas(&result)
}

fn clean_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if escape {
            result.push(c);
            escape = false;
            i += 1;
            continue;
        }

        if c == '\\' && in_string {
            result.push(c);
            escape = true;
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = !in_string;
            result.push(c);
            i += 1;
            continue;
        }

        if !in_string && c == ',' {
            let mut j = i + 1;
            let mut is_trailing = false;
            while j < chars.len() {
                if chars[j].is_whitespace() {
                    j += 1;
                } else if chars[j] == '}' || chars[j] == ']' {
                    is_trailing = true;
                    break;
                } else {
                    break;
                }
            }

            if is_trailing {
                i += 1;
                continue;
            }
        }

        result.push(c);
        i += 1;
    }

    result
}

pub fn parse_manifest(raw_json: &str) -> Result<Manifest, String> {
    let raw_json = raw_json.trim_start_matches('\u{feff}');
    let cleaned = clean_json_comments(raw_json);
    let val: Value = serde_json::from_str(&cleaned)
        .map_err(|e| format!("Failed to parse manifest.json: {}", e))?;

    let obj = val
        .as_object()
        .ok_or_else(|| "manifest.json root must be a JSON object".to_string())?;

    let get_field = |key: &str| -> Option<&Value> {
        obj.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    };

    let name = get_field("Name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing or invalid 'Name' field in manifest".to_string())?
        .trim()
        .to_string();

    let author = get_field("Author")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .trim()
        .to_string();

    let version_str = get_field("Version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing or invalid 'Version' field in manifest".to_string())?
        .trim();

    SmapiVersion::parse(version_str)
        .map_err(|e| format!("Invalid 'Version' in manifest '{}': {}", version_str, e))?;

    let unique_id = get_field("UniqueID")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Missing or invalid 'UniqueID' field in manifest".to_string())?
        .trim()
        .to_string();

    if unique_id.is_empty() {
        return Err("Manifest 'UniqueID' cannot be empty".to_string());
    }

    if unique_id == "."
        || unique_id == ".."
        || unique_id.contains("..")
        || unique_id.contains('/')
        || unique_id.contains('\\')
        || unique_id.contains('\0')
    {
        return Err(format!(
            "Manifest 'UniqueID' contains illegal characters or path traversal: '{}'",
            unique_id
        ));
    }

    if unique_id.starts_with('.') || unique_id.ends_with('.') {
        return Err(format!(
            "Manifest 'UniqueID' cannot start or end with a period: '{}'",
            unique_id
        ));
    }

    let description = get_field("Description")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    let entry_dll = get_field("EntryDll")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    let minimum_api_version = get_field("MinimumApiVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    if let Some(ref min_api) = minimum_api_version {
        SmapiVersion::parse(min_api).map_err(|e| {
            format!(
                "Invalid 'MinimumApiVersion' in manifest '{}': {}",
                min_api, e
            )
        })?;
    }

    let minimum_game_version = get_field("MinimumGameVersion")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string());

    let mut update_keys = Vec::new();
    if let Some(keys_val) = get_field("UpdateKeys").and_then(|v| v.as_array()) {
        for key_item in keys_val {
            if let Some(k_str) = key_item.as_str() {
                let trimmed = k_str.trim();
                if !trimmed.is_empty() {
                    update_keys.push(trimmed.to_string());
                }
            }
        }
    }

    let mut dependencies = Vec::new();
    if let Some(deps_val) = get_field("Dependencies").and_then(|v| v.as_array()) {
        for item in deps_val {
            if let Some(dep_obj) = item.as_object() {
                let dep_get = |k: &str| -> Option<&Value> {
                    dep_obj
                        .iter()
                        .find(|(dk, _)| dk.eq_ignore_ascii_case(k))
                        .map(|(_, dv)| dv)
                };

                if let Some(dep_id) = dep_get("UniqueID").and_then(|v| v.as_str()) {
                    let dep_id = dep_id.trim().to_string();
                    let min_v = dep_get("MinimumVersion")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string());

                    if let Some(ref mv) = min_v {
                        if let Err(e) = SmapiVersion::parse(mv) {
                            return Err(format!(
                                "Invalid dependency MinimumVersion '{}': {}",
                                mv, e
                            ));
                        }
                    }

                    let is_req = dep_get("IsRequired")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(true);

                    dependencies.push(ModDependency {
                        unique_id: ModUniqueId::new(dep_id),
                        minimum_version: min_v,
                        is_required: is_req,
                    });
                }
            }
        }
    }

    let content_pack_for = if let Some(cp_val) =
        get_field("ContentPackFor").and_then(|v| v.as_object())
    {
        let cp_get = |k: &str| -> Option<&Value> {
            cp_val
                .iter()
                .find(|(ck, _)| ck.eq_ignore_ascii_case(k))
                .map(|(_, cv)| cv)
        };

        let cp_unique_id = cp_get("UniqueID")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Missing or invalid 'UniqueID' in ContentPackFor object".to_string())?
            .trim()
            .to_string();

        let min_v = cp_get("MinimumVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string());

        if let Some(ref mv) = min_v {
            if let Err(e) = SmapiVersion::parse(mv) {
                return Err(format!(
                    "Invalid ContentPackFor MinimumVersion '{}': {}",
                    mv, e
                ));
            }
        }

        Some(ContentPackFor {
            unique_id: ModUniqueId::new(cp_unique_id),
            minimum_version: min_v,
        })
    } else {
        None
    };

    Ok(Manifest {
        unique_id: ModUniqueId::new(unique_id),
        name,
        author,
        version: version_str.to_string(),
        description,
        entry_dll,
        minimum_api_version,
        minimum_game_version,
        update_keys,
        dependencies,
        content_pack_for,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_clean_json_comments_and_trailing_commas() {
        let json_with_comments = r#"{
            // Single line comment
            "Name": "Test Mod", /* Inline comment */
            "Version": "1.0.0",
            "UniqueID": "Author.TestMod",
            "Dependencies": [
                {
                    "UniqueID": "Dep.Mod",
                    "IsRequired": true,
                },
            ],
            /*
             Multi line comment
            */
            "EntryDll": "Test.dll",
        }"#;

        let manifest = parse_manifest(json_with_comments).unwrap();
        assert_eq!(manifest.name, "Test Mod");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.unique_id.as_str(), "Author.TestMod");
        assert_eq!(manifest.dependencies.len(), 1);
        assert_eq!(manifest.dependencies[0].unique_id.as_str(), "Dep.Mod");
    }

    #[test]
    fn test_manifest_with_utf8_bom() {
        let bom_json =
            "\u{feff}{\"Name\": \"Bom Mod\", \"Version\": \"2.0.0\", \"UniqueID\": \"Author.Bom\"}";
        let manifest = parse_manifest(bom_json).unwrap();
        assert_eq!(manifest.name, "Bom Mod");
        assert_eq!(manifest.version, "2.0.0");
    }

    #[test]
    fn test_content_pack_manifest() {
        let cp_json = r#"{
            "Name": "CP Mod",
            "Author": "Artist",
            "Version": "1.0.0",
            "UniqueID": "Artist.CPMod",
            "ContentPackFor": {
                "UniqueID": "Pathoschild.ContentPatcher",
                "MinimumVersion": "1.20.0"
            },
            "UpdateKeys": ["Nexus:1234"]
        }"#;
        let manifest = parse_manifest(cp_json).unwrap();
        assert_eq!(manifest.name, "CP Mod");
        assert!(manifest.content_pack_for.is_some());
        assert_eq!(
            manifest.content_pack_for.unwrap().unique_id.as_str(),
            "Pathoschild.ContentPatcher"
        );
        assert_eq!(manifest.update_keys, vec!["Nexus:1234"]);
    }
}
