use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::graph::EnvGraph;
use crate::models::{EnvRecord, EnvSurface, Project, Scope};
use crate::ordering::compare_env_names;
use crate::util::{color, display_rel};

const DESCRIPTION_WIDTH: usize = 48;

pub fn render_list(project: &Project, graph: &EnvGraph, expand: bool) {
    let grouped = group_records_by_name(graph);
    let app_owner_count = app_owner_count(project);
    let mut rows = grouped
        .values()
        .filter(|records| group_has_definition_surface(records))
        .map(|records| ListRow {
            index: String::new(),
            name: records
                .first()
                .map(|record| record.name.clone())
                .unwrap_or_default(),
            owner: format_group_owner(records, app_owner_count, expand),
            scope: format_group_scope(records),
            value_type: format_group_type(records, expand),
            surfaces: format_group_surfaces(records),
            description: format_group_description(records),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        compare_inventory_rows(&left.name, &left.owner, &right.name, &right.owner)
    });
    for (index, row) in rows.iter_mut().enumerate() {
        row.index = (index + 1).to_string();
    }
    let widths = ListWidths::from_rows(&rows);

    println!("{} variable(s)", rows.len());
    println!();
    println!(
        "{:<idx$}  {:<name$}  {:<owner$}  {:<scope$}  {:<type_width$}  {:<surfaces$}  description",
        "#",
        "name",
        "owner",
        "scope",
        "type",
        "surfaces",
        idx = widths.index,
        name = widths.name,
        owner = widths.owner,
        scope = widths.scope,
        type_width = widths.value_type,
        surfaces = widths.surfaces,
    );
    println!("{}", "-".repeat(widths.total()));

    for row in rows {
        let description_lines = wrap_cell(&row.description, widths.description);
        println!(
            "{:<idx$}  {:<name$}  {:<owner$}  {:<scope$}  {:<type_width$}  {:<surfaces$}  {}",
            row.index,
            row.name,
            row.owner,
            row.scope,
            row.value_type,
            row.surfaces,
            description_lines.first().map(String::as_str).unwrap_or(""),
            idx = widths.index,
            name = widths.name,
            owner = widths.owner,
            scope = widths.scope,
            type_width = widths.value_type,
            surfaces = widths.surfaces,
        );
        for line in description_lines.iter().skip(1) {
            println!(
                "{:<idx$}  {:<name$}  {:<owner$}  {:<scope$}  {:<type_width$}  {:<surfaces$}  {}",
                "",
                "",
                "",
                "",
                "",
                "",
                line,
                idx = widths.index,
                name = widths.name,
                owner = widths.owner,
                scope = widths.scope,
                type_width = widths.value_type,
                surfaces = widths.surfaces,
            );
        }
    }
}

fn format_owner_list(owners: &BTreeSet<PathBuf>) -> String {
    owners
        .iter()
        .map(|owner| display_rel(owner))
        .collect::<Vec<_>>()
        .join(", ")
}

fn app_owner_count(project: &Project) -> usize {
    project
        .workspaces
        .iter()
        .filter(|workspace| workspace.kind == crate::models::WorkspaceKind::App)
        .count()
}

