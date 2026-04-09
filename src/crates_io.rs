//! Client for the crates.io API, used to sync crate versions into the registry.

use semver::Version;
use serde::Deserialize;
use snafu::prelude::*;
use std::io::Read;

const CRATES_IO_API_BASE: &str = "https://crates.io/api/v1";
const CRATES_IO_STATIC_BASE: &str = "https://static.crates.io/crates";
const USER_AGENT: &str = concat!(
    "gnostr-registry/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/gnostr-org/gnostr-registry)",
);

/// A minimal crates.io API client.
pub struct Client {
    inner: ureq::Agent,
}

impl Client {
    pub fn new() -> Self {
        let inner = ureq::AgentBuilder::new()
            .user_agent(USER_AGENT)
            .build();
        Self { inner }
    }

    /// Return all (non-yanked) versions of `krate` available on crates.io.
    pub fn fetch_versions(&self, krate: &str) -> Result<Vec<CrateVersion>, Error> {
        use error::*;

        let url = format!("{CRATES_IO_API_BASE}/crates/{krate}/versions");

        let response: VersionsResponse = self
            .inner
            .get(&url)
            .call()
            .context(RequestSnafu { url: &url })?
            .into_json()
            .context(DeserializeSnafu { url: &url })?;

        let versions = response
            .versions
            .into_iter()
            .filter(|v| !v.yanked)
            .collect();

        Ok(versions)
    }

    /// Download the `.crate` file for `krate` at `version`.
    pub fn download_crate(&self, krate: &str, version: &str) -> Result<Vec<u8>, Error> {
        use error::*;

        let url = format!("{CRATES_IO_STATIC_BASE}/{krate}/{krate}-{version}.crate");

        let response = self
            .inner
            .get(&url)
            .call()
            .context(RequestSnafu { url: &url })?;

        let mut data = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut data)
            .context(ReadBodySnafu { url: &url })?;

        Ok(data)
    }
}

/// One version entry returned by the crates.io versions API.
#[derive(Debug, Deserialize)]
pub struct CrateVersion {
    /// Parsed semver version number.
    #[serde(rename = "num")]
    pub num: Version,

    /// Whether this version has been yanked.
    pub yanked: bool,
}

#[derive(Debug, Deserialize)]
struct VersionsResponse {
    versions: Vec<CrateVersion>,
}

#[derive(Debug, Snafu)]
#[snafu(module)]
pub enum Error {
    #[snafu(display("HTTP request to {url} failed"))]
    Request {
        source: ureq::Error,
        url: String,
    },

    #[snafu(display("Could not deserialize response from {url}"))]
    Deserialize {
        source: std::io::Error,
        url: String,
    },

    #[snafu(display("Could not read response body from {url}"))]
    ReadBody {
        source: std::io::Error,
        url: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that the crates.io versions API returns at least one version
    /// for the `margo` crate and that version numbers can be parsed as semver.
    ///
    /// This test is marked `#[ignore]` so it does not run during a normal
    /// `cargo test` invocation (which has no network access). Run it explicitly
    /// with:
    ///
    ///   cargo test --features sync-crates-io -- --ignored
    #[test]
    #[ignore = "requires network access to crates.io"]
    fn fetch_margo_versions_from_crates_io() {
        let client = Client::new();

        let versions = client
            .fetch_versions("margo")
            .expect("should be able to fetch margo versions from crates.io");

        assert!(
            !versions.is_empty(),
            "margo should have at least one version on crates.io"
        );

        // Every version number must be valid semver (enforced by the type).
        for v in &versions {
            println!("  margo {}", v.num);
        }
    }

    /// Verify that the package version declared in Cargo.toml for this crate
    /// is present among the versions available on crates.io, confirming that
    /// the release has been published and that our version tracking is correct.
    #[test]
    #[ignore = "requires network access to crates.io"]
    fn local_version_exists_on_crates_io() {
        let local_version: Version = env!("CARGO_PKG_VERSION")
            .parse()
            .expect("CARGO_PKG_VERSION should be valid semver");

        let client = Client::new();

        let versions = client
            .fetch_versions("margo")
            .expect("should be able to fetch margo versions from crates.io");

        let found = versions.iter().any(|v| v.num == local_version);

        assert!(
            found,
            "version {local_version} (from Cargo.toml) was not found on crates.io; \
             either the crate has not been published yet or the version numbers are out of sync",
        );
    }

    /// Verify that a `.crate` file can be successfully downloaded from crates.io
    /// and is non-empty.
    #[test]
    #[ignore = "requires network access to crates.io"]
    fn download_margo_crate_from_crates_io() {
        let client = Client::new();

        // Fetch the list of versions first so we download a known-good one.
        let versions = client
            .fetch_versions("margo")
            .expect("should be able to fetch margo versions");

        let first = versions
            .first()
            .expect("margo should have at least one version");

        let data = client
            .download_crate("margo", &first.num.to_string())
            .expect("should be able to download the margo crate");

        assert!(
            !data.is_empty(),
            "downloaded .crate file should not be empty"
        );

        // A .crate file is a gzip-compressed tar archive; the first two bytes
        // of a gzip stream are always 0x1f 0x8b.
        assert_eq!(
            data[..2],
            [0x1f, 0x8b],
            "downloaded file does not look like a gzip archive"
        );
    }
}
