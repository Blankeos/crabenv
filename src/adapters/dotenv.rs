use anyhow::{bail, Result};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::models::{DotenvEntry, Project, Scope, SourceKind, VarSource, Workspace};
use crate::ordering::{compare_env_names, env_group};
use crate::util::is_valid_var_name;

pub fn example_path(workspace: &Workspace) -> PathBuf {
    workspace.root.join(".env.example")
}

/// A logical dotenv line: one assignment (possibly multiline quoted) or one
/// single physical line. `text` joins the physical lines with `\n` verbatim so
/// quoted values keep embedded newlines; `start_line` retains the starting
/// physical 1-based line number for diagnostics.
#[derive(Clone, Debug)]
struct LogicalLine {
    text: String,
    start_line: usize,
}

/// Split file contents into logical lines, grouping continuation lines that
/// belong to an unclosed single/double quoted assignment value. This keeps
/// PEM-like values (which may contain lines resembling `FOO=bar` or section
/// headers like `# ---- shared ----`) together so later passes never split or
/// rearrange inside values.
///
/// Standard loader semantics (Node `dotenv`, `python-dotenv`, `dotenvy`):
/// lines beginning with `#` are comments and never start a multiline block
/// that can swallow following active lines. A commented (`# KEY="...`)
/// multiline is only grouped when *every* continuation physical line is
/// itself commented (`# ...`); the parser then strips exactly one added
/// `# `/`#` prefix per continuation. An unclosed commented quote followed by
/// an active line (e.g. `# FOO="oops` + `BAR=value`) stays two logical
/// lines so `BAR` remains active.
///
/// A malformed *active* unterminated quote is an error; callers propagate it
/// before any mutation so trailing vars are never dropped via `remove`.
fn split_logical_lines(contents: &str) -> Result<Vec<LogicalLine>> {
    let physical: Vec<&str> = contents.lines().collect();
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < physical.len() {
        let line = physical[idx];
        let start = idx + 1;
        if let Some(open) = opening_quote_for_line(line) {
            if is_commented_line(line) {
                // Only group when the whole continuation run is commented.
                let mut end: Option<usize> = None;
                for (next, cont) in physical.iter().enumerate().skip(idx + 1) {
                    if !is_commented_line(cont) {
                        break;
                    }
                    let stripped = strip_one_comment_prefix(cont).unwrap_or_default();
                    if line_contains_closing_quote(&stripped, open) {
                        end = Some(next);
                        break;
                    }
                }
                if let Some(last) = end {
                    out.push(LogicalLine {
                        text: physical[idx..=last].join("\n"),
                        start_line: start,
                    });
                    idx = last + 1;
                } else {
                    // Unclosed commented quote: never swallow following lines.
                    out.push(LogicalLine {
                        text: line.to_string(),
                        start_line: start,
                    });
                    idx += 1;
                }
            } else if let Some(last) = find_active_closing(&physical, idx, open) {
                out.push(LogicalLine {
                    text: physical[idx..=last].join("\n"),
                    start_line: start,
                });
                idx = last + 1;
            } else {
                let hint = parse_entry_key(line)
                    .map(|(key, _)| key)
                    .unwrap_or_else(|| line.trim().to_string());
                bail!(
                    "unterminated quoted value for '{hint}' starting at line {start}: missing closing '{open}'"
                );
            }
        } else {
            out.push(LogicalLine {
                text: line.to_string(),
                start_line: start,
            });
            idx += 1;
        }
    }
    Ok(out)
}

fn find_active_closing(physical: &[&str], start: usize, quote: char) -> Option<usize> {
    for (offset, line) in physical.iter().enumerate().skip(start + 1) {
        if line_contains_closing_quote(line, quote) {
            return Some(offset);
        }
    }
    None
}

