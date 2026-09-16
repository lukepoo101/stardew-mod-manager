use std::fs;
use std::path::Path;

fn visit(path: &Path, violations: &mut Vec<String>) {
    for entry in fs::read_dir(path).expect("read manager-app source") {
        let entry = entry.expect("read source entry");
        let path = entry.path();
        if path.is_dir() {
            visit(&path, violations);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            let source = fs::read_to_string(&path).expect("read Rust source");
            for (line_number, line) in source.lines().enumerate() {
                if [
                    "std::fs",
                    "std::process",
                    "std::env",
                    ".exists()",
                    ".is_file()",
                    ".is_dir()",
                ]
                .iter()
                .any(|pattern| line.contains(pattern))
                {
                    violations.push(format!("{}:{}: {}", path.display(), line_number + 1, line));
                }
            }
        }
    }
}

#[test]
fn application_layer_does_not_perform_infrastructure_io() {
    let mut violations = Vec::new();
    visit(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(),
        &mut violations,
    );
    assert!(
        violations.is_empty(),
        "manager-app I/O boundary violations:\n{}",
        violations.join("\n")
    );
}
