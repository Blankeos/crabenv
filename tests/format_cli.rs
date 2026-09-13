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

fn format_cmd(root: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut cmd = Command::cargo_bin("crabenv").unwrap();
    cmd.arg("--root").arg(root);
    cmd.arg("format");
    for arg in args {
        cmd.arg(arg);
    }
    cmd.assert()
}

fn read_example(root: &Path) -> String {
    fs::read_to_string(root.join(".env.example")).unwrap()
}

// ---------------------------------------------------------------------------
// Attached comments / decorators
// ---------------------------------------------------------------------------

#[test]
fn attached_comments_move_with_their_entry() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    // Z_OTHER is first so B/A comments are non-leading and must travel.
    write(
        &dir.path().join(".env.example"),
        "Z_OTHER=9\n# Comment for B\nB_VAL=2\n# Comment for A\nA_VAL=1\n",
    );

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "# Comment for A\nA_VAL=1\n\n# Comment for B\nB_VAL=2\n\nZ_OTHER=9\n"
    );
    // Idempotent.
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn file_header_separated_by_blank_stays_at_top() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# File header docs\n# More header\n\nZ_VAL=1\nA_VAL=2\n",
    );

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "# File header docs\n# More header\n\nA_VAL=2\n\nZ_VAL=1\n"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn leading_comments_before_first_entry_travel_with_first_entry() {
    // Contiguous comments immediately before the first entry travel with
    // that entry when it sorts elsewhere. Only blank-separated headers stay
    // at the top (see `file_header_separated_by_blank_stays_at_top`).
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# Direct header\nZ_VAL=1\nA_VAL=2\n",
    );

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_VAL=2\n\n# Direct header\nZ_VAL=1\n"
    );
    // Idempotent: second run keeps the traveled comment with its entry.
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn first_entry_decorators_travel_with_first_entry() {
    // Same rule for decorators: contiguous `# @...` lines before the first
    // entry are attached, not file headers.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# @optional\n# Description for Z\nZ_VAL=1\nA_VAL=2\n",
    );

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_VAL=2\n\n# @optional\n# Description for Z\nZ_VAL=1\n"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn attached_comments_reordered_to_first_are_idempotent() {
    // Both the first entry (Z) and a later entry (A) carry attached
    // comments; after sorting A (with its comment) becomes first and a
    // second format is a no-op.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# Comment for Z\nZ_VAL=1\n# Comment for A\nA_VAL=2\n",
    );

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "# Comment for A\nA_VAL=2\n\n# Comment for Z\nZ_VAL=1\n"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn decorators_move_with_their_entry_when_not_first() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_OTHER=9\n# @optional\n# Description for B\nB_VAL=2\n# @required\n# Description for A\nA_VAL=1\n",
    );

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "# @required\n# Description for A\nA_VAL=1\n\n# @optional\n# Description for B\nB_VAL=2\n\nZ_OTHER=9\n"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

// ---------------------------------------------------------------------------
// Multiline values
// ---------------------------------------------------------------------------

#[test]
fn multiline_double_quoted_value_never_splits_and_sorts_as_unit() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_AFTER=done\nPRIVATE_KEY=\"-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7\nFOO=bar\n# ---- shared ----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC8\n-----END PRIVATE KEY-----\"\nA_FIRST=hello\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    // assignment-like and header-like lines stay inside the quoted value.
    assert!(
        out.contains("FOO=bar\n# ---- shared ----"),
        "multiline must not split, got:\n{out}"
    );
    assert!(out.contains("-----BEGIN PRIVATE KEY-----"));
    assert!(out.contains("-----END PRIVATE KEY-----\""));
    // Sorted as a unit: A_FIRST first, PRIVATE_KEY middle, Z_AFTER last.
    assert!(out.starts_with("A_FIRST=hello"));
    assert!(out.trim_end().ends_with("Z_AFTER=done"));
    // The assignment-like line is inside the value, not a separate entry:
    // it must appear glued between the PEM markers, which the contains()
    // check above already proves. (Splitting `out.lines()` cannot tell
    // interior lines from entries, so no line-equality assertion here.)
    // Idempotent.
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn multiline_single_quoted_value_never_splits() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_AFTER=done\nSINGLE='line1\nFOO=bar\n# ---- shared ----\nline4'\nA_FIRST=1\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    assert!(
        out.contains("FOO=bar\n# ---- shared ----"),
        "single-quoted multiline must not split, got:\n{out}"
    );
    assert!(out.starts_with("A_FIRST=1"));
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn commented_multiline_keeps_every_continuation_commented() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_AFTER=done\n# OPTIONAL_KEY=\"line1\n# line2\n# line3\"\nA_FIRST=1\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    assert!(
        out.contains("# OPTIONAL_KEY=\"line1\n# line2\n# line3\""),
        "commented multiline must round-trip, got:\n{out}"
    );
    for line in out.lines() {
        if line.contains("line1") || line.contains("line2") || line.contains("line3") {
            assert!(
                line.trim_start().starts_with('#'),
                "every continuation must stay commented, got: {line:?} in:\n{out}"
            );
        }
    }
    assert!(!out.lines().any(|line| line == "OPTIONAL_KEY=\"line1"));
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn header_like_line_inside_quotes_is_not_a_section() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# ---- shared ----\nDATABASE_URL=file:./db\nPRIVATE_KEY=\"line1\n# ---- apps/web/.env.example ----\nline3\"\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    // Fake header stays glued inside the value.
    assert!(
        out.contains("# ---- apps/web/.env.example ----\nline3\""),
        "fake header must stay inside value, got:\n{out}"
    );
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

