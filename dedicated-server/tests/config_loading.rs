//! Integration tests for configuration loading through the public API.

use std::path::{Path, PathBuf};

use dedicated_server::{ConfigError, ServerConfig};

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(relative)
}

#[test]
fn load_accepts_the_valid_fixture() {
    ServerConfig::load(&fixture("valid_server.toml")).expect("valid fixture must load");
}

#[test]
fn load_missing_file_reports_read_error_with_path() {
    let path = Path::new("/nonexistent/homecloud/dedicated-server.toml");

    assert!(matches!(
        ServerConfig::load(path),
        Err(ConfigError::Read { path: error_path, .. }) if error_path == path
    ));
}

#[test]
fn load_rejects_unknown_top_level_key() {
    assert!(matches!(
        ServerConfig::load(&fixture("invalid/unknown_top_level_key.toml")),
        Err(ConfigError::Parse { .. })
    ));
}
