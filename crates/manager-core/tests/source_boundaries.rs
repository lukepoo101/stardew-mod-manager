//! Domain code must reach the outside world through ports, not `std` I/O.

use std::path::{Path, PathBuf};

const FORBIDDEN: [&str; 3] = ["std::fs", "std::process", "std::env"];

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable source directory") {
        let path = entry.expect("readable entry").path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn manager_core_does_not_touch_the_filesystem_process_table_or_environment() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&src, &mut files);

    let mut violations = Vec::new();
    for file in files {
        let relative = file
            .strip_prefix(&src)
            .expect("path under src")
            .to_string_lossy()
            .replace('\\', "/");

        let contents = std::fs::read_to_string(&file).expect("readable source file");
        for (index, line) in contents.lines().enumerate() {
            if let Some(hit) = FORBIDDEN.iter().find(|needle| line.contains(**needle)) {
                violations.push(format!("{}:{}: {}", relative, index + 1, hit));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "manager-core must use ports instead of std I/O:\n{}",
        violations.join("\n")
    );
}
