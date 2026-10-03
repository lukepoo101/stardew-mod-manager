//! SMAPI's version is read from its assembly, because newer installers ship
//! a StardewModdingAPI.deps.json that no longer names SMAPI.

use manager_infra::smapi_adapter::{detect_installed_smapi_version, version_from_dll};

/// A version-resource string entry as a PE file lays it out: the UTF-16 key,
/// its terminator, padding to four bytes, then the UTF-16 value.
fn entry(key: &str, value: &str) -> Vec<u8> {
    let utf16 = |s: &str| -> Vec<u8> {
        s.encode_utf16()
            .chain(std::iter::once(0))
            .flat_map(u16::to_le_bytes)
            .collect()
    };
    let mut bytes = vec![0x40, 0x00, 0x16, 0x00, 0x01, 0x00];
    bytes.extend(utf16(key));
    while bytes.len() % 4 != 0 {
        bytes.push(0);
    }
    bytes.extend(utf16(value));
    bytes
}

fn dll(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut bytes = b"MZ\x90\x00 some code StardewModdingAPI 1.2.3".to_vec();
    for (key, value) in entries {
        bytes.extend(entry(key, value));
    }
    bytes
}

#[test]
fn the_product_version_without_build_metadata_is_used() {
    let bytes = dll(&[
        ("FileVersion", "4.5.2.0"),
        (
            "ProductVersion",
            "4.5.2+821167e5c511bf3a2d98f604e5e838561c469219",
        ),
    ]);
    assert_eq!(version_from_dll(&bytes).as_deref(), Some("4.5.2"));
}

#[test]
fn the_file_version_is_the_fallback() {
    let bytes = dll(&[("FileVersion", "4.1.10.0")]);
    assert_eq!(version_from_dll(&bytes).as_deref(), Some("4.1.10"));
    assert_eq!(version_from_dll(b"MZ no resources"), None);
}

#[test]
fn a_manifest_without_smapi_falls_back_to_the_assembly() {
    let dir = tempfile::tempdir().unwrap();
    // What the SMAPI 4.5 installer writes: the game's own manifest.
    std::fs::write(
        dir.path().join("StardewModdingAPI.deps.json"),
        r#"{"targets":{".NETCoreApp,Version=v6.0/linux-x64":{"Stardew Valley/1.6.15.24356":{}}}}"#,
    )
    .unwrap();
    assert_eq!(detect_installed_smapi_version(dir.path()), None);
    std::fs::write(
        dir.path().join("StardewModdingAPI.dll"),
        dll(&[("ProductVersion", "4.5.2+abc")]),
    )
    .unwrap();
    assert_eq!(
        detect_installed_smapi_version(dir.path()).as_deref(),
        Some("4.5.2")
    );
}

#[test]
fn an_older_manifest_that_names_smapi_still_works() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("StardewModdingAPI.deps.json"),
        r#"{"targets":{".NETCoreApp,Version=v6.0":{"StardewModdingAPI/3.18.6":{}}}}"#,
    )
    .unwrap();
    assert_eq!(
        detect_installed_smapi_version(dir.path()).as_deref(),
        Some("3.18.6")
    );
}