fn is_commented_line(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// Strip exactly one added comment prefix (`# `, `#\t` or `#`) from a
/// continuation line. Leading indent before `#` is ignored; a single content
/// indent after `# ` is preserved so value leading spaces round-trip.
fn strip_one_comment_prefix(line: &str) -> Option<String> {
    let rest = line.trim_start().strip_prefix('#')?;
    if let Some(stripped) = rest.strip_prefix(' ') {
        Some(stripped.to_string())
    } else if let Some(stripped) = rest.strip_prefix('\t') {
        Some(stripped.to_string())
    } else {
        Some(rest.to_string())
    }
}

/// Render a (possibly multiline) assignment as commented by prefixing *every*
/// physical line with `# ` (bare `#` for empty lines) so real dotenv loaders
/// keep all continuations inactive.
fn comment_assignment(assignment: &str) -> String {
    assignment
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                "#".to_string()
            } else {
                format!("# {line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// If `line` starts an assignment whose quoted value is unclosed on this
/// physical line, return the opening quote char. Otherwise return `None`.
fn opening_quote_for_line(line: &str) -> Option<char> {
    if is_section_header(line) {
        return None;
    }
    let trimmed_start = line.trim_start();
    if trimmed_start.is_empty() {
        return None;
    }
    let value_part: &str = if let Some(stripped) = trimmed_start.strip_prefix('#') {
        let body = stripped.trim_start();
        if body.is_empty() || body.starts_with('#') {
            return None;
        }
        let body = if let Some(rest) = body.strip_prefix("export ") {
            rest.trim_start()
        } else {
            body
        };
        let (_, value) = body.split_once('=')?;
        value
    } else {
        let rest = if let Some(stripped) = trimmed_start.strip_prefix("export ") {
            stripped.trim_start()
        } else {
            trimmed_start
        };
        let (_, value) = rest.split_once('=')?;
        value
    };
    let value_trimmed = value_part.trim_start();
    let open = value_trimmed.chars().next()?;
    if open != '"' && open != '\'' {
        return None;
    }
    let mut escaped = false;
    let mut chars = value_trimmed.chars();
    chars.next(); // skip opening quote
    for ch in chars {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == open {
            return None;
        }
    }
    Some(open)
}

fn line_contains_closing_quote(line: &str, quote: char) -> bool {
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == quote {
            return true;
        }
    }
    false
}

fn quote_double_preserving_newlines(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\r' => out.push_str("\\r"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn quote_single_preserving_newlines(value: &str) -> String {
    if value.contains('\r') {
        return quote_double_preserving_newlines(value);
    }
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        match ch {
            '\'' => out.push_str("\\'"),
            '\\' => out.push_str("\\\\"),
            _ => out.push(ch),
        }
    }
    out.push('\'');
    out
}

fn parse_entry_key(line: &str) -> Option<(String, bool)> {
    parse_assignment_key(line)
        .map(|key| (key, false))
        .or_else(|| parse_commented_assignment_key(line).map(|key| (key, true)))
}

fn parse_commented_assignment_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let body = trimmed.strip_prefix('#')?.trim_start();
    if body.starts_with('#') || body.starts_with("export ") {
        return None;
    }
    let (key, _) = body.split_once('=')?;
    let key = key.trim();
    is_valid_var_name(key).then(|| key.to_string())
}

#[derive(Clone, Debug)]
struct RawSection {
    header: Option<String>,
    lines: Vec<String>,
    original_index: usize,
}

#[derive(Clone, Debug)]
struct DotenvEntryBlock {
    key: String,
    lines: Vec<String>,
    original_index: usize,
}

#[derive(Clone, Debug)]
struct ParsedSection {
    header: Option<String>,
    leading: Vec<String>,
    loose: Vec<Vec<String>>,
    entries: Vec<DotenvEntryBlock>,
}

fn parse_sections(contents: &str) -> Result<Vec<RawSection>> {
    let mut sections = vec![RawSection {
        header: None,
        lines: Vec::new(),
        original_index: 0,
    }];

    for logical in split_logical_lines(contents)? {
        let line = logical.text;
        if is_section_header(&line) {
            sections.push(RawSection {
                header: Some(line),
                lines: Vec::new(),
                original_index: sections.len(),
            });
        } else if let Some(section) = sections.last_mut() {
            section.lines.push(line);
        }
    }

    Ok(sections)
}

fn render_section(section: RawSection) -> Vec<String> {
    let parsed = parse_section_body(section);
    let mut output = Vec::new();

    if let Some(header) = parsed.header {
        output.push(header);
    }

    push_lines(&mut output, parsed.leading);

    for loose in parsed.loose {
        if !output.is_empty() && !output.last().is_some_and(|line| line.is_empty()) {
            output.push(String::new());
        }
        push_lines(&mut output, loose);
    }

    let mut entries = parsed.entries;
    entries.sort_by(|left, right| {
        compare_env_names(&left.key, &right.key)
            .then_with(|| left.original_index.cmp(&right.original_index))
    });

    let mut previous_group: Option<String> = None;
    for entry in entries {
        let group = env_group(&entry.key);
        if previous_group
            .as_ref()
            .is_some_and(|previous| previous != &group)
            && !output.is_empty()
            && !output.last().is_some_and(|line| line.is_empty())
        {
            output.push(String::new());
        }
        push_lines(&mut output, entry.lines);
        previous_group = Some(group);
    }

    trim_blank_edges(output)
}

fn parse_section_body(section: RawSection) -> ParsedSection {
    let mut leading = Vec::new();
    let mut loose = Vec::new();
    let mut entries = Vec::new();
    let mut pending = Vec::<String>::new();
    let mut saw_entry_or_loose = false;

    for line in section.lines {
        if let Some((key, _commented)) = parse_entry_key(&line) {
            let mut lines = Vec::new();
            if saw_entry_or_loose {
                lines.append(&mut pending);
            } else if !pending.is_empty() {
                leading.append(&mut pending);
            }
            lines.push(line);
            entries.push(DotenvEntryBlock {
                key,
                lines,
                original_index: entries.len(),
            });
            saw_entry_or_loose = true;
            continue;
        }

        if line.trim().is_empty() {
            if !saw_entry_or_loose {
                if !pending.is_empty() {
                    leading.append(&mut pending);
                }
                if !leading.is_empty() && !leading.last().is_some_and(|line| line.is_empty()) {
                    leading.push(String::new());
                }
            } else if !pending.is_empty() {
                loose.push(std::mem::take(&mut pending));
            }
            continue;
        }

        if line.trim_start().starts_with('#') {
            pending.push(line);
            continue;
        }

        if !pending.is_empty() {
            loose.push(std::mem::take(&mut pending));
        }
        loose.push(vec![line]);
        saw_entry_or_loose = true;
    }

    if !pending.is_empty() {
        if saw_entry_or_loose {
            loose.push(pending);
        } else {
            leading.extend(pending);
        }
    }

    ParsedSection {
        header: section.header,
        leading,
        loose,
        entries,
    }
}

