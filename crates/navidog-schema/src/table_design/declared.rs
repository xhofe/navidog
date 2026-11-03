//! Generic ALTER planning never enters the builtin table rebuild paths.
use super::*;
use std::collections::BTreeMap;

pub(super) fn plan(
    d: &dyn Dialect,
    scope: &Scope,
    before: Option<&TableSnapshot>,
    after: &TableDesign,
) -> DbResult<Vec<String>> {
    let templates = d
        .designer_templates()
        .ok_or_else(|| DbError::msg("Designer templates are unavailable."))?;
    let render = |op: &str, values: BTreeMap<String, String>| -> DbResult<String> {
        templates
            .get(op)
            .ok_or_else(|| {
                DbError::msg(format!(
                    "Driver has not declared {op}; the original definition is preserved."
                ))
            })?
            .render(d, &values)
            .map_err(DbError::msg)
    };
    let values = |name: &str, definition: &str| {
        BTreeMap::from([
            ("table".into(), d.qualified(scope, &after.name)),
            ("name".into(), after.name.clone()),
            ("column".into(), name.into()),
            ("definition".into(), definition.into()),
        ])
    };
    after.validate(d)?;
    let Some(before) = before else {
        let mut definition = after
            .columns
            .iter()
            .map(|c| format!("{} {}", d.quote_ident(&c.name), c.definition))
            .collect::<Vec<_>>();
        definition.extend(after.constraints.clone());
        let mut out = vec![render("create_table", values("", &definition.join(", ")))?];
        for index in &after.indexes {
            out.push(render("create_index", values("", index))?);
        }
        return Ok(out);
    };
    let old = &before.design;
    if old == after {
        return Ok(vec![]);
    }
    if old.name != after.name {
        return Err(DbError::msg(
            "Rename the table before editing its structure.",
        ));
    }
    if old.owner != after.owner || old.unlogged != after.unlogged {
        return Err(DbError::msg(
            "Owner and persistence edits are not declared.",
        ));
    }
    for column in &after.columns {
        if let Some(original) = column
            .original
            .as_ref()
            .and_then(|name| old.columns.iter().find(|column| &column.name == name))
        {
            let before = column_properties(&original.definition, d);
            let next = column_properties(&column.definition, d);
            let checks = [
                ("type", before.data_type != next.data_type),
                ("length", before.length != next.length),
                ("scale", before.scale != next.scale),
                ("not_null", before.not_null != next.not_null),
                ("default", before.default != next.default),
                (
                    "comment",
                    before.comment != next.comment || original.comment != column.comment,
                ),
                ("charset", before.charset != next.charset),
                ("collation", before.collation != next.collation),
                ("identity", before.identity != next.identity),
                ("on_update", before.on_update != next.on_update),
                ("unsigned", before.unsigned != next.unsigned),
                (
                    "auto_increment",
                    before.auto_increment != next.auto_increment,
                ),
                (
                    "expression",
                    before.generated_expression != next.generated_expression,
                ),
                (
                    "storage",
                    before.generated_storage != next.generated_storage,
                ),
            ];
            if checks
                .iter()
                .any(|(key, changed)| *changed && !d.column_property(key))
            {
                return Err(DbError::msg(
                    "An undeclared column property changed; the original definition is preserved.",
                ));
            }
        }
    }
    let originals: Vec<_> = after
        .columns
        .iter()
        .filter_map(|c| c.original.as_ref())
        .collect();
    let original_order: Vec<_> = old
        .columns
        .iter()
        .filter(|c| originals.contains(&&c.name))
        .map(|c| &c.name)
        .collect();
    if originals != original_order && !templates.contains_key("move_column") {
        return Err(DbError::msg(
            "Column reordering requires an explicit move-column template.",
        ));
    }
    let mut out = Vec::new();
    for (operation, changed, fragment) in [
        (
            "table_options",
            old.options != after.options,
            after.options.clone(),
        ),
        (
            "table_comment",
            old.comment != after.comment,
            after.comment.clone(),
        ),
        (
            "partition",
            old.partition != after.partition,
            after.partition.clone(),
        ),
    ] {
        if changed {
            let mut parameters = values("", &fragment);
            parameters.insert("value".into(), fragment.clone());
            out.push(render(operation, parameters)?);
        }
    }
    for (operation, before, after) in [
        (
            "trigger",
            old.triggers
                .clone()
                .unwrap_or_else(|| before.triggers.clone()),
            after
                .triggers
                .clone()
                .unwrap_or_else(|| before.triggers.clone()),
        ),
        (
            "partition_child",
            old.partition_children.clone(),
            after.partition_children.clone(),
        ),
    ] {
        for definition in before
            .iter()
            .filter(|definition| !after.contains(definition))
        {
            out.push(render(
                &format!("drop_{operation}"),
                values("", definition),
            )?);
        }
        for definition in after
            .iter()
            .filter(|definition| !before.contains(definition))
        {
            out.push(render(
                &format!("create_{operation}"),
                values("", definition),
            )?);
        }
    }
    for constraint in old
        .constraints
        .iter()
        .filter(|c| !after.constraints.contains(c))
    {
        out.push(render("drop_constraint", values("", constraint))?);
    }
    for column in old.columns.iter().filter(|c| {
        !after
            .columns
            .iter()
            .any(|n| n.original.as_ref() == Some(&c.name))
    }) {
        out.push(render("drop_column", values(&column.name, ""))?);
    }
    for column in &after.columns {
        match column
            .original
            .as_ref()
            .and_then(|name| old.columns.iter().find(|c| &c.name == name))
        {
            None => out.push(render(
                "add_column",
                values(&column.name, &column.definition),
            )?),
            Some(old) if old.name != column.name || old.definition != column.definition => {
                let mut values = values(&column.name, &column.definition);
                values.insert("old_name".into(), old.name.clone());
                out.push(render("alter_column", values)?);
            }
            _ => {}
        }
    }
    for constraint in after
        .constraints
        .iter()
        .filter(|c| !old.constraints.contains(c))
    {
        out.push(render("add_constraint", values("", constraint))?);
    }
    for index in old.indexes.iter().filter(|i| !after.indexes.contains(i)) {
        out.push(render("drop_index", values("", index))?);
    }
    for index in after.indexes.iter().filter(|i| !old.indexes.contains(i)) {
        out.push(render("create_index", values("", index))?);
    }
    if originals != original_order {
        for (position, column) in after.columns.iter().enumerate() {
            let mut parameters = values(&column.name, "");
            parameters.insert("position".into(), position.to_string());
            out.push(render("move_column", parameters)?);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use navidog_sql::{
        declared::{DeclaredDialect, DialectDescriptor},
        template::{Encoding, SqlTemplate},
    };
    #[test]
    fn undeclared_changes_and_reordering_never_fall_into_builtin_rebuilds() {
        let template = SqlTemplate::new(
            "ALTER TABLE ${table} ADD COLUMN ${column} ${definition}".into(),
            BTreeMap::from([
                ("table".into(), Encoding::Fragment),
                ("column".into(), Encoding::Identifier),
                ("definition".into(), Encoding::Fragment),
            ]),
        )
        .unwrap();
        let descriptor: DialectDescriptor = serde_json::from_value(
            serde_json::json!({"id":"plugin","templates":{"add_column":template}}),
        )
        .unwrap();
        let d = DeclaredDialect::new(descriptor).unwrap();
        let scope = Scope::new("test", "db", None);
        let mut old = TableDesign::blank(&d);
        old.name = "t".into();
        old.columns[0].original = Some("id".into());
        let baseline = TableSnapshot::new(old.clone(), "opaque".into(), vec![]);
        let mut draft = old.clone();
        draft.columns.push(DesignColumn::new(2, "name", "TEXT"));
        assert_eq!(
            plan(&d, &scope, Some(&baseline), &draft).unwrap(),
            vec!["ALTER TABLE \"t\" ADD COLUMN \"name\" TEXT"]
        );
        draft.options = "unknown=1".into();
        assert!(plan(&d, &scope, Some(&baseline), &draft).is_err());
        draft = old;
        draft.columns.push(DesignColumn::new(2, "name", "TEXT"));
        draft.columns[1].original = Some("name".into());
        let baseline = TableSnapshot::new(draft.clone(), "opaque".into(), vec![]);
        draft.columns.reverse();
        assert!(plan(&d, &scope, Some(&baseline), &draft).is_err());
    }
}
