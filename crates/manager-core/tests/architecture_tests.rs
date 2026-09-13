#[test]
fn test_manager_core_cargo_dependencies_guardrail() {
    let cargo_toml = include_str!("../Cargo.toml");
    let forbidden = ["rusqlite", "tauri", "reqwest", "tokio", "libc"];
    for dep in forbidden {
        assert!(
            !cargo_toml.contains(&format!("{dep} =")) && !cargo_toml.contains(&format!("{dep}.")),
            "manager-core Cargo.toml must not depend on forbidden infrastructure crate: {dep}"
        );
    }
}
