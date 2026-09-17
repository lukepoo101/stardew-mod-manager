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
/// Operation bookkeeping has exactly one writer.
///
/// The regression this prevents is not a type error: a service that writes
/// operation state or steps directly still compiles, and silently bypasses both
/// the transition validation and the lifecycle helper. The guard is a plain
/// substring scan on purpose - it is not a Rust parser.
#[test]
fn operation_state_and_steps_are_only_written_through_the_lifecycle_helper() {
    let services = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services");
    let mut violations = Vec::new();

    for entry in fs::read_dir(&services).expect("read services") {
        let path = entry.expect("service entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        if path.file_name().and_then(|name| name.to_str()) == Some("operation_lifecycle.rs") {
            continue;
        }
        let source = fs::read_to_string(&path).expect("read service source");
        for (line_number, line) in source.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("update_operation_state(")
                || trimmed.contains("save_operation_step(")
            {
                violations.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    line_number + 1,
                    trimmed
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "operation bookkeeping must go through OperationLifecycle:\n{}",
        violations.join("\n")
    );

    // Keep the guard honest: if the helper stops writing those records, this
    // test would otherwise pass while nothing enforced anything.
    let lifecycle = fs::read_to_string(services.join("operation_lifecycle.rs"))
        .expect("read the lifecycle helper");
    assert!(lifecycle.contains("update_operation_state("));
    assert!(lifecycle.contains("save_operation_step("));
}
