use assert_cmd::Command;
use std::fs;
use std::path::Path;
use tempfile::tempdir;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn package_json(root: &Path) {
    write(
        &root.join("package.json"),
        r#"{"dependencies":{"zod":"latest"}}"#,
    );
}

fn private_schema_single(root: &Path, entry: &str) {
    write(
        &root.join("src/env.private.ts"),
        &format!(
            r#"import {{ createEnv }} from "@t3-oss/env-core";
import {{ z }} from "zod";

export const privateEnv = createEnv({{
  runtimeEnv: process.env,
  server: {{
    {entry},
  }},
}});
"#
        ),
    );
}

fn private_schema_formatted_two(root: &Path) {
    // Already formatted: blank line between different prefix groups.
    write(
        &root.join("src/env.private.ts"),
        r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    DATABASE_URL: z.string(),

    EXTRA_VAR: z.string().optional(),
  },
});
"#,
    );
}

/// Minimal healthy single-app repo: schema/template/local aligned and formatted.
fn healthy_repo(root: &Path) {
    package_json(root);
    write(&root.join(".env.example"), "DATABASE_URL=file:./local.db\n");
    write(&root.join(".env"), "DATABASE_URL=file:./local.db\n");
    private_schema_single(root, "DATABASE_URL: z.string()");
}

/// Repo with an error-severity issue: public var missing from runtimeEnvStrict.
fn error_repo(root: &Path) {
    package_json(root);
    write(&root.join(".env.example"), "PUBLIC_FOO=hello\n");
    write(&root.join(".env"), "PUBLIC_FOO=hello\n");
    write(
        &root.join("src/env.public.ts"),
        r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const publicEnv = createEnv({
  emptyStringAsUndefined: true,
  clientPrefix: "PUBLIC_",
  client: {
    PUBLIC_FOO: z.string(),
  },
  runtimeEnvStrict: {},
});
"#,
    );
}

/// Repo with only a warning: schema var missing from template (optional so no
/// required-missing-from-local warning, formatted so no formatting warning).
fn warning_repo(root: &Path) {
    package_json(root);
    write(&root.join(".env.example"), "DATABASE_URL=file:./local.db\n");
    write(&root.join(".env"), "DATABASE_URL=file:./local.db\n");
    private_schema_formatted_two(root);
}

/// Repo with only an info: missing `.env` for an optional var.
fn info_repo(root: &Path) {
    package_json(root);
    write(&root.join(".env.example"), "# DATABASE_URL=\n");
    private_schema_single(root, "DATABASE_URL: z.string().optional()");
    // Intentionally no `.env`.
}

fn doctor(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("crabenv").unwrap();
    cmd.arg("--root").arg(root);
    cmd.arg("doctor");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.assert()
}

fn format_cmd(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("crabenv").unwrap();
    cmd.arg("--root").arg(root);
    cmd.arg("format");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.assert()
}

#[test]
fn healthy_doctor_check_passes() {
    let dir = tempdir().unwrap();
    healthy_repo(dir.path());

    doctor(dir.path(), &["--check"]).success();
    doctor(dir.path(), &[]).success();
}

#[test]
fn error_doctor_check_fails_but_plain_doctor_passes() {
    let dir = tempdir().unwrap();
    error_repo(dir.path());

    // Default behavior stays compatible: plain `doctor` exits 0 even with errors.
    doctor(dir.path(), &[]).success();
    // CI gate: `--check` exits nonzero on error severity.
    doctor(dir.path(), &["--check"]).failure();
}

#[test]
fn warning_only_does_not_fail_check() {
    let dir = tempdir().unwrap();
    warning_repo(dir.path());

    let assert = doctor(dir.path(), &["--check"]);
    assert.success();

    // Sanity: the repo really has a warning (not accidentally healthy).
    let assert = doctor(dir.path(), &["--json"]);
    let output = assert.success().get_output().stdout.clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let issues = value.get("issues").unwrap().as_array().unwrap();
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("severity").unwrap() == "warn"),
        "expected at least one warning, got {value}"
    );
    assert!(
        issues
            .iter()
            .all(|issue| issue.get("severity").unwrap() != "error"),
        "expected no errors, got {value}"
    );
}

