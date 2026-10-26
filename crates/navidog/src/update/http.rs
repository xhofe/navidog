use super::manifest::*;

use std::time::Duration;

pub(super) fn read_updates() -> Result<String, HttpFail> {
    match http_get(&manifest_url(), Duration::from_secs(15)) {
        Ok(text) => Ok(text),
        Err(err) if err.status == Some(404) => {
            http_get(&manifest_url_stable(), Duration::from_secs(15))
        }
        Err(err) => Err(err),
    }
}

/// The index, or this file alone when it was published before `releases` existed.
pub(super) fn listed_releases(manifest: Manifest) -> Vec<UpdateRelease> {
    if !manifest.releases.is_empty() {
        return manifest.releases;
    }
    if manifest.tag.is_empty() && manifest.version.is_empty() {
        return Vec::new();
    }
    vec![UpdateRelease {
        tag: manifest.tag,
        version: manifest.version,
        prerelease: false,
        draft: false,
        published_at: String::new(),
        built_at: String::new(),
        body: manifest.body,
        assets: manifest.assets,
    }]
}

pub(super) struct HttpFail {
    pub(super) status: Option<u16>,
    pub(super) detail: String,
}

pub(super) fn http_get(url: &str, timeout: Duration) -> Result<String, HttpFail> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build()
        .into();
    let mut response = agent
        .get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|err| HttpFail {
            status: match &err {
                ureq::Error::StatusCode(code) => Some(*code),
                _ => None,
            },
            detail: err.to_string(),
        })?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|err| HttpFail {
            status: None,
            detail: err.to_string(),
        })
}

/// Last path segment of a download name. Rejects empty names, `.`/`..` and
/// anything that Windows would read as a drive prefix or device path, so the
/// joined path cannot leave the private update directory.
pub(super) fn download_file_name(name: &str) -> Option<&str> {
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(':')
        || name.chars().any(char::is_control)
    {
        None
    } else {
        Some(name)
    }
}