pub fn render_doctor_inventory(project: &Project, graph: &EnvGraph) {
    let grouped = group_records_by_name(graph);
    let app_owner_count = app_owner_count(project);
    let mut rows = grouped
        .values()
        .map(|records| DoctorInventoryRow {
            name: records
                .first()
                .map(|record| record.name.clone())
                .unwrap_or_default(),
            owner: format_group_owner(records, app_owner_count, false),
            surfaces: group_surfaces(records),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        compare_inventory_rows(&left.name, &left.owner, &right.name, &right.owner)
    });
    let show_owner = rows.iter().any(|row| row.owner != ".");
    let name_width = rows
        .iter()
        .map(|row| row.name.len())
        .max()
        .unwrap_or("name".len())
        .max("name".len());
    let owner_width = rows
        .iter()
        .map(|row| row.owner.len())
        .max()
        .unwrap_or("owner".len())
        .max("owner".len());

    println!();
    println!("detected env vars: {}", rows.len());
    println!();

    if show_owner {
        println!(
            "{:<name_width$}  {:<owner_width$}  schema  template  local  sinks",
            "name", "owner"
        );
        println!(
            "{}  {}  ------  --------  -----  -----",
            "-".repeat(name_width),
            "-".repeat(owner_width),
        );
    } else {
        println!("{:<name_width$}  schema  template  local  sinks", "name");
        println!("{}  ------  --------  -----  -----", "-".repeat(name_width));
    }

    for row in rows {
        let schema = required_surface_cell(row.surfaces.contains(&EnvSurface::Schema));
        let template = required_surface_cell(row.surfaces.contains(&EnvSurface::Template));
        let local = required_surface_cell(row.surfaces.contains(&EnvSurface::Local));
        let sinks = optional_surface_cell(row.surfaces.contains(&EnvSurface::Sinks));

        if show_owner {
            println!(
                "{:<name_width$}  {:<owner_width$}  {}     {}       {}    {}",
                row.name, row.owner, schema, template, local, sinks
            );
        } else {
            println!(
                "{:<name_width$}  {}     {}       {}    {}",
                row.name, schema, template, local, sinks
            );
        }
    }
}

fn list_owner_rank(owner: &str) -> u8 {
    if owner.starts_with("shared(") {
        0
    } else {
        1
    }
}

fn compare_inventory_rows(
    left_name: &str,
    left_owner: &str,
    right_name: &str,
    right_owner: &str,
) -> std::cmp::Ordering {
    list_owner_rank(left_owner)
        .cmp(&list_owner_rank(right_owner))
        .then_with(|| compare_env_names(left_name, right_name))
        .then_with(|| left_owner.cmp(right_owner))
}

fn group_has_definition_surface(records: &[&EnvRecord]) -> bool {
    records
        .iter()
        .any(|record| record_has_schema_surface(record) || record_has_template_surface(record))
}

fn group_records_by_name(graph: &EnvGraph) -> BTreeMap<String, Vec<&EnvRecord>> {
    let mut grouped = BTreeMap::<String, Vec<&EnvRecord>>::new();
    for record in graph.values() {
        grouped.entry(record.name.clone()).or_default().push(record);
    }
    grouped
}

fn format_group_owner(records: &[&EnvRecord], app_owner_count: usize, expand: bool) -> String {
    let schema_owners = owners_matching(records, record_has_schema_surface);
    let owners = if schema_owners.is_empty() {
        let template_owners = owners_matching(records, record_has_template_surface);
        if template_owners.is_empty() {
            owners_matching(records, |_| true)
        } else {
            template_owners
        }
    } else {
        schema_owners
    };

    if owners.len() > 1 {
        let label = if owners.len() == app_owner_count {
            "shared(all)".to_string()
        } else {
            format!("shared({})", owners.len())
        };

        if expand {
            format!("{}: {}", label, format_owner_list(&owners))
        } else {
            label
        }
    } else {
        owners
            .iter()
            .next()
            .map(|owner| display_rel(owner))
            .unwrap_or_else(|| "-".to_string())
    }
}

fn owners_matching(
    records: &[&EnvRecord],
    predicate: impl Fn(&EnvRecord) -> bool,
) -> BTreeSet<PathBuf> {
    records
        .iter()
        .filter(|record| predicate(record))
        .map(|record| record.owner.clone())
        .collect()
}

fn format_group_scope(records: &[&EnvRecord]) -> String {
    let scopes = records
        .iter()
        .filter(|record| record_has_schema_surface(record))
        .filter(|record| record.scope != Scope::Unknown)
        .map(|record| record.scope.as_str())
        .collect::<BTreeSet<_>>();

    match scopes.len() {
        0 => "-".to_string(),
        1 => scopes.iter().next().unwrap().to_string(),
        _ => "mixed".to_string(),
    }
}