fn parse_assignment_key(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("export ") {
        return None;
    }
    let (key, _) = trimmed.split_once('=')?;
    let key = key.trim();
    is_valid_var_name(key).then(|| key.to_string())
}

fn is_section_header(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('#') {
        return false;
    }
    let body = trimmed.trim_start_matches('#').trim();
    body.starts_with("---") && body.ends_with("---")
}

fn compare_sections(left: &RawSection, right: &RawSection) -> std::cmp::Ordering {
    let left_key = section_sort_key(left);
    let right_key = section_sort_key(right);
    left_key.cmp(&right_key)
}

fn section_sort_key(section: &RawSection) -> (u8, String, usize) {
    let label = section
        .header
        .as_deref()
        .map(section_label)
        .unwrap_or_default();
    let lower = label.to_ascii_lowercase();
    let rank = if label.is_empty() && section.lines.iter().all(|line| line.trim().is_empty()) {
        4
    } else if section.header.is_none() {
        0
    } else if lower == "shared" {
        1
    } else if lower.starts_with("apps/") {
        2
    } else if section.header.is_some() {
        3
    } else {
        4
    };
    (rank, lower, section.original_index)
}

fn section_label(line: &str) -> String {
    line.trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('-')
        .trim()
        .to_string()
}

fn push_lines(output: &mut Vec<String>, lines: Vec<String>) {
    for line in lines {
        if line.is_empty() && output.last().is_some_and(|last| last.is_empty()) {
            continue;
        }
        output.push(line);
    }
}

