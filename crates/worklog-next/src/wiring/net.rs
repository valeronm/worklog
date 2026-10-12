//! The port implemented over the network: where releases of this binary are published.

use ureq::Agent;

use crate::domain::ports::{Releases, StoreError};

const REPO: &str = "valeronm/worklog";

pub struct GitHubReleases {
    agent: Agent,
}

impl GitHubReleases {
    pub fn new() -> GitHubReleases {
        GitHubReleases {
            agent: Agent::new_with_defaults(),
        }
    }
}

impl Releases for GitHubReleases {
    // `releases/latest` answers with a redirect to the tag, and the API that gives the same
    // is rate-limited for a caller with no login.
    fn latest(&self) -> Result<String, StoreError> {
        let url = format!("https://github.com/{REPO}/releases/latest");
        let response = self
            .agent
            .get(&url)
            .config()
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|e| StoreError::io(&url, e))?;
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| StoreError::io(&url, "no release to redirect to"))?;
        location
            .rsplit('/')
            .next()
            .filter(|tag| !tag.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| StoreError::io(&url, format!("cannot read a tag from {location}")))
    }

    fn fetch(&self, tag: &str, asset: &str) -> Result<Vec<u8>, StoreError> {
        let url = format!("https://github.com/{REPO}/releases/download/{tag}/{asset}");
        let mut response = self
            .agent
            .get(&url)
            .call()
            .map_err(|e| StoreError::io(&url, e))?;
        response
            .body_mut()
            .with_config()
            // The default ceiling is 10 MB, under a release binary.
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| StoreError::io(&url, e))
    }
}
