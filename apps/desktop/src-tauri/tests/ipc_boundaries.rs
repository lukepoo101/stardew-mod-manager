//! Guardrail for the structured IPC error boundary.
//!
//! Application failures must reach JavaScript as a serialized ApiErrorDto. These
//! checks deliberately scan source text instead of types, because the regression
//! they prevent is a signature change (Result<T, String>) or a conversion
//! (map_err(|e| e.to_string())) that still compiles.
//!
//! The scan is intentionally narrow: it covers the canonical IPC command modules
//! rather than banning .to_string() across the crate, where it is legitimate.

use std::fs;
use std::path::Path;

/// Tauri command modules that form the product IPC surface.
const IPC_COMMAND_MODULES: [&str; 2] = ["src/commands.rs", "src/modern_smapi.rs"];

fn command_return_lines(source: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut returns = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        if !line.contains("#[tauri::command]") {
            continue;
        }
        for (offset, candidate) in lines.iter().enumerate().skip(index + 1) {
            if candidate.contains("-> ") {
                returns.push((offset + 1, candidate.trim().to_string()));
                break;
            }
            if offset > index + 15 {
                break;
            }
        }
    }
    returns
}

#[test]
fn ipc_command_modules_keep_structured_errors() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for module in IPC_COMMAND_MODULES {
        let path = manifest_dir.join(module);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("could not read {module}: {error}"));
        let lines: Vec<&str> = source.lines().collect();

        for (index, line) in lines.iter().enumerate() {
            let location = format!("{module}:{}", index + 1);
            let trimmed = line.trim();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("Result<") && trimmed.contains(", String>") {
                violations.push(format!(
                    "{location}: command result returns an arbitrary string error: {trimmed}"
                ));
            }
            if trimmed.contains("map_err(") && trimmed.contains(".to_string()") {
                violations.push(format!(
                    "{location}: application error is flattened with to_string(): {trimmed}"
                ));
            }
            let next_line = lines.get(index + 1).map(|next| next.trim()).unwrap_or("");
            if trimmed.ends_with(".ok()") && next_line.starts_with(".flatten()") {
                violations.push(format!(
                    "{location}: a backend failure is suppressed into absence with .ok().flatten()"
                ));
            }
        }

        let returns = command_return_lines(&source);
        assert!(
            !returns.is_empty(),
            "{module} declares no Tauri commands; the structured-error guard is scanning the wrong file."
        );
        for (line_number, declaration) in returns {
            if !declaration.contains("IpcResult<") {
                violations.push(format!(
                    "{module}:{line_number}: command does not return IpcResult<T>: {declaration}"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "IPC commands must preserve the structured error contract:\n{}",
        violations.join("\n")
    );
}