fn trim_blank_edges(mut lines: Vec<String>) -> Vec<String> {
    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_quoted_backslash_r_remains_literal() {
        assert_eq!(parse_line(r"PATH_VALUE='C:\repo'").unwrap().1, r"C:\repo");
    }

    #[test]
    fn format_contents_preserves_comments_and_placeholder_values() {
        let input = r#"# Defaults/docs live in schemas
RESEND_FROM="Team <team@example.com>"
S3_SECRET_ACCESS_KEY="secret"
NODE_ENV="development"
# Bucket comment
S3_BUCKET_NAME="bucket"
RESEND_API_KEY=""
SESSION_SECRET="$(openssl rand -base64 32)"
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"# Defaults/docs live in schemas
NODE_ENV="development"

RESEND_API_KEY=""
RESEND_FROM="Team <team@example.com>"

# Bucket comment
S3_BUCKET_NAME="bucket"
S3_SECRET_ACCESS_KEY="secret"

SESSION_SECRET="$(openssl rand -base64 32)"
"#
        );
    }

    #[test]
    fn format_contents_keeps_root_example_sections() {
        let input = r#"# ---- apps/web/.env.example ----
S3_BUCKET=web
NODE_ENV=development
# ---- shared ----
DATABASE_URL=file:./db
# ---- apps/api/.env.example ----
RESEND_API_KEY=
CI=true
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"# ---- shared ----
DATABASE_URL=file:./db

# ---- apps/api/.env.example ----
CI=true

RESEND_API_KEY=

# ---- apps/web/.env.example ----
NODE_ENV=development

S3_BUCKET=web
"#
        );
    }

    #[test]
    fn format_contents_preserves_commented_assignment_duplicates() {
        let input = r#"# local port options
# PORT=3000
# PORT=3001
DATABASE_URL=file:./db
# TOKEN=
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"# local port options
# PORT=3000
# PORT=3001

DATABASE_URL=file:./db

# TOKEN=
"#
        );
    }

    #[test]
    fn format_contents_keeps_commented_and_active_same_key_together() {
        let input = r#"# DATABASE_URL=""
API_KEY=api
DATABASE_URL=""
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"API_KEY=api

# DATABASE_URL=""
DATABASE_URL=""
"#
        );
    }

    #[test]
    fn format_contents_repairs_local_only_duplicate_of_commented_template() {
        let input = r#"# DATABASE_URL="file:/tmp/local.db"

# --- local-only ---

DATABASE_URL="file:/tmp/local.db"
"#;

        let formatted = format_contents(input);

        assert_eq!(
            formatted,
            r#"# DATABASE_URL="file:/tmp/local.db"
DATABASE_URL="file:/tmp/local.db"
"#
        );
    }

    #[test]
    fn parse_file_marks_commented_assignments_without_activating_them() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        fs::write(&path, "# PORT=3000\nTOKEN=secret\n# plain comment\n").unwrap();

        let entries = parse_file(&path).unwrap();
        let active = parse_active_file(&path).unwrap();

        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "PORT");
        assert!(entries[0].commented);
        assert_eq!(entries[1].key, "TOKEN");
        assert!(!entries[1].commented);
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].key, "TOKEN");
    }

    #[test]
    fn parse_file_preserves_quote_style_and_ignores_inline_comments() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        fs::write(
            &path,
            "S3_BUCKET_NAME=\"solid-launch\" # Create this bucket locally\nRAW=value#literal\n",
        )
        .unwrap();

        let entries = parse_file(&path).unwrap();

        assert_eq!(entries[0].key, "S3_BUCKET_NAME");
        assert_eq!(entries[0].value, "solid-launch");
        assert_eq!(entries[0].quote, Some('"'));
        assert_eq!(entries[1].value, "value#literal");
    }

    #[test]
    fn example_sources_include_commented_assignments_as_template_surface() {
        let tempdir = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: tempdir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };
        fs::write(
            tempdir.path().join(".env.example"),
            "# NODE_ENV=development\n# PORT=3000\n",
        )
        .unwrap();

        let sources = collect_example(&workspace).unwrap();

        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "NODE_ENV");
        assert_eq!(sources[0].value.as_deref(), Some("development"));
        assert_eq!(sources[1].name, "PORT");
        assert_eq!(sources[1].value.as_deref(), Some("3000"));
    }

    #[test]
    fn local_sources_ignore_commented_assignments() {
        let tempdir = tempfile::tempdir().unwrap();
        let workspace = Workspace {
            root: tempdir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };
        let project = Project {
            root: tempdir.path().to_path_buf(),
            is_monorepo: false,
            workspaces: vec![workspace.clone()],
        };
        fs::write(tempdir.path().join(".env"), "# PORT=3000\nTOKEN=secret\n").unwrap();

        let sources = collect_local(&project, &workspace).unwrap();

        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "TOKEN");
    }

    #[test]
    fn remove_key_removes_commented_assignments_too() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        fs::write(
            &path,
            "# PORT=3000\nTOKEN=secret\n# plain comment\n# PORT=3001\n",
        )
        .unwrap();

        remove_key(&path, "PORT").unwrap();

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "TOKEN=secret\n# plain comment\n"
        );
    }

    fn multiline_pem_fixture() -> String {
        "PRIVATE_KEY=\"-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC7\nFOO=bar\n# ---- shared ----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQC8\n-----END PRIVATE KEY-----\"\nOTHER=hello\n"
            .to_string()
    }

    #[test]
    fn parse_multiline_pem_keeps_full_value_and_line_numbers() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        fs::write(&path, multiline_pem_fixture()).unwrap();

        let entries = parse_file(&path).unwrap();
        assert_eq!(entries.len(), 2, "continuation FOO=bar must not split");
        assert_eq!(entries[0].key, "PRIVATE_KEY");
        assert_eq!(entries[0].quote, Some('"'));
        assert!(
            entries[0].value.contains("FOO=bar"),
            "value must keep assignment-like line, got {:?}",
            entries[0].value
        );
        assert!(
            entries[0].value.contains("# ---- shared ----"),
            "value must keep header-like line, got {:?}",
            entries[0].value
        );
        assert!(entries[0].value.contains('\n'));
        assert_eq!(entries[1].key, "OTHER");
        assert_eq!(entries[1].value, "hello");

        // Shared logical-line reader retains starting physical line numbers.
        let workspace = Workspace {
            root: tempdir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };
        let sources = collect_example(&workspace).unwrap();
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "PRIVATE_KEY");
        assert_eq!(sources[0].line, 1);
        assert_eq!(sources[1].name, "OTHER");
        assert_eq!(sources[1].line, 7);
        assert!(
            !sources.iter().any(|s| s.name == "FOO"),
            "FOO=bar inside quotes must not become a source"
        );

        // Single-quoted multiline behaves the same.
        let single = "SINGLE='line1\nFOO=bar\n# ---- shared ----\nline4'\nAFTER=1\n";
        fs::write(&path, single).unwrap();
        let entries = parse_file(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "SINGLE");
        assert_eq!(entries[0].quote, Some('\''));
        assert!(entries[0].value.contains("FOO=bar"));
        assert!(entries[0].value.contains("# ---- shared ----"));
        let sources = collect_example(&workspace).unwrap();
        assert_eq!(sources[0].line, 1);
        assert_eq!(sources[1].line, 5);
    }

    #[test]
    fn format_multiline_is_idempotent_and_never_rearranges_inside() {
        let before = multiline_pem_fixture();
        let once = format_contents(&before);
        // Full quoted value survives formatting verbatim.
        assert!(
            once.contains("FOO=bar\n# ---- shared ----"),
            "format must not split inside value, got:\n{once}"
        );
        assert!(once.contains("-----BEGIN PRIVATE KEY-----"));
        assert!(once.contains("-----END PRIVATE KEY-----\""));
        // No phantom entry was created from the continuation line.
        let reparsed = split_logical_lines(&once).unwrap().len();
        assert!(
            reparsed <= 4,
            "multiline must stay grouped, got {reparsed} logical lines in:\n{once}"
        );
        let twice = format_contents(&once);
        assert_eq!(once, twice, "format must be idempotent");

        // A header-like line inside quotes must not become a real section.
        let with_sections = "# ---- shared ----\nDATABASE_URL=file:./db\nPRIVATE_KEY=\"line1\n# ---- apps/web/.env.example ----\nline3\"\n";
        let formatted = format_contents(with_sections);
        assert_eq!(format_contents(&formatted), formatted);
        assert!(
            formatted.contains("# ---- apps/web/.env.example ----\nline3\"")
                || formatted.contains("\"line1\n# ---- apps/web/.env.example ----\nline3\""),
            "fake header must stay inside value, got:\n{formatted}"
        );
        // Only the real header counts as a section header when viewed as
        // logical lines; the fake one stays glued inside the quoted value.
        let logical_headers = split_logical_lines(&formatted)
            .unwrap()
            .into_iter()
            .filter(|l| is_section_header(&l.text))
            .count();
        assert_eq!(logical_headers, 1, "got:\n{formatted}");
        // And the value still round-trips as one entry.
        let tmp = tempfile::tempdir().unwrap();
        let tmp_path = tmp.path().join(".env.example");
        fs::write(&tmp_path, &formatted).unwrap();
        let reparsed_entries = parse_file(&tmp_path).unwrap();
        assert_eq!(reparsed_entries.len(), 2);
        assert!(reparsed_entries
            .iter()
            .find(|e| e.key == "PRIVATE_KEY")
            .unwrap()
            .value
            .contains("# ---- apps/web/.env.example ----"));
    }

    #[test]
    fn upsert_preserves_multiline_whole_entry_before_and_after() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        let before = multiline_pem_fixture();
        fs::write(&path, &before).unwrap();

        // Updating an unrelated key leaves the whole multiline block intact.
        upsert_example(&path, "OTHER", "world").unwrap();
        let after_other = fs::read_to_string(&path).unwrap();
        assert!(
            after_other.contains("FOO=bar\n# ---- shared ----"),
            "unrelated upsert must keep full value, got:\n{after_other}"
        );
        assert!(after_other.contains("OTHER=\"world\"") || after_other.contains("OTHER=world"));

        // Updating the multiline key replaces the whole entry, not one line.
        fs::write(&path, &before).unwrap();
        upsert_example(&path, "PRIVATE_KEY", "rotated").unwrap();
        let after_self = fs::read_to_string(&path).unwrap();
        assert!(
            !after_self.contains("FOO=bar"),
            "update must replace whole multiline entry, got:\n{after_self}"
        );
        assert!(
            !after_self.contains("BEGIN PRIVATE KEY"),
            "old PEM lines must be gone, got:\n{after_self}"
        );
        assert!(after_self.contains("PRIVATE_KEY=rotated"));
        assert!(after_self.contains("OTHER=hello"));
    }

    #[test]
    fn remove_multiline_removes_whole_entry_before_and_after() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        let before = multiline_pem_fixture();
        fs::write(&path, &before).unwrap();
        assert!(before.contains("FOO=bar"));

        remove_key(&path, "PRIVATE_KEY").unwrap();
        let after = fs::read_to_string(&path).unwrap();
        assert_eq!(
            after, "OTHER=hello\n",
            "remove must drop whole block, got:\n{after}"
        );
        assert!(!after.contains("FOO=bar"));
        assert!(!after.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn commented_optional_multiline_is_safe() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        // Every physical line carries `# ` so real dotenv loaders keep all
        // continuations inactive.
        let before = "# OPTIONAL_KEY=\"line1\n# line2\n# line3\"\nOTHER=hello\n";
        fs::write(&path, before).unwrap();

        let entries = parse_file(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "OPTIONAL_KEY");
        assert!(entries[0].commented);
        assert_eq!(entries[0].value, "line1\nline2\nline3");

        let formatted = format_contents(before);
        assert_eq!(format_contents(&formatted), formatted);
        assert!(
            formatted.contains("# OPTIONAL_KEY=\"line1\n# line2\n# line3\""),
            "commented multiline must round-trip, got:\n{formatted}"
        );
        for line in formatted.lines().filter(|line| {
            line.contains("line1") || line.contains("line2") || line.contains("line3")
        }) {
            assert!(
                line.trim_start().starts_with('#'),
                "every continuation must stay commented, got: {line:?} in:\n{formatted}"
            );
        }

        // Removing the commented multiline drops all its physical lines.
        remove_key(&path, "OPTIONAL_KEY").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "OTHER=hello\n");
    }

    #[test]
    fn commented_optional_multiline_all_lines_commented_loader_semantics() {
        // Standard loader check: an optional multiline whose continuations
        // look like assignments must not activate them when every line is
        // commented.
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        let before = "# OPTIONAL_KEY=\"line1\n# FOO=bar\n# line3\"\nOTHER=hello\n";
        fs::write(&path, before).unwrap();

        let entries = parse_file(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].key, "OPTIONAL_KEY");
        assert!(entries[0].commented);
        assert_eq!(entries[0].value, "line1\nFOO=bar\nline3");
        assert_eq!(entries[1].key, "OTHER");

        // Optional entries are inactive for real loaders.
        let active = parse_active_file(&path).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].key, "OTHER");

        // Every physical line of the optional block stays commented.
        for line in before.lines().take(3) {
            assert!(
                line.trim_start().starts_with('#'),
                "fixture must comment every line, got: {line:?}"
            );
        }
        let formatted = format_contents(before);
        for line in formatted.lines() {
            if line.contains("FOO=bar") {
                assert!(
                    line.trim_start().starts_with('#'),
                    "assignment-like continuation must stay commented, got: {line:?} in:\n{formatted}"
                );
            }
        }
        assert!(formatted.contains("# FOO=bar"));
        assert!(!formatted.lines().any(|line| line == "FOO=bar"));
    }

    #[test]
    fn unclosed_commented_quote_does_not_hide_active_vars() {
        // `# FOO="oops` is an unclosed commented quote; the following active
        // `BAR=value` must stay visible and never be swallowed.
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        let before = "# FOO=\"oops\nBAR=value\n";
        fs::write(&path, before).unwrap();

        let entries = parse_file(&path).unwrap();
        assert!(
            entries
                .iter()
                .any(|entry| entry.key == "BAR" && !entry.commented),
            "BAR must stay active, got: {entries:?}"
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.key == "BAR")
                .unwrap()
                .value,
            "value"
        );

        let active = parse_active_file(&path).unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].key, "BAR");

        let workspace = Workspace {
            root: tempdir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };
        let sources = collect_example(&workspace).unwrap();
        assert!(
            sources.iter().any(|source| source.name == "BAR"),
            "BAR source must survive, got: {:?}",
            sources
                .iter()
                .map(|source| &source.name)
                .collect::<Vec<_>>()
        );

        // Formatter leaves the two logical lines alone (no grouping).
        let formatted = format_contents(before);
        assert!(
            formatted.contains("BAR=value"),
            "BAR must survive formatting, got:\n{formatted}"
        );

        // Removing BAR keeps the commented line; removing FOO keeps BAR.
        remove_key(&path, "BAR").unwrap();
        let after_bar = fs::read_to_string(&path).unwrap();
        assert!(
            after_bar.contains("# FOO=\"oops"),
            "removing BAR must keep commented FOO, got:\n{after_bar}"
        );
        assert!(!after_bar.contains("BAR=value"));
        fs::write(&path, before).unwrap();
        remove_key(&path, "FOO").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "BAR=value\n");
    }

    #[test]
    fn malformed_active_unterminated_quote_errors_before_mutation() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");
        let before = "FOO=\"oops\nBAR=value\n";
        fs::write(&path, before).unwrap();

        assert!(
            parse_file(&path).is_err(),
            "active unterminated quote must error"
        );
        let workspace = Workspace {
            root: tempdir.path().to_path_buf(),
            rel: PathBuf::from("."),
            kind: crate::models::WorkspaceKind::App,
            framework: "typescript".to_string(),
        };
        assert!(collect_example(&workspace).is_err());
        assert!(sources(&path, Path::new("."), SourceKind::EnvExample).is_err());
        assert!(sources(&path, Path::new("."), SourceKind::EnvLocal).is_err());

        // Upsert and remove must error without mutating the file.
        assert!(upsert_example(&path, "BAZ", "qux").is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "failed upsert must leave file unchanged"
        );
        assert!(remove_key(&path, "BAR").is_err());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            before,
            "failed remove must never drop trailing vars"
        );
        // Removing an unrelated key also errors rather than dropping.
        assert!(remove_key(&path, "OTHER").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);

        // Single-line unterminated is also malformed.
        fs::write(&path, "FOO=\"oops\n").unwrap();
        assert!(parse_file(&path).is_err());

        // Formatter cannot signal an error so it preserves input unchanged.
        assert_eq!(format_contents(before), before);
    }

    #[test]
    fn backslash_and_crlf_values_roundtrip() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(".env.example");

        // Backslashes and embedded quotes survive a write/read cycle.
        let value = "C:\\Users\\name \"quoted\" \\";
        let quoted = quote_value(value);
        fs::write(&path, format!("WIN={quoted}\n")).unwrap();
        let entries = parse_file(&path).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].value, value, "backslash round-trip");
        assert_eq!(entries[0].quote, Some('"'));

        // CRLF inside a multiline value is escaped as `\r` + real newline so
        // `lines()` splitting does not lose the carriage return.
        let crlf = "line1\r\nline2\r\nline3";
        let crlf_quoted = quote_value(crlf);
        assert!(
            crlf_quoted.contains("\\r"),
            "CRLF must escape CR, got: {crlf_quoted:?}"
        );
        fs::write(&path, format!("CRLF={crlf_quoted}\nAFTER=1\n")).unwrap();
        let entries = parse_file(&path).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].value, crlf);
        assert_eq!(entries[1].value, "1");

        // Single-quoted multiline preserves newlines and backslashes.
        let single = "a\\b\nc'd";
        let single_quoted = quote_value_with(single, '\'');
        fs::write(&path, format!("SINGLE={single_quoted}\n")).unwrap();
        let entries = parse_file(&path).unwrap();
        assert_eq!(entries[0].value, single);

        // Double-quoted `\n` escape expands to a real newline per loader docs.
        fs::write(&path, "ESC=\"a\\nb\"\n").unwrap();
        let entries = parse_file(&path).unwrap();
        assert_eq!(entries[0].value, "a\nb");
    }
}