// ---------------------------------------------------------------------------
// Duplicates
// ---------------------------------------------------------------------------

#[test]
fn duplicate_active_values_preserve_relative_order_and_values() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    // PORT is a standard var so it sorts before DATABASE_URL; the two PORT
    // lines must stay in input order (last-wins preserved) with values exact.
    write(
        &dir.path().join(".env.example"),
        "DATABASE_URL=file:./db\nPORT=3000\nPORT=3001\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    assert_eq!(out, "PORT=3000\nPORT=3001\n\nDATABASE_URL=file:./db\n");
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn duplicate_commented_and_active_entries_stay_verbatim() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "DATABASE_URL=file:./db\nPORT=3000\n# PORT=3001\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    // Active PORT and its commented duplicate travel together, values exact,
    // commented line stays commented (inactive for real loaders).
    assert_eq!(out, "PORT=3000\n# PORT=3001\n\nDATABASE_URL=file:./db\n");
    assert!(!out.lines().any(|line| line == "PORT=3001"));
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn duplicate_commented_options_all_preserved() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# local port options\n# PORT=3000\n# PORT=3001\nDATABASE_URL=file:./db\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    assert!(out.contains("# PORT=3000\n# PORT=3001"), "got:\n{out}");
    assert!(!out.lines().any(|line| line == "PORT=3000"));
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn cross_section_active_duplicates_shared_vs_app_are_left_unchanged() {
    // Same active key in reordered sections: sorting `shared` before
    // `apps/web` would flip last-wins (`shared_value` last in input, but
    // `web_value` last after sort), so the formatter must leave the file
    // byte-identical instead of guessing.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "# ---- apps/web/.env.example ----\nDUP_KEY=web_value\n# ---- shared ----\nDUP_KEY=shared_value\n";
    write(&dir.path().join(".env.example"), before);

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        before,
        "cross-section active duplicate must be preserved unchanged"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
    // Already formatted (no-op) so --check passes without mutation.
    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), before);
}

#[test]
fn cross_section_active_duplicates_between_apps_are_left_unchanged() {
    // `apps/api` sorts before `apps/web`; input is web-then-api so sorting
    // would change last-wins from `api_value` to `web_value`.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "# ---- apps/web/.env.example ----\nDUP_KEY=web_value\n# ---- apps/api/.env.example ----\nDUP_KEY=api_value\n";
    write(&dir.path().join(".env.example"), before);

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        before,
        "app-vs-app active duplicate must be preserved unchanged"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), before);
}

#[test]
fn cross_section_active_duplicates_with_local_only_are_left_unchanged() {
    // `local-only` sorts after app sections; input is local-only-then-app
    // so sorting would flip last-wins. Local-only promotion must not run
    // either: the file stays byte-identical.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "# ---- local-only ----\nDUP_KEY=local_value\n# ---- apps/web/.env.example ----\nDUP_KEY=web_value\n";
    write(&dir.path().join(".env.example"), before);

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        before,
        "local-only active duplicate must be preserved unchanged"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), before);
}

// ---------------------------------------------------------------------------
// Interpolation / templates: never expand, unsafe order preserved
// ---------------------------------------------------------------------------

