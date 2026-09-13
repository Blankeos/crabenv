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

fn single_var_repo(root: &Path) {
    write(
        &root.join("package.json"),
        r#"{"dependencies":{"zod":"latest"}}"#,
    );
    write(&root.join(".env.example"), "DATABASE_URL=file:./local.db\n");
    write(&root.join(".env"), "DATABASE_URL=file:./local.db\n");
    write(
        &root.join("src/env.private.ts"),
        r#"import { createEnv } from "@t3-oss/env-core";
import { z } from "zod";

export const privateEnv = createEnv({
  runtimeEnv: process.env,
  server: {
    DATABASE_URL: z.string(),
  },
});
"#,
    );
}

fn ls(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("crabenv").unwrap();
    cmd.arg("--root").arg(root).arg("ls").args(args);
    cmd.assert()
}

#[test]
fn ls_print_lists_definition_variables_for_scripts() {
    let dir = tempdir().unwrap();
    single_var_repo(dir.path());

    let output = ls(dir.path(), &["-p"])
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(
        stdout.contains("DATABASE_URL"),
        "ls -p should list the variable, got:\n{stdout}"
    );
}

#[test]
fn ls_without_print_falls_back_to_table_when_noninteractive() {
    let dir = tempdir().unwrap();
    single_var_repo(dir.path());

    let output = ls(dir.path(), &[]).success().get_output().stdout.clone();
    let stdout = String::from_utf8(output).unwrap();
    assert!(
        stdout.contains("DATABASE_URL"),
        "bare ls should fall back to the plain table when not a TTY, got:\n{stdout}"
    );
}

#[test]
fn ls_print_json_lists_variables_as_machine_readable_rows() {
    let dir = tempdir().unwrap();
    single_var_repo(dir.path());

    let output = ls(dir.path(), &["-p", "--json"])
        .success()
        .get_output()
        .stdout
        .clone();
    let value: serde_json::Value =
        serde_json::from_slice(&output).expect("ls -p --json stdout must be pure JSON");
    let rows = value.as_array().expect("ls --json must be a row array");
    assert!(
        rows.iter()
            .any(|row| row.get("name").unwrap() == "DATABASE_URL"),
        "expected DATABASE_URL row, got {value}"
    );
}

#[test]
fn ls_json_requires_print_to_protect_noninteractive_use() {
    let dir = tempdir().unwrap();
    single_var_repo(dir.path());

    ls(dir.path(), &["--json"]).failure();
}
