//! Lossless table drafts and structural snapshots shared with external drivers.
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DesignColumn {
    pub id: u64,
    pub original: Option<String>,
    pub name: String,
    pub definition: String,
    #[serde(default)]
    pub comment: String,
}
impl DesignColumn {
    pub fn new(id: u64, name: impl Into<String>, definition: impl Into<String>) -> Self {
        Self {
            id,
            original: None,
            name: name.into(),
            definition: definition.into(),
            comment: String::new(),
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TableDesign {
    pub name: String,
    pub columns: Vec<DesignColumn>,
    pub constraints: Vec<String>,
    pub indexes: Vec<String>,
    pub options: String,
    #[serde(default)]
    pub option_fields: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub unlogged: bool,
    /// `None` means “use the snapshot’s trigger list”. `Some` is an explicit edit, including
    /// an empty list when every trigger was removed.
    #[serde(default)]
    pub triggers: Option<Vec<String>>,
    /// `PARTITION BY …` clause, without a leading space. Empty when the table is not partitioned.
    #[serde(default)]
    pub partition: String,
    /// Extra statements for PostgreSQL child partitions (`CREATE TABLE … PARTITION OF`).
    #[serde(default)]
    pub partition_children: Vec<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TableSnapshot {
    pub design: TableDesign,
    pub ddl: String,
    pub triggers: Vec<String>,
    #[serde(default)]
    pub revision: Option<String>,
    #[serde(default)]
    pub unknown: Vec<String>,
    #[serde(default)]
    pub attachments: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owned_sequences: Option<Vec<OwnedSequence>>,
}

impl TableSnapshot {
    pub fn new(design: TableDesign, ddl: String, triggers: Vec<String>) -> Self {
        Self {
            design,
            ddl,
            triggers,
            revision: None,
            unknown: Vec::new(),
            attachments: Vec::new(),
            owned_sequences: None,
        }
    }

    /// This metadata is absent in snapshots from legacy layouts and plugin drivers.
    pub fn owned_sequences(&self) -> Option<&[OwnedSequence]> {
        self.owned_sequences.as_deref()
    }

    pub fn with_owned_sequences(mut self, sequences: Option<Vec<OwnedSequence>>) -> Self {
        self.owned_sequences = sequences;
        self
    }

    pub fn matches_baseline(&self, before: &Self) -> bool {
        self.design == before.design
            && self.ddl == before.ddl
            && self.triggers == before.triggers
            && self.revision == before.revision
            && self.unknown == before.unknown
            && self.attachments == before.attachments
            // Legacy drafts can save without rebuilding; rebuilds require a fresh baseline.
            && (before.owned_sequences.is_none() || self.owned_sequences == before.owned_sequences)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OwnedSequence {
    pub column: String,
    pub schema: String,
    pub name: String,
    pub oid: u32,
}

impl OwnedSequence {
    pub fn new(column: String, schema: String, name: String, oid: u32) -> Self {
        Self {
            column,
            schema,
            name,
            oid,
        }
    }
}

/// Structured driver metadata keeps opaque SQL and a stable database revision.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct StructuredTableSnapshot {
    pub snapshot: TableSnapshot,
    pub revision: String,
    pub unknown: Vec<String>,
    pub attachments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ObjectSnapshot {
    pub name: String,
    pub body: String,
    pub fields: std::collections::BTreeMap<String, String>,
    pub ddl: String,
    pub revision: String,
    pub unknown: Vec<String>,
}