#[test]
fn variable_interpolation_is_never_expanded_and_preserved_in_place() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    // Z_VAL before A_VAL is the dependency order for sequential-expansion
    // loaders; alphabetical sorting would put A_VAL first and break it, so
    // the formatter must preserve input unchanged (conservative no-op).
    let before = "Z_VAL=hello\nA_VAL=${Z_VAL}_suffix\n";
    write(&dir.path().join(".env.example"), before);

    // Already formatted (no-op) so plain format changes nothing...
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        before,
        "interpolation file must be preserved in place, not reordered"
    );
    // ...and --check passes without mutation (no drift to report).
    let snapshot = read_example(dir.path());
    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), snapshot);
}

#[test]
fn unquoted_and_double_quoted_interpolation_are_both_unsafe() {
    for value in [
        "$Z_VAL_suffix",
        "\"${Z_VAL}_world\"",
        "\"prefix $Z_VAL suffix\"",
    ] {
        let dir = tempdir().unwrap();
        package_json(dir.path());
        let before = format!("Z_VAL=hello\nB_VAL={value}\nA_VAL=1\n");
        write(&dir.path().join(".env.example"), &before);
        format_cmd(dir.path(), &[]).success();
        assert_eq!(
            read_example(dir.path()),
            before,
            "value {value:?} must trigger conservative no-op"
        );
    }
}

#[test]
fn apostrophe_in_unquoted_value_does_not_hide_interpolation() {
    // `don't $Z_VAL` contains an apostrophe (unclosed single quote) plus an
    // unsafe `$` ref. The `'` is a literal, not a quoting delimiter, so the
    // file must be left unchanged (dependency order Z before A preserved).
    // `$(...)` after an apostrophe stays safe; a bare apostrophe with no `$`
    // also stays sortable.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "Z_VAL=hello\nA_VAL=don't $Z_VAL\n";
    write(&dir.path().join(".env.example"), before);
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        before,
        "apostrophe must not hide $ interpolation"
    );

    // Nested quotes are handled precisely: single-outside/double-inside is
    // a literal (sortable), double-outside/single-inside still expands
    // (no-op). These cases already passed before the apostrophe fix.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_VAL=hello\nA_VAL='\"$BAR\"'\n",
    );
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_VAL='\"$BAR\"'\n\nZ_VAL=hello\n"
    );

    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "Z_VAL=hello\nA_VAL=\"'$BAR'\"\n";
    write(&dir.path().join(".env.example"), before);
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), before);
}

#[test]
fn command_templates_single_quoted_and_escaped_dollars_remain_sortable() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    // `$(...)` has no intra-file dependency; single-quoted `$` and escaped
    // `\$` are literals. All must still sort with values byte-identical.
    write(
        &dir.path().join(".env.example"),
        "Z_VAR=1\nSESSION_SECRET=\"$(openssl rand -base64 32)\"\nA_VAR=2\n",
    );
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_VAR=2\n\nSESSION_SECRET=\"$(openssl rand -base64 32)\"\n\nZ_VAR=1\n"
    );

    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_VAL=hello\nA_VAL='literal ${Z_VAL}'\n",
    );
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_VAL='literal ${Z_VAL}'\n\nZ_VAL=hello\n"
    );

    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_VAL=hello\nA_VAL=\\${Z_VAL}\n",
    );
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_VAL=\\${Z_VAL}\n\nZ_VAL=hello\n"
    );
}

#[test]
fn commented_interpolation_and_hash_in_trailing_comment_stay_sortable() {
    // Inactive (`# ...`) interpolation and `$` inside ` # comment` are not
    // part of any active value, so sorting remains safe.
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_VAL=hello\n# A_VAL=${Z_VAL}_suffix\nA_OTHER=1\n",
    );
    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        "A_OTHER=1\n# A_VAL=${Z_VAL}_suffix\n\nZ_VAL=hello\n"
    );

    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_VAR=1 # costs $5\nA_VAR=2\n",
    );
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), "A_VAR=2\n\nZ_VAR=1 # costs $5\n");
}

// ---------------------------------------------------------------------------
// Unsafe-to-reorder shell/loose syntax + malformed values
// ---------------------------------------------------------------------------

#[test]
fn export_and_loose_lines_are_preserved_in_place() {
    for before in [
        "B=2\nexport EXPORTED=1\nA=1\n",
        "B=2\nHELLO_WORLD\nA=1\n",
        "B=2\nFOO: bar\nA=1\n",
    ] {
        let dir = tempdir().unwrap();
        package_json(dir.path());
        write(&dir.path().join(".env.example"), before);
        format_cmd(dir.path(), &[]).success();
        assert_eq!(
            read_example(dir.path()),
            before,
            "loose/shell line must be preserved in place via no-op, input was {before:?}"
        );
        let snapshot = read_example(dir.path());
        format_cmd(dir.path(), &["--check"]).success();
        assert_eq!(read_example(dir.path()), snapshot);
    }
}