pub fn format_contents(contents: &str) -> String {
    if contents.trim().is_empty() {
        return String::new();
    }

    let sections = match parse_sections(contents) {
        Ok(sections) => sections,
        // Cannot signal an error through this signature; preserve the input
        // unchanged so a malformed active quote is never reformatted away.
        Err(_) => return contents.to_string(),
    };
    let mut sections = sections;
    if sections.iter().any(|section| section.header.is_some()) {
        sections.sort_by(|left, right| compare_sections(left, right));
    }
    move_local_only_duplicates_next_to_templates(&mut sections);

    let mut output = Vec::new();
    for section in sections {
        let rendered = render_section(section);
        if rendered.is_empty() {
            continue;
        }
        if !output.is_empty() {
            output.push(String::new());
        }
        output.extend(rendered);
    }

    format!("{}\n", output.join("\n").trim_end())
}

fn move_local_only_duplicates_next_to_templates(sections: &mut Vec<RawSection>) {
    let template_targets = sections
        .iter()
        .enumerate()
        .filter(|(_, section)| !is_local_only_section(section))
        .flat_map(|(index, section)| {
            section.lines.iter().filter_map(move |line| {
                let (key, commented) = parse_entry_key(line)?;
                commented.then_some((key, index))
            })
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    if template_targets.is_empty() {
        return;
    }

    let mut moved = Vec::<(usize, String, String)>::new();
    for section in sections
        .iter_mut()
        .filter(|section| is_local_only_section(section))
    {
        let mut kept = Vec::new();
        for line in std::mem::take(&mut section.lines) {
            if let Some(key) = parse_assignment_key(&line) {
                if let Some(target) = template_targets.get(&key) {
                    moved.push((*target, key, line));
                    continue;
                }
            }
            kept.push(line);
        }
        section.lines = kept;
    }

    for (target, key, line) in moved {
        insert_after_key_block(&mut sections[target].lines, &key, line);
    }

    sections.retain(|section| {
        !is_local_only_section(section) || section.lines.iter().any(|line| !line.trim().is_empty())
    });
}

fn insert_after_key_block(lines: &mut Vec<String>, key: &str, line: String) {
    let insert_at = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            parse_entry_key(line)
                .filter(|(line_key, _)| line_key == key)
                .map(|_| index + 1)
        })
        .last()
        .unwrap_or(lines.len());
    lines.insert(insert_at, line);
}