#[test]
fn info_only_does_not_fail_check() {
    let dir = tempdir().unwrap();
    info_repo(dir.path());

    doctor(dir.path(), &["--check"]).success();

    let assert = doctor(dir.path(), &["--json"]);
    let output = assert.success().get_output().stdout.clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let issues = value.get("issues").unwrap().as_array().unwrap();
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("severity").unwrap() == "info"),
        "expected an info issue, got {value}"
    );
}

#[test]
fn repo_only_without_env_filters_local_checks() {
    let dir = tempdir().unwrap();
    info_repo(dir.path());
    assert!(!dir.path().join(".env").exists());

    // Without --repo-only the missing `.env` is reported (info, still passes --check).
    let output = doctor(dir.path(), &["--json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert!(
        value
            .get("issues")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .any(|issue| issue.get("code").unwrap() == "missing-local-env"),
        "expected missing-local-env without --repo-only, got {value}"
    );

    // With --repo-only the same directory has no issues and needs no secrets.
    let output = doctor(dir.path(), &["--repo-only", "--json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(
        value.get("repo_only").unwrap(),
        &serde_json::Value::Bool(true)
    );
    assert!(
        value.get("issues").unwrap().as_array().unwrap().is_empty(),
        "repo-only should filter missing-local-env, got {value}"
    );

    doctor(dir.path(), &["--repo-only", "--check"]).success();
}

#[test]
fn repo_only_checks_and_fixes_never_parse_or_rewrite_local_env() {
    let dir = tempdir().unwrap();
    healthy_repo(dir.path());
    let local = "DATABASE_URL=\"unterminated\n";
    write(&dir.path().join(".env"), local);
    // Force a repository template backfill, exercising the fix path too.
    write(&dir.path().join(".env.example"), "");

    doctor(dir.path(), &["--check"]).failure();
    doctor(dir.path(), &["--repo-only", "--check", "--json"]).success();
    doctor(dir.path(), &["--repo-only", "--fix", "--yes", "--check"]).success();
    assert_eq!(fs::read_to_string(dir.path().join(".env")).unwrap(), local);
    assert!(fs::read_to_string(dir.path().join(".env.example"))
        .unwrap()
        .contains("DATABASE_URL="));
}

#[test]
fn failing_json_check_keeps_stdout_machine_readable() {
    let dir = tempdir().unwrap();
    error_repo(dir.path());
    let output = doctor(dir.path(), &["--json", "--check"])
        .code(1)
        .get_output()
        .clone();
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["check"], true);
    assert!(report["summary"]["error"].as_u64().unwrap() > 0);
    assert!(!output.stderr.is_empty());
}

#[test]
fn json_output_is_parseable_with_stable_metadata() {
    let dir = tempdir().unwrap();
    error_repo(dir.path());

    let output = doctor(dir.path(), &["--json"])
        .success()
        .get_output()
        .stdout
        .clone();
    // Pure machine-readable stdout: the whole stdout must parse as JSON.
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("doctor --json stdout must be pure JSON");

    assert_eq!(value.get("version").unwrap(), &serde_json::Value::from(1));
    let issues = value.get("issues").unwrap().as_array().unwrap();
    assert!(!issues.is_empty(), "expected issues, got {value}");
    for issue in issues {
        for field in [
            "code",
            "severity",
            "message",
            "owner",
            "paths",
            "remediation",
            "fixable",
        ] {
            assert!(
                issue.get(field).is_some(),
                "issue missing field {field}: {issue}"
            );
        }
        let code = issue.get("code").unwrap().as_str().unwrap();
        assert!(!code.is_empty(), "code must be non-empty");
        assert!(
            ["error", "warn", "info"].contains(&issue.get("severity").unwrap().as_str().unwrap()),
            "unknown severity in {issue}"
        );
        assert!(
            !issue
                .get("remediation")
                .unwrap()
                .as_str()
                .unwrap()
                .is_empty(),
            "remediation must be non-empty"
        );
    }
    let summary = value.get("summary").unwrap();
    for field in ["error", "warn", "info", "total"] {
        assert!(summary.get(field).is_some(), "summary missing {field}");
    }
    // Error fixture must surface the stable public-strict code.
    assert!(
        issues
            .iter()
            .any(|issue| issue.get("code").unwrap() == "public-var-missing-runtime-strict"),
        "expected public-var-missing-runtime-strict code, got {value}"
    );
}

#[test]
fn json_paths_are_repo_relative() {
    let dir = tempdir().unwrap();
    error_repo(dir.path());

    let output = doctor(dir.path(), &["--json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let root = dir.path().to_string_lossy().to_string();
    for issue in value.get("issues").unwrap().as_array().unwrap() {
        for path in issue.get("paths").unwrap().as_array().unwrap() {
            let path = path.as_str().unwrap();
            assert!(
                !path.starts_with('/'),
                "paths must be repo-relative, got {path}"
            );
            assert!(
                !path.contains(&root),
                "paths must not contain absolute root, got {path}"
            );
        }
        // Owner is repo-relative where known ("." or apps/...) or null.
        if let Some(owner) = issue.get("owner").unwrap().as_str() {
            assert!(
                !owner.starts_with('/'),
                "owner must be relative, got {owner}"
            );
        }
    }
    // Fixture error must point at the public schema file, repo-relative.
    assert!(
        value
            .get("issues")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|issue| issue.get("paths").unwrap().as_array().unwrap().clone())
            .any(|path| path.as_str().unwrap() == "src/env.public.ts"),
        "expected repo-relative src/env.public.ts, got {value}"
    );
}

#[test]
fn json_rejects_fix_combination() {
    let dir = tempdir().unwrap();
    healthy_repo(dir.path());

    doctor(dir.path(), &["--json", "--fix"]).failure();
    doctor(dir.path(), &["--json", "--fix", "--yes"]).failure();
    doctor(dir.path(), &["--json", "--yes"]).failure();
}

#[test]
fn format_check_passes_when_already_formatted() {
    let dir = tempdir().unwrap();
    healthy_repo(dir.path());

    format_cmd(dir.path(), &["--check"]).success();
}

#[test]
fn format_check_fails_when_files_would_change() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    // Reversed order is not the deterministic ordering, so formatting is needed.
    write(&dir.path().join(".env.example"), "Z_VAR=1\nA_VAR=2\n");
    write(&dir.path().join(".env"), "Z_VAR=1\nA_VAR=2\n");
    write(
        &dir.path().join("src/env.private.ts"),
        r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    Z_VAR: z.string(),
    A_VAR: z.string(),
  },
});
"#,
    );

    format_cmd(dir.path(), &["--check"]).failure();
}

#[test]
fn fix_yes_check_reevaluates_remaining_findings() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(&dir.path().join(".env.example"), "Z_VAR=1\nA_VAR=2\n");
    write(&dir.path().join(".env"), "Z_VAR=1\nA_VAR=2\n");
    write(
        &dir.path().join("src/env.private.ts"),
        r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    Z_VAR: z.string(),
    A_VAR: z.string(),
  },
});
"#,
    );

    // Only fixable formatting warnings: --fix --yes --check must re-evaluate and pass.
    doctor(dir.path(), &["--fix", "--yes", "--check"]).success();
    // Files are now formatted, so a follow-up check also passes.
    format_cmd(dir.path(), &["--check"]).success();
}

#[test]
fn fix_yes_check_still_fails_on_unfixable_error() {
    let dir = tempdir().unwrap();
    error_repo(dir.path());

    // The public-strict error has no automatic fix, so re-check must still fail.
    doctor(dir.path(), &["--fix", "--yes", "--check"]).failure();
}