fn format_group_type(records: &[&EnvRecord], expand: bool) -> String {
    let types = records
        .iter()
        .filter(|record| record_has_schema_surface(record))
        .map(|record| format_list_type(record, expand))
        .filter(|value_type| value_type != "-")
        .collect::<BTreeSet<_>>();

    match types.len() {
        0 => "-".to_string(),
        1 => types.iter().next().unwrap().to_string(),
        _ => "mixed".to_string(),
    }
}

fn format_group_surfaces(records: &[&EnvRecord]) -> String {
    let surfaces = group_surfaces(records);

    if surfaces.is_empty() {
        return "-".to_string();
    }

    surfaces
        .iter()
        .map(EnvSurface::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

fn format_group_description(records: &[&EnvRecord]) -> String {
    let descriptions = records
        .iter()
        .filter(|record| record_has_schema_surface(record))
        .filter_map(|record| record.description.as_deref())
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .collect::<BTreeSet<_>>();

    match descriptions.len() {
        0 => "-".to_string(),
        1 => descriptions.iter().next().unwrap().to_string(),
        _ => format!(
            "mixed: {}",
            descriptions.into_iter().collect::<Vec<_>>().join(" / ")
        ),
    }
}

fn group_surfaces(records: &[&EnvRecord]) -> BTreeSet<EnvSurface> {
    records
        .iter()
        .flat_map(|record| record.surfaces.iter())
        .copied()
        .collect::<BTreeSet<_>>()
}

fn required_surface_cell(checked: bool) -> String {
    if checked {
        color("[x]", "32")
    } else {
        color("[ ]", "33")
    }
}

fn optional_surface_cell(checked: bool) -> String {
    if checked {
        color("[x]", "32")
    } else {
        color("[-]", "90")
    }
}

fn record_has_schema_surface(record: &EnvRecord) -> bool {
    record.surfaces.contains(&EnvSurface::Schema)
}

fn record_has_template_surface(record: &EnvRecord) -> bool {
    record.surfaces.contains(&EnvSurface::Template)
}

fn format_list_type(record: &EnvRecord, expand: bool) -> String {
    let mut value_type = record.value_type.clone().unwrap_or_else(|| "-".to_string());
    if record.required == Some(false) && value_type != "-" {
        value_type.push('?');
    }
    if expand {
        if let Some(values) = &record.enum_values {
            if !values.is_empty() && value_type.starts_with("enum(") {
                value_type.push_str(": ");
                value_type.push_str(&values.join(" | "));
            }
        }
    }
    value_type
}

struct ListRow {
    index: String,
    name: String,
    owner: String,
    scope: String,
    value_type: String,
    surfaces: String,
    description: String,
}

struct ListWidths {
    index: usize,
    name: usize,
    owner: usize,
    scope: usize,
    value_type: usize,
    surfaces: usize,
    description: usize,
}

impl ListWidths {
    fn from_rows(rows: &[ListRow]) -> Self {
        let mut widths = Self {
            index: "#".len(),
            name: "name".len(),
            owner: "owner".len(),
            scope: "scope".len(),
            value_type: "type".len(),
            surfaces: "surfaces".len(),
            description: "description".len(),
        };

        for row in rows {
            widths.index = widths.index.max(row.index.len());
            widths.name = widths.name.max(row.name.len());
            widths.owner = widths.owner.max(row.owner.len());
            widths.scope = widths.scope.max(row.scope.len());
            widths.value_type = widths.value_type.max(row.value_type.len());
            widths.surfaces = widths.surfaces.max(row.surfaces.len());
            widths.description = widths
                .description
                .max(row.description.len().min(DESCRIPTION_WIDTH));
        }

        widths
    }

    fn total(&self) -> usize {
        self.index
            + self.name
            + self.owner
            + self.scope
            + self.value_type
            + self.surfaces
            + self.description
            + 12
    }
}

fn wrap_cell(value: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if value.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    for word in value.split(' ') {
        let word_len = word.chars().count();
        if word_len > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            lines.extend(split_long_word(word, width));
            continue;
        }

        let current_len = current.chars().count();
        if current.is_empty() {
            current.push_str(word);
        } else if current_len + 1 + word_len <= width {
            current.push(' ');
            current.push_str(word);
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn split_long_word(word: &str, width: usize) -> Vec<String> {
    let chars = word.chars().collect::<Vec<_>>();
    chars
        .chunks(width)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

struct DoctorInventoryRow {
    name: String,
    owner: String,
    surfaces: BTreeSet<EnvSurface>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn wrap_cell_wraps_long_descriptions() {
        assert_eq!(
            wrap_cell(
                "Local sqlite URL in development, managed database URL elsewhere.",
                24
            ),
            vec![
                "Local sqlite URL in".to_string(),
                "development, managed".to_string(),
                "database URL elsewhere.".to_string(),
            ]
        );
    }

    #[test]
    fn wrap_cell_splits_unbroken_long_words() {
        assert_eq!(
            wrap_cell("abc supercalifragilistic", 8),
            vec![
                "abc".to_string(),
                "supercal".to_string(),
                "ifragili".to_string(),
                "stic".to_string(),
            ]
        );
    }

    #[test]
    fn inventory_rows_sort_shared_owners_before_env_name_order() {
        assert_eq!(
            compare_inventory_rows("ZZZ_API_KEY", "shared(2)", "NODE_ENV", "apps/web"),
            Ordering::Less
        );
    }

    #[test]
    fn inventory_rows_keep_env_name_order_within_owner_rank() {
        assert_eq!(
            compare_inventory_rows("S3_BUCKET", "apps/web", "NODE_ENV", "apps/api"),
            Ordering::Greater
        );
        assert_eq!(
            compare_inventory_rows("S3_BUCKET", "shared(2)", "RESEND_API_KEY", "shared(3)"),
            Ordering::Greater
        );
    }

    #[test]
    fn group_owner_marks_all_app_owners_as_shared_all() {
        let records = [record("apps/api"), record("apps/web")];
        let refs = records.iter().collect::<Vec<_>>();

        assert_eq!(format_group_owner(&refs, 2, false), "shared(all)");
    }

    #[test]
    fn group_owner_keeps_count_for_partial_shared_owners() {
        let records = [record("apps/api"), record("apps/web")];
        let refs = records.iter().collect::<Vec<_>>();

        assert_eq!(format_group_owner(&refs, 3, false), "shared(2)");
    }

    #[test]
    fn group_owner_expands_shared_owners() {
        let records = [record("apps/api"), record("apps/web")];
        let refs = records.iter().collect::<Vec<_>>();

        assert_eq!(
            format_group_owner(&refs, 3, true),
            "shared(2): apps/api, apps/web"
        );
    }

    #[test]
    fn list_type_expands_enum_values() {
        let mut record = record("apps/api");
        record.value_type = Some("enum(3)".to_string());
        record.enum_values = Some(vec![
            "development".to_string(),
            "staging".to_string(),
            "production".to_string(),
        ]);

        assert_eq!(
            format_list_type(&record, true),
            "enum(3): development | staging | production"
        );
    }

    #[test]
    fn list_type_marks_optional_enum_before_expanded_values() {
        let mut record = record("apps/api");
        record.value_type = Some("enum(2)".to_string());
        record.enum_values = Some(vec!["debug".to_string(), "info".to_string()]);
        record.required = Some(false);

        assert_eq!(format_list_type(&record, true), "enum(2)?: debug | info");
    }

    fn record(owner: &str) -> EnvRecord {
        EnvRecord {
            name: "DATABASE_URL".to_string(),
            owner: PathBuf::from(owner),
            scope: Scope::Private,
            value_type: Some("string".to_string()),
            enum_values: None,
            required: Some(true),
            default_value: None,
            description: None,
            example_value: None,
            local_present: false,
            surfaces: BTreeSet::from([EnvSurface::Schema]),
            surface_sources: BTreeMap::new(),
            sources: BTreeSet::new(),
        }
    }
}