fn is_local_only_section(section: &RawSection) -> bool {
    section
        .header
        .as_deref()
        .map(section_label)
        .is_some_and(|label| label.eq_ignore_ascii_case("local-only"))
}

pub fn local_path(project: &Project, workspace: &Workspace) -> PathBuf {
    if project.is_monorepo {
        project.root.join(".env")
    } else {
        workspace.root.join(".env")
    }
}

pub fn collect_example(workspace: &Workspace) -> Result<Vec<VarSource>> {
    let path = example_path(workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    sources(&path, &workspace.rel, SourceKind::EnvExample)
}

pub fn collect_local(project: &Project, workspace: &Workspace) -> Result<Vec<VarSource>> {
    if project.is_monorepo {
        return Ok(Vec::new());
    }

    let path = local_path(project, workspace);
    if !path.exists() {
        return Ok(Vec::new());
    }
    sources(&path, &workspace.rel, SourceKind::EnvLocal)
}

pub fn collect_root_local(project: &Project) -> Result<Vec<VarSource>> {
    if !project.is_monorepo {
        return Ok(Vec::new());
    }

    let path = project.root.join(".env");
    if !path.exists() {
        return Ok(Vec::new());
    }
    sources(&path, &PathBuf::from("."), SourceKind::EnvLocal)
}

pub fn sources(path: &Path, owner: &Path, kind: SourceKind) -> Result<Vec<VarSource>> {
    let contents = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for logical in split_logical_lines(&contents)? {
        let parsed = if matches!(kind, SourceKind::EnvExample) {
            parse_entry_line(&logical.text).map(|entry| (entry.key, entry.value))
        } else {
            parse_line(&logical.text)
        };
        if let Some((key, value)) = parsed {
            out.push(VarSource {
                name: key,
                owner: owner.to_path_buf(),
                scope: Scope::Unknown,
                kind: kind.clone(),
                value_type: None,
                enum_values: None,
                required: None,
                default_value: None,
                description: None,
                value: Some(value),
                path: path.to_path_buf(),
                line: logical.start_line,
            });
        }
    }
    Ok(out)
}

pub fn parse_file(path: &Path) -> Result<Vec<DotenvEntry>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(path)?;
    Ok(split_logical_lines(&contents)?
        .into_iter()
        .filter_map(|logical| parse_entry_line(&logical.text))
        .collect())
}

