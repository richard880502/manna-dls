//! Dependency rules for the application/domain boundary. These checks prevent shared
//! logic from drifting back into an HTTP module as new entry points are introduced.
use std::path::{Path, PathBuf};

fn rust_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|e| e == "rs") {
            files.push(path);
        }
    }
}

fn code(path: &Path) -> String {
    std::fs::read_to_string(path)
        .expect("Rust source")
        .lines()
        .map(|line| line.split("//").next().unwrap_or_default())
        .collect::<String>()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

#[test]
fn application_and_domain_code_do_not_import_http_routes() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    for path in files {
        let relative = path.strip_prefix(&root).unwrap();
        if relative.starts_with("routes") || matches!(relative.to_str(), Some("lib.rs" | "main.rs"))
        {
            continue;
        }
        assert!(
            !code(&path).contains("crate::routes"),
            "{} depends on HTTP routes; move the reusable operation or type below the transport layer",
            relative.display()
        );
    }
}

#[test]
fn provider_execution_has_no_application_or_feature_dependencies() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ai/execution.rs");
    let source = code(&path);
    for dependency in [
        "application",
        "memory",
        "prep",
        "sop",
        "worksheet",
        "objectives",
    ] {
        assert!(
            !source.contains(&format!("crate::{dependency}")),
            "provider execution must not orchestrate {dependency}; enrich the request in the application layer"
        );
    }
}

#[test]
fn worksheet_revision_rules_do_not_depend_on_io_or_persistence() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/worksheet/revise/rules.rs");
    let source = code(&path);
    let production = source.split("#[cfg(test)]").next().unwrap();
    for dependency in [
        "usesuper::*",
        "super::persistence",
        "worksheet::revise::persistence",
        "worksheet::store",
        "super::super::store",
        "sqlx::",
        "reqwest::",
        "axum::",
        "crate::routes",
    ] {
        assert!(
            !production.contains(dependency),
            "worksheet revision rules must not depend on I/O or persistence: {dependency}"
        );
    }
}