#[test]
fn malformed_unterminated_quote_is_never_silently_corrupted() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "FOO=\"oops\nBAR=value\n";
    write(&dir.path().join(".env.example"), before);

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        read_example(dir.path()),
        before,
        "malformed active quote must be preserved unchanged, not reformatted away"
    );
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn unclosed_commented_quote_does_not_swallow_active_vars() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "# FOO=\"oops\nBAR=value\nA_FIRST=1\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    assert!(
        out.contains("BAR=value"),
        "BAR must survive formatting, got:\n{out}"
    );
    assert!(
        out.contains("A_FIRST=1"),
        "A_FIRST must survive formatting, got:\n{out}"
    );
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

// ---------------------------------------------------------------------------
// Idempotence + --check no-mutation contract
// ---------------------------------------------------------------------------

#[test]
fn format_twice_leaves_file_unchanged() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_VAR=1\n# Comment for B\nB_VAR=2\n# Comment for A\nA_VAR=3\n",
    );

    format_cmd(dir.path(), &[]).success();
    let once = read_example(dir.path());
    format_cmd(dir.path(), &[]).success();
    let twice = read_example(dir.path());
    assert_eq!(once, twice, "format must be idempotent");
    // Third run for good measure (catches oscillation between two states).
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), twice);
}

#[test]
fn format_check_errors_on_drift_without_mutating() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "Z_VAR=1\nA_VAR=2\n";
    write(&dir.path().join(".env.example"), before);

    // Drift: --check must fail...
    format_cmd(dir.path(), &["--check"]).failure();
    // ...without writing anything.
    assert_eq!(
        read_example(dir.path()),
        before,
        "--check must never mutate files"
    );

    // Real format fixes the drift.
    format_cmd(dir.path(), &[]).success();
    let formatted = read_example(dir.path());
    assert_ne!(formatted, before);
    assert_eq!(formatted, "A_VAR=2\n\nZ_VAR=1\n");

    // Now --check passes and still mutates nothing.
    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), formatted);
    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), formatted);
}

#[test]
fn format_check_passes_without_mutating_already_formatted_file() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    let before = "A_VAR=2\n\nZ_VAR=1\n";
    write(&dir.path().join(".env.example"), before);

    format_cmd(dir.path(), &["--check"]).success();
    assert_eq!(read_example(dir.path()), before);
}

#[test]
fn values_with_hashes_quotes_and_placeholders_are_byte_identical() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(
        &dir.path().join(".env.example"),
        "Z_LATE=last\nS3_BUCKET_NAME=\"solid-launch\" # Create this bucket locally\nRAW=value#literal\nRESEND_FROM=\"Team <team@example.com>\"\nA_FIRST=1\n",
    );

    format_cmd(dir.path(), &[]).success();
    let out = read_example(dir.path());
    // Raw values preserved verbatim (inline ` # comment` travels with its
    // line; `value#literal` keeps its hash).
    assert!(out.contains("S3_BUCKET_NAME=\"solid-launch\" # Create this bucket locally"));
    assert!(out.contains("RAW=value#literal"));
    assert!(out.contains("RESEND_FROM=\"Team <team@example.com>\""));
    let once = out;
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), once);
}

#[test]
fn format_preserves_local_env_file_too() {
    let dir = tempdir().unwrap();
    package_json(dir.path());
    write(&dir.path().join(".env.example"), "A_VAR=2\n\nZ_VAR=1\n");
    write(&dir.path().join(".env"), "Z_VAR=1\nA_VAR=2\n");

    format_cmd(dir.path(), &[]).success();
    assert_eq!(
        fs::read_to_string(dir.path().join(".env")).unwrap(),
        "A_VAR=2\n\nZ_VAR=1\n"
    );
    // Both files now formatted: second run changes nothing.
    let example_once = read_example(dir.path());
    let local_once = fs::read_to_string(dir.path().join(".env")).unwrap();
    format_cmd(dir.path(), &[]).success();
    assert_eq!(read_example(dir.path()), example_once);
    assert_eq!(
        fs::read_to_string(dir.path().join(".env")).unwrap(),
        local_once
    );
    format_cmd(dir.path(), &["--check"]).success();
}
