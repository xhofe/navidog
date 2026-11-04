//! Plugin dialects own their declarations; nothing is leaked across reloads.

use crate::{
    ContextSelection, Dialect, DialectFeatures, ExplainStyle, Lex, TableOptionStyle,
    template::SqlTemplate,
};
use navidog_model::object::{ObjectKind, Scope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct DesignerDescriptor {
    pub properties: std::collections::BTreeSet<String>,
    pub constraints: std::collections::BTreeSet<String>,
    pub index_methods: Vec<String>,
    pub options: std::collections::BTreeMap<String, FieldDescriptor>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct FieldDescriptor {
    pub label: String,
    pub control: FieldControl,
    #[serde(default)]
    pub default: String,
    pub encoding: crate::template::Encoding,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldControl {
    Text,
    Sql,
    Checkbox,
    Number,
    Select(Vec<String>),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ObjectDesignerDescriptor {
    #[serde(default)]
    pub fields: std::collections::BTreeMap<String, FieldDescriptor>,
    pub create: SqlTemplate,
    #[serde(default)]
    pub update: Option<SqlTemplate>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
#[non_exhaustive]
pub struct DialectDescriptor {
    pub id: String,
    pub quote: char,
    pub has_schemas: bool,
    pub database_prefix: bool,
    pub object_kinds: Vec<ObjectKind>,
    pub keywords: Vec<String>,
    pub functions: Vec<String>,
    pub extra_lex: Option<crate::Lex>,
    pub system_databases: Vec<String>,
    pub system_schemas: Vec<String>,
    pub backslash_strings: bool,
    pub dollar_quotes: bool,
    pub nested_comments: bool,
    pub begin: Option<String>,
    pub savepoint: Option<String>,
    pub rollback_savepoint: Option<String>,
    pub transactional_ddl: bool,
    pub designer: DesignerDescriptor,
    pub objects: std::collections::BTreeMap<String, ObjectDesignerDescriptor>,
    pub types: Vec<crate::ColumnType>,
    pub templates: BTreeMap<String, SqlTemplate>,
}
impl Default for DialectDescriptor {
    fn default() -> Self {
        Self {
            id: String::new(),
            quote: '"',
            has_schemas: false,
            database_prefix: false,
            object_kinds: vec![ObjectKind::Table, ObjectKind::View],
            keywords: Vec::new(),
            functions: Vec::new(),
            extra_lex: None,
            system_databases: Vec::new(),
            system_schemas: Vec::new(),
            backslash_strings: false,
            dollar_quotes: false,
            nested_comments: false,
            begin: None,
            savepoint: None,
            rollback_savepoint: None,
            transactional_ddl: false,
            designer: Default::default(),
            objects: Default::default(),
            types: Vec::new(),
            templates: BTreeMap::new(),
        }
    }
}
impl DialectDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || !['"', '`', '['].contains(&self.quote) {
            return Err("Dialect needs an id and a supported identifier quote.".into());
        }
        if self.begin.is_some() != self.savepoint.is_some()
            || self.begin.is_some() != self.rollback_savepoint.is_some()
        {
            return Err(
                "Grid writes need begin, savepoint and rollback-savepoint contracts.".into(),
            );
        }
        for property in &self.designer.properties {
            if ![
                "type",
                "length",
                "scale",
                "not_null",
                "default",
                "comment",
                "collation",
                "charset",
                "auto_increment",
                "unsigned",
                "generated",
                "expression",
                "storage",
                "primary_key",
                "on_update",
                "zerofill",
                "identity",
            ]
            .contains(&property.as_str())
            {
                return Err(format!("Unknown column property: {property}"));
            }
        }
        for (kind, object) in &self.objects {
            if crate::definition::DefinitionKind::from_key(kind).is_none() {
                return Err(format!("Unknown object designer kind: {kind}"));
            }
            object.create.validate()?;
            if let Some(update) = &object.update {
                update.validate()?;
            }
            for template in std::iter::once(&object.create).chain(object.update.iter()) {
                for parameter in template
                    .parameter_names()
                    .filter_map(|key| key.strip_prefix("field."))
                {
                    let field = object
                        .fields
                        .get(parameter)
                        .ok_or_else(|| format!("Undeclared object field: {parameter}"))?;
                    if template.encoding(&format!("field.{parameter}")) != Some(field.encoding) {
                        return Err(format!(
                            "Field encoding disagrees with template: {parameter}"
                        ));
                    }
                }
            }
        }
        const OPERATIONS: &[&str] = &[
            "table_options",
            "table_comment",
            "partition",
            "create_trigger",
            "drop_trigger",
            "create_partition_child",
            "drop_partition_child",
            "create_table",
            "add_column",
            "drop_column",
            "alter_column",
            "add_constraint",
            "drop_constraint",
            "create_index",
            "drop_index",
            "move_column",
            "lock_table",
            "explain",
            "select_page",
            "count",
            "rename_table",
            "rename_view",
            "create_view",
            "update_view",
        ];
        if self.transactional_ddl && self.begin.is_none() {
            return Err("Transactional DDL needs a declared begin operation.".into());
        }
        for statement in [&self.begin, &self.savepoint, &self.rollback_savepoint]
            .into_iter()
            .flatten()
        {
            crate::span::check(statement, crate::Lex::GENERIC)
                .map_err(|e| format!("Invalid transaction statement: {e:?}"))?;
            if statement.trim().is_empty()
                || !crate::definition::single_statement(statement, crate::Lex::GENERIC)
            {
                return Err("Transaction operations must be single non-empty statements.".into());
            }
        }
        for (operation, template) in &self.templates {
            if !OPERATIONS.contains(&operation.as_str()) {
                return Err(format!("Unknown SQL operation template: {operation}"));
            }
            template.validate()?;
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct DeclaredDialect {
    descriptor: DialectDescriptor,
    generation: u64,
}
impl DeclaredDialect {
    pub fn new(descriptor: DialectDescriptor) -> Result<Self, String> {
        descriptor.validate()?;
        Ok(Self {
            descriptor,
            generation: 0,
        })
    }
    pub fn with_generation(mut self, generation: u64) -> Self {
        self.generation = generation;
        self
    }
    pub fn descriptor(&self) -> &DialectDescriptor {
        &self.descriptor
    }
    pub fn template(
        &self,
        name: &str,
        values: &BTreeMap<String, String>,
    ) -> Result<Option<String>, String> {
        self.descriptor
            .templates
            .get(name)
            .map(|t| t.render(self, values))
            .transpose()
    }
}
impl Dialect for DeclaredDialect {
    fn definition_statements(
        &self,
        scope: &Scope,
        draft: &crate::definition::DefinitionDraft,
    ) -> Result<Vec<String>, String> {
        if draft.raw {
            return Err("Plugin designers require structured fields.".into());
        }
        if let Some(descriptor) = self.descriptor.objects.get(draft.kind.key()) {
            let template = if draft.previous.is_some() {
                descriptor
                    .update
                    .as_ref()
                    .ok_or("Object updates are not declared.")?
            } else {
                &descriptor.create
            };
            let mut values = BTreeMap::from([
                ("table".into(), self.qualified(scope, &draft.name)),
                ("name".into(), draft.name.clone()),
                (
                    "old_name".into(),
                    draft.previous.clone().unwrap_or_default(),
                ),
                ("body".into(), draft.body.clone()),
            ]);
            for (key, field) in &descriptor.fields {
                let value = draft.options.get(key).unwrap_or(&field.default);
                match &field.control {
                    FieldControl::Number => {
                        value
                            .parse::<f64>()
                            .ok()
                            .filter(|n| n.is_finite())
                            .ok_or("Field needs a finite number.")?;
                    }
                    FieldControl::Checkbox if !["true", "false"].contains(&value.as_str()) => {
                        return Err("Checkbox field needs true or false.".into());
                    }
                    FieldControl::Select(choices) if !choices.contains(value) => {
                        return Err("Unknown field choice.".into());
                    }
                    _ => {}
                }
                values.insert(format!("field.{key}"), value.clone());
            }
            return Ok(vec![template.render(self, &values)?]);
        }
        let operation = format!(
            "{}_{}",
            if draft.previous.is_some() {
                "update"
            } else {
                "create"
            },
            draft.kind.key()
        );
        let mut values = BTreeMap::from([
            ("table".into(), self.qualified(scope, &draft.name)),
            ("name".into(), draft.name.clone()),
            (
                "old_name".into(),
                draft.previous.clone().unwrap_or_default(),
            ),
            ("body".into(), draft.body.clone()),
        ]);
        values.extend(
            draft
                .options
                .iter()
                .map(|(key, value)| (format!("field.{key}"), value.clone())),
        );
        self.template(&operation, &values)?
            .map(|sql| vec![sql])
            .ok_or_else(|| format!("Driver has not declared {operation}."))
    }
    fn plugin_generation(&self) -> Option<u64> {
        Some(self.generation)
    }
    fn grid_writes(&self) -> bool {
        self.descriptor.begin.is_some()
            && self.descriptor.savepoint.is_some()
            && self.descriptor.rollback_savepoint.is_some()
    }
    fn savepoint_sql(&self) -> &str {
        self.descriptor.savepoint.as_deref().unwrap_or("")
    }
    fn rollback_savepoint_sql(&self) -> &str {
        self.descriptor.rollback_savepoint.as_deref().unwrap_or("")
    }
    fn designer_descriptor(&self) -> Option<&DesignerDescriptor> {
        Some(&self.descriptor.designer)
    }
    fn object_designer(&self, kind: &str) -> Option<&ObjectDesignerDescriptor> {
        self.descriptor.objects.get(kind)
    }
    fn designer_templates(&self) -> Option<&BTreeMap<String, SqlTemplate>> {
        Some(&self.descriptor.templates)
    }
    fn id(&self) -> &str {
        &self.descriptor.id
    }
    fn lex(&self) -> Lex {
        if let Some(lex) = self.descriptor.extra_lex {
            return lex;
        }
        Lex {
            backtick_idents: self.descriptor.quote == '`',
            hash_comments: false,
            dollar_quotes: self.descriptor.dollar_quotes,
            bracket_idents: self.descriptor.quote == '[',
            backslash_strings: self.descriptor.backslash_strings,
            escape_string_prefix: false,
            double_quote_strings: false,
            nested_block_comments: self.descriptor.nested_comments,
            delimiter_command: false,
        }
    }
    fn quote_ident(&self, name: &str) -> String {
        let open = self.descriptor.quote;
        let close = if open == '[' { ']' } else { open };
        format!(
            "{open}{}{close}",
            name.replace(close, &format!("{close}{close}"))
        )
    }
    fn string_literal(&self, value: &str) -> String {
        let value = if self.descriptor.backslash_strings {
            value.replace('\\', "\\\\")
        } else {
            value.into()
        };
        format!("'{}'", value.replace('\'', "''"))
    }
    fn qualify_prefix<'a>(&self, scope: &'a Scope) -> Option<&'a str> {
        if self.descriptor.has_schemas {
            scope.schema.as_deref()
        } else if self.descriptor.database_prefix {
            Some(&scope.database)
        } else {
            None
        }
    }
    fn object_kinds(&self) -> &[ObjectKind] {
        &self.descriptor.object_kinds
    }
    fn begin_transaction(&self) -> &str {
        self.descriptor.begin.as_deref().unwrap_or("")
    }
    fn is_system_database(&self, name: &str) -> bool {
        self.descriptor.system_databases.iter().any(|s| s == name)
    }
    fn is_system_schema(&self, name: &str) -> bool {
        self.descriptor.system_schemas.iter().any(|s| s == name)
    }
    fn exclusive_lock_sql(&self, qualified: &str) -> Option<String> {
        self.template(
            "lock_table",
            &BTreeMap::from([("table".into(), qualified.into())]),
        )
        .ok()
        .flatten()
    }
    fn extra_keywords(&self) -> &[&str] {
        &[]
    }
    fn extra_functions(&self) -> &[&str] {
        &[]
    }
    fn keywords(&self) -> crate::keywords::Words<'_> {
        crate::keywords::Words::owned(crate::keywords::COMMON, &self.descriptor.keywords)
    }
    fn functions(&self) -> crate::keywords::Words<'_> {
        crate::keywords::Words::owned(
            crate::keywords::COMMON_FUNCTIONS,
            &self.descriptor.functions,
        )
    }
    fn rename_object(
        &self,
        scope: &Scope,
        kind: ObjectKind,
        old: &str,
        new: &str,
    ) -> Option<String> {
        self.template(
            &format!("rename_{}", kind.key()),
            &BTreeMap::from([
                ("table".into(), self.qualified(scope, old)),
                ("old_name".into(), old.into()),
                ("name".into(), new.into()),
            ]),
        )
        .ok()
        .flatten()
    }
    fn duplicate_table(
        &self,
        _: &Scope,
        _: &str,
        _: &str,
        _: bool,
        _: Option<&str>,
    ) -> Option<Vec<String>> {
        None
    }
    fn console_prompt(&self, database: &str, _: bool, continuation: bool) -> String {
        if continuation {
            "...>".into()
        } else {
            format!("{database}>")
        }
    }
    fn context_selection(&self) -> ContextSelection {
        if self.descriptor.has_schemas {
            ContextSelection::Schema
        } else {
            ContextSelection::Database
        }
    }
    fn features(&self) -> DialectFeatures {
        DialectFeatures {
            has_schemas: self.descriptor.has_schemas,
            transactional_ddl: self.descriptor.transactional_ddl,
            supports_column_comments: self.descriptor.designer.properties.contains("comment"),
            supports_virtual_generated: self.descriptor.designer.properties.contains("storage"),
            supports_column_charset: self.descriptor.designer.properties.contains("charset"),
            supports_identity: self.descriptor.designer.properties.contains("identity"),
            supports_unsigned: self.descriptor.designer.properties.contains("unsigned"),
            supports_zerofill: self.descriptor.designer.properties.contains("zerofill"),
            supports_on_update: self.descriptor.designer.properties.contains("on_update"),
            supports_partitions: self.descriptor.templates.contains_key("partition"),
            supports_index_method: !self.descriptor.designer.index_methods.is_empty(),
            supports_index_expression: self
                .descriptor
                .designer
                .constraints
                .contains("index_expression"),
            supports_exclude: self.descriptor.designer.constraints.contains("exclude"),
            fk_reference_schema: self.descriptor.has_schemas,
            ..Default::default()
        }
    }
    fn explain_sql(&self, sql: &str) -> Option<String> {
        self.template("explain", &BTreeMap::from([("body".into(), sql.into())]))
            .ok()
            .flatten()
    }
    fn select_page(
        &self,
        scope: &Scope,
        table: &str,
        filter: &str,
        order: &str,
        limit: Option<u64>,
        offset: u64,
        _rowid: bool,
    ) -> String {
        let table = self.qualified(scope, table);
        let values = BTreeMap::from([
            ("table".into(), table.clone()),
            ("body".into(), filter.into()),
            ("fields".into(), order.into()),
            ("limit".into(), limit.unwrap_or(u64::MAX).to_string()),
            ("offset".into(), offset.to_string()),
        ]);
        if let Ok(Some(sql)) = self.template("select_page", &values) {
            return sql;
        }
        let mut sql = format!("SELECT * FROM {table}");
        if !filter.trim().is_empty() {
            sql.push_str(&format!(" WHERE {filter}"));
        }
        if !order.trim().is_empty() {
            sql.push_str(&format!(" ORDER BY {order}"));
        }
        if let Some(limit) = limit {
            sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));
        }
        sql
    }
    fn default_column_type(&self) -> &str {
        self.descriptor
            .types
            .first()
            .map(|ty| ty.name.as_ref())
            .unwrap_or("INTEGER")
    }
    fn added_column_type(&self) -> &str {
        self.descriptor
            .types
            .iter()
            .find(|ty| ty.name.eq_ignore_ascii_case("text"))
            .map(|ty| ty.name.as_ref())
            .unwrap_or("TEXT")
    }
    fn column_types(&self) -> &[crate::ColumnType] {
        &self.descriptor.types
    }
    fn table_option_style(&self) -> TableOptionStyle {
        TableOptionStyle::Clauses
    }
    fn explain_style(&self) -> ExplainStyle {
        ExplainStyle::QueryPlanRows
    }
}