pub fn parse_active_file(path: &Path) -> Result<Vec<DotenvEntry>> {
    Ok(parse_file(path)?
        .into_iter()
        .filter(|entry| !entry.commented)
        .collect())
}

fn parse_entry_line(line: &str) -> Option<DotenvEntry> {
    if let Some(parsed) = parse_assignment(line) {
        return Some(DotenvEntry {
            key: parsed.key,
            value: parsed.value,
            commented: false,
            quote: parsed.quote,
        });
    }

    let trimmed = line.trim_start();
    let body_first = trimmed.strip_prefix('#')?.trim_start();
    // Commented multiline: strip exactly one added `# `/`#` prefix from every
    // continuation line, but only when the whole continuation is commented.
    // Otherwise this is not a grouped logical line (see split) and we must
    // not swallow active lines.
    let body = if line.contains('\n') {
        let mut parts = body_first.split('\n');
        let first = parts.next().unwrap_or("").to_string();
        let mut out = vec![first];
        for cont in parts {
            out.push(strip_one_comment_prefix(cont)?);
        }
        out.join("\n")
    } else {
        body_first.to_string()
    };
    let parsed = parse_assignment(&body)?;
    Some(DotenvEntry {
        key: parsed.key,
        value: parsed.value,
        commented: true,
        quote: parsed.quote,
    })
}

pub fn parse_line(line: &str) -> Option<(String, String)> {
    let parsed = parse_assignment(line)?;
    Some((parsed.key, parsed.value))
}

struct ParsedAssignment {
    key: String,
    value: String,
    quote: Option<char>,
}

