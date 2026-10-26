//! `latest.json` schema shared by the app and the release tooling. Keeping the
//! asset fields in one place means a new field cannot drift from what CI emits.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub os: String,
    pub arch: String,
    pub kind: String,
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub tag: String,
    pub notes: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub body: String,
}
