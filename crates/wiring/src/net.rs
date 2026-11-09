//! The port implemented over the network: where releases of this binary are published.

use ureq::Agent;
use ureq::http::StatusCode;

use worklog_domain::ports::{Releases, StoreError};

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

fn tag_of(url: &str, status: StatusCode, location: Option<&str>) -> Result<String, StoreError> {
    if !status.is_redirection() {
        return Err(StoreError::io(
            url,
            format!("answered {status}, not a redirect"),
        ));
    }
    let location = location.ok_or_else(|| StoreError::io(url, "redirected to no location"))?;
    location
        .rsplit('/')
        .next()
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| StoreError::io(url, format!("cannot read a tag from {location}")))
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
            .and_then(|value| value.to_str().ok());
        tag_of(&url, response.status(), location)
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

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://github.com/lantern/relay/releases/latest";

    fn refused(status: StatusCode, location: Option<&str>) -> String {
        tag_of(URL, status, location).unwrap_err().to_string()
    }

    #[test]
    fn a_redirect_gives_the_tag_at_the_end_of_its_location() {
        let location = "https://github.com/lantern/relay/releases/tag/v0.3.0";
        let tag = tag_of(URL, StatusCode::FOUND, Some(location));
        assert_eq!(tag, Ok("v0.3.0".to_owned()));
        let bare = refused(StatusCode::FOUND, Some("https://github.com/lantern/"));
        assert!(bare.contains("cannot read a tag"), "{bare}");
    }

    #[test]
    fn any_answer_but_a_redirect_is_refused_with_its_status() {
        let tagged = Some("https://github.com/lantern/relay/releases/tag/v0.3.0");
        for (status, location) in [
            (StatusCode::TOO_MANY_REQUESTS, None),
            (StatusCode::BAD_GATEWAY, None),
            (StatusCode::NOT_FOUND, None),
            (StatusCode::OK, tagged),
        ] {
            let refused = refused(status, location);
            assert!(refused.contains(&status.to_string()), "{refused}");
        }
    }
}
