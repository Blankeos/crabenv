use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::graph::EnvGraph;
use crate::models::{EnvRecord, EnvSurface, Scope};
use crate::util::display_rel;

pub fn render_list(graph: &EnvGraph) {
    let grouped = group_records_by_name(graph);
    let mut rows = grouped
        .values()
        .filter(|records| group_has_definition_surface(records))
        .map(|records| ListRow {
            index: String::new(),
            name: records
                .first()
                .map(|record| record.name.clone())
                .unwrap_or_default(),
            owner: format_group_owner(records),
            scope: format_group_scope(records),
            value_type: format_group_type(records),
            surfaces: format_group_surfaces(records),
        })
        .collect::<Vec<_>>();

    rows.sort_by(|left, right| {
        list_owner_rank(&left.owner)
            .cmp(&list_owner_rank(&right.owner))
            .then_with(|| left.owner.cmp(&right.owner))
            .then_with(|| left.name.cmp(&right.name))
    });
    for (index, row) in rows.iter_mut().enumerate() {
        row.index = (index + 1).to_string();
    }

    let widths = ListWidths::from_rows(&rows);

    println!("{} variable(s)", rows.len());
    println!();
    println!(
        "{:<idx$}  {:<name$}  {:<owner$}  {:<scope$}  {:<type_width$}  surfaces",
        "#",
        "name",
        "owner",
        "scope",
        "type",
        idx = widths.index,
        name = widths.name,
        owner = widths.owner,
        scope = widths.scope,
        type_width = widths.value_type,
    );
    println!("{}", "-".repeat(widths.total()));

    for row in rows {
        println!(
            "{:<idx$}  {:<name$}  {:<owner$}  {:<scope$}  {:<type_width$}  {}",
            row.index,
            row.name,
            row.owner,
            row.scope,
            row.value_type,
            row.surfaces,
            idx = widths.index,
            name = widths.name,
            owner = widths.owner,
            scope = widths.scope,
            type_width = widths.value_type,
        );
    }
}

fn list_owner_rank(owner: &str) -> u8 {
    if owner.starts_with("shared(") {
        0
    } else {
        1
    }
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

fn format_group_owner(records: &[&EnvRecord]) -> String {
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
        format!("shared({})", owners.len())
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

fn format_group_type(records: &[&EnvRecord]) -> String {
    let types = records
        .iter()
        .filter(|record| record_has_schema_surface(record))
        .map(|record| format_list_type(record))
        .filter(|value_type| value_type != "-")
        .collect::<BTreeSet<_>>();

    match types.len() {
        0 => "-".to_string(),
        1 => types.iter().next().unwrap().to_string(),
        _ => "mixed".to_string(),
    }
}

fn format_group_surfaces(records: &[&EnvRecord]) -> String {
    let surfaces = records
        .iter()
        .flat_map(|record| record.surfaces.iter())
        .copied()
        .collect::<BTreeSet<_>>();

    if surfaces.is_empty() {
        return "-".to_string();
    }

    surfaces
        .iter()
        .map(EnvSurface::as_str)
        .collect::<Vec<_>>()
        .join(" ")
}

fn record_has_schema_surface(record: &EnvRecord) -> bool {
    record.surfaces.contains(&EnvSurface::Schema)
}

fn record_has_template_surface(record: &EnvRecord) -> bool {
    record.surfaces.contains(&EnvSurface::Template)
}

fn format_list_type(record: &EnvRecord) -> String {
    let mut value_type = record.value_type.clone().unwrap_or_else(|| "-".to_string());
    if record.required == Some(false) && value_type != "-" {
        value_type.push('?');
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
}

struct ListWidths {
    index: usize,
    name: usize,
    owner: usize,
    scope: usize,
    value_type: usize,
    surfaces: usize,
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
        };

        for row in rows {
            widths.index = widths.index.max(row.index.len());
            widths.name = widths.name.max(row.name.len());
            widths.owner = widths.owner.max(row.owner.len());
            widths.scope = widths.scope.max(row.scope.len());
            widths.value_type = widths.value_type.max(row.value_type.len());
            widths.surfaces = widths.surfaces.max(row.surfaces.len());
        }

        widths
    }

    fn total(&self) -> usize {
        self.index + self.name + self.owner + self.scope + self.value_type + self.surfaces + 10
    }
}