fn parse_assignment(line: &str) -> Option<ParsedAssignment> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("export ") {
        return None;
    }
    let (key, value) = trimmed.split_once('=')?;
    let key = key.trim();
    if !is_valid_var_name(key) {
        return None;
    }
    let raw_value = value.trim_start();
    let value_without_comment = strip_inline_comment(raw_value).trim_end();
    let (value, quote) = unquote_value(value_without_comment);
    Some(ParsedAssignment {
        key: key.to_string(),
        value,
        quote,
    })
}

pub fn key_set(path: &Path) -> Result<BTreeSet<String>> {
    Ok(parse_file(path)?
        .into_iter()
        .map(|entry| entry.key)
        .collect())
}

pub fn upsert_example(path: &Path, key: &str, value: &str) -> Result<()> {
    upsert_example_entry(path, key, value, false)
}

pub fn upsert_commented_example(path: &Path, key: &str, value: &str) -> Result<()> {
    upsert_example_entry(path, key, value, true)
}

fn upsert_example_entry(path: &Path, key: &str, value: &str, commented: bool) -> Result<()> {
    let mut lines = if path.exists() {
        let contents = fs::read_to_string(path)?;
        split_logical_lines(&contents)?
            .into_iter()
            .map(|logical| logical.text)
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let rendered_value = if commented && value.is_empty() {
        String::new()
    } else {
        quote_value(value)
    };
    let assignment = format!("{key}={rendered_value}");
    let replacement = if commented {
        comment_assignment(&assignment)
    } else {
        assignment
    };
    let mut changed = false;
    for line in &mut lines {
        if parse_entry_line(line)
            .map(|entry| entry.key == key)
            .unwrap_or(false)
        {
            *line = replacement.clone();
            changed = true;
        }
    }

    if !changed {
        if !lines.is_empty() && lines.last().is_some_and(|line| !line.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(replacement);
    }

    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}

pub fn remove_key(path: &Path, key: &str) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let contents = fs::read_to_string(path)?;
    let lines = split_logical_lines(&contents)?
        .into_iter()
        .filter(|logical| {
            parse_entry_line(&logical.text)
                .map(|entry| entry.key != key)
                .unwrap_or(true)
        })
        .map(|logical| logical.text)
        .collect::<Vec<_>>();
    fs::write(path, format!("{}\n", lines.join("\n").trim_end()))?;
    Ok(())
}

pub fn quote_value(value: &str) -> String {
    if value.contains('\n') || value.contains('\r') {
        return quote_double_preserving_newlines(value);
    }
    if value.is_empty()
        || value
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '#' | '$' | '<' | '>'))
    {
        format!("{:?}", value)
    } else {
        value.to_string()
    }
}

pub fn quote_value_with(value: &str, quote: char) -> String {
    if value.contains('\n') || value.contains('\r') {
        match quote {
            '\'' => quote_single_preserving_newlines(value),
            _ => quote_double_preserving_newlines(value),
        }
    } else {
        match quote {
            '\'' => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
            _ => format!("{:?}", value),
        }
    }
}

fn strip_inline_comment(value: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;
    let mut previous_was_whitespace = false;

    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
            previous_was_whitespace = ch.is_whitespace();
            continue;
        }
        if ch == '\\' {
            escaped = true;
            previous_was_whitespace = false;
            continue;
        }
        if let Some(active_quote) = quote {
            if ch == active_quote {
                quote = None;
            }
            previous_was_whitespace = ch.is_whitespace();
            continue;
        }
        if matches!(ch, '\'' | '"') {
            quote = Some(ch);
            previous_was_whitespace = false;
            continue;
        }
        if ch == '#' && (index == 0 || previous_was_whitespace) {
            return &value[..index];
        }
        previous_was_whitespace = ch.is_whitespace();
    }

    value
}

fn unquote_value(value: &str) -> (String, Option<char>) {
    let value = value.trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        if (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'')
        {
            let quote = bytes[0] as char;
            let inner = &value[1..value.len() - 1];
            let decoded = if quote == '"' {
                decode_double_quoted(inner)
            } else {
                decode_single_quoted(inner)
            };
            return (decoded, Some(quote));
        }
    }
    (value.to_string(), None)
}

/// Decode double-quoted escapes (`\\`, `\"`, `\'`, `\n`, `\r`, `\t`, `\b`,
/// `\f`, `\v`, `\a`). Real newlines from multiline values pass through.
/// Unknown escapes are preserved verbatim so no data is lost.
fn decode_double_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(esc) = chars.next() else {
            out.push('\\');
            break;
        };
        match esc {
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'b' => out.push('\x08'),
            'f' => out.push('\x0C'),
            'v' => out.push('\x0B'),
            'a' => out.push('\x07'),
            '\\' => out.push('\\'),
            '"' => out.push('"'),
            '\'' => out.push('\''),
            '\n' => out.push('\n'),
            _ => {
                out.push('\\');
                out.push(esc);
            }
        }
    }
    out
}

/// Decode single-quoted escapes; only `\\` and `\'` are special.
fn decode_single_quoted(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(esc) = chars.next() else {
            out.push('\\');
            break;
        };
        match esc {
            '\\' => out.push('\\'),
            '\'' => out.push('\''),
            _ => {
                out.push('\\');
                out.push(esc);
            }
        }
    }
    out
}
