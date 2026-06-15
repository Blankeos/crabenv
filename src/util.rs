use anyhow::{bail, Result};
use std::env;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

pub fn normalize_root(root: &Path) -> Result<PathBuf> {
    let path = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()?.join(root)
    };
    Ok(path.canonicalize()?)
}

pub fn normalize_rel_root(root: &Path, rel: &Path) -> PathBuf {
    if rel == Path::new(".") {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}

pub fn normalize_rel_display(rel: &Path) -> PathBuf {
    if rel == Path::new(".") {
        PathBuf::from(".")
    } else {
        rel.to_path_buf()
    }
}

pub fn validate_var_name(name: &str) -> Result<()> {
    if is_valid_var_name(name) {
        Ok(())
    } else {
        bail!("invalid env var name {name}")
    }
}

pub fn is_valid_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_uppercase() || first == '_') {
        return false;
    }
    chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
}

pub fn line_number_at(contents: &str, byte_index: usize) -> usize {
    contents[..byte_index.min(contents.len())]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

pub fn display_rel(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

pub fn color(text: impl AsRef<str>, code: &str) -> String {
    if colors_enabled() {
        format!("\x1b[{code}m{}\x1b[0m", text.as_ref())
    } else {
        text.as_ref().to_string()
    }
}

fn colors_enabled() -> bool {
    env::var_os("NO_COLOR").is_none() && io::stdout().is_terminal()
}
