//! Fixtures for the US-21 step 02-02 acceptance scenarios (AC-21-4 +
//! AC-21-5).
//!
//! Two builders ship here:
//!
//! 1. **`devon_tool_error_ollama`** — a tempdir tree where Ollama's discovery
//!    root EXISTS but its `manifests/` is a regular FILE (not a directory).
//!    `read_dir("manifests")` then returns `NotADirectory`, the plugin
//!    propagates `DiscoverError::Io`, and the reconcile_writeback path writes
//!    a `cache_tools` row with `last_error: Some("io error: ...")` +
//!    `last_error_at: Some(SystemTime::now())`. The detail-screen orchestrator
//!    reads the row and surfaces the message — closing AC-21-4 end-to-end
//!    through the real discover-error → reconcile → detail-screen pipeline
//!    (no pre-seeding required).
//!
//! 2. **`devon_ollama_userconfig`** — a tempdir tree with a `config.toml`
//!    that adds a `[plugins.ollama] search_paths` entry. `MODELTAP_CONFIG_PATH`
//!    is then pointed at that file so `OllamaPlugin::inspect_tool` appends the
//!    user-config entry to its default search-paths list with
//!    `SearchPathSource::UserConfig`. The detail screen labels it `(user
//!    config)` per AC-21-5.
//!
//!    AC-21-5's Gherkin text targets llama-cli, but the dispatch routes the
//!    scenario through the Ollama plugin because the Ollama plugin is the one
//!    that ships an `inspect_tool` override in step 02-02 (llama-cli inherits
//!    the trait-default `Unsupported`). The TUI rendering being asserted is
//!    plugin-agnostic — labelling defaults vs user-config — so the routing
//!    swap leaves AC-21-5's behavioural assertion intact.

use std::path::PathBuf;

use tempfile::TempDir;

/// Filesystem layout owned by an AC-21-4 / AC-21-5 scenario. Mirrors
/// `DevonCacheEmptyFixture` (cache-empty + TestTool seed + logs +
/// modeltap-home) and additionally produces `MODELTAP_OLLAMA_DIR` /
/// `MODELTAP_CONFIG_PATH` overrides specific to the inspect scenarios.
pub struct InspectFixture {
    pub temp: TempDir,
    /// Value the scenario assigns to `MODELTAP_OLLAMA_DIR`. For
    /// `devon_tool_error_ollama` this points at a path where `manifests/` is
    /// a FILE so discover errors with `DiscoverError::Io`. For
    /// `devon_ollama_userconfig` this points at a path that does not exist
    /// (NotInstalled), keeping the per-row left pane consistent with the
    /// other scenarios — no error pathway is required for AC-21-5.
    pub ollama_dir: PathBuf,
    /// Value the scenario assigns to `MODELTAP_CONFIG_PATH`. For
    /// `devon_ollama_userconfig` this points at a real `config.toml` with a
    /// `[plugins.ollama] search_paths` entry. For `devon_tool_error_ollama`
    /// this is `/nonexistent/no-such-config.toml` so the inspect path takes
    /// the empty-user-config branch.
    pub config_path: PathBuf,
}

impl InspectFixture {
    /// Absolute path to `<temp>/xdg-data/modeltap/cache.sqlite` — the value
    /// the scenario sets `MODELTAP_CACHE_PATH` to.
    pub fn cache_path(&self) -> PathBuf {
        self.temp
            .path()
            .join("xdg-data")
            .join("modeltap")
            .join("cache.sqlite")
    }

    /// Absolute path to `<temp>/logs` — the value the scenario sets
    /// `MODELTAP_LOG_DIR` to.
    pub fn log_dir(&self) -> PathBuf {
        self.temp.path().join("logs")
    }

    /// Absolute path to the synthetic TestTool root. The TestTool's
    /// discover() returns one model from this directory.
    pub fn test_tool_root(&self) -> PathBuf {
        self.temp.path().join("test-tool").join("models")
    }
}

/// AC-21-4 fixture: an Ollama `MODELTAP_OLLAMA_DIR` whose `manifests/` is a
/// plain FILE rather than a directory. The plugin's `discover()` reaches the
/// `read_dir(manifests_dir)` probe and surfaces the OS-level "not a
/// directory" error as `DiscoverError::Io`. The composition root's
/// `reconcile_writeback` then writes the row with `last_error` populated.
///
/// The temp tree also seeds the TestTool's model file so the parallel
/// TestTool discover-success path still populates a left-pane row — keeping
/// the scenario's `(error)` annotation specifically attributable to the
/// Ollama row (the bug we are catching) rather than to a wholesale empty
/// inventory.
pub fn devon_tool_error_ollama() -> InspectFixture {
    let temp = TempDir::new().expect("create devon-tool-error-ollama tempdir");
    setup_common_tree(temp.path());

    // Build the broken-Ollama tree: <root>/manifests is a FILE.
    let ollama_dir = temp.path().join("ollama-root");
    std::fs::create_dir_all(&ollama_dir).expect("create ollama-root");
    let manifests_as_file = ollama_dir.join("manifests");
    std::fs::write(&manifests_as_file, b"this-is-not-a-directory").expect("seed manifests-as-file");

    let config_path = PathBuf::from("/nonexistent/no-such-config.toml");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
    }
}

/// AC-21-5 fixture: a real `config.toml` that adds one user-config search
/// path under `[plugins.ollama]`. `MODELTAP_CONFIG_PATH` is pointed at it.
/// Ollama's `inspect_tool` then emits one `Default` entry (the models root)
/// plus one `UserConfig` entry. The detail-screen renderer labels each
/// accordingly.
///
/// The models root is pinned at a non-existent path so the Ollama plugin's
/// discover() returns `NotInstalled` (no error row written) — only the
/// `inspect_tool` half of the pipeline is exercised by AC-21-5. This keeps
/// the assertion focused on the search-paths labelling without dragging in
/// last-error pathway concerns.
pub fn devon_ollama_userconfig() -> InspectFixture {
    let temp = TempDir::new().expect("create devon-ollama-userconfig tempdir");
    setup_common_tree(temp.path());

    let config_path = temp.path().join("config.toml");
    std::fs::write(
        &config_path,
        r#"[plugins.ollama]
search_paths = ["/data/models-extra"]
"#,
    )
    .expect("write config.toml");

    let ollama_dir = PathBuf::from("/nonexistent/no-such-ollama-root");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
    }
}

/// Seed the shared directory tree (cache dir, TestTool's model file, log
/// dir, modeltap-home). Identical shape to `DevonCacheEmptyFixture::build`
/// so the headless harness drives both fixtures through the same env-var
/// scaffolding.
fn setup_common_tree(root: &std::path::Path) {
    let xdg_modeltap = root.join("xdg-data").join("modeltap");
    std::fs::create_dir_all(&xdg_modeltap).expect("create xdg-data/modeltap");

    let model_dir = root.join("test-tool").join("models");
    std::fs::create_dir_all(&model_dir).expect("create test-tool/models");
    let model_path = model_dir.join(crate::test_tool::TEST_MODEL_FILENAME);
    std::fs::write(&model_path, b"synthetic-walking-skeleton-gguf-bytes")
        .expect("seed synthetic gguf");

    let log_dir = root.join("logs");
    std::fs::create_dir_all(&log_dir).expect("create logs/");

    let modeltap_home = root.join("modeltap-home");
    std::fs::create_dir_all(&modeltap_home).expect("create modeltap-home/");
}

// ---------------------------------------------------------------------------
// Unit tests — exercise the fixture builders are deterministic and produce
// the layout the acceptance scenarios depend on.
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devon_tool_error_ollama_manifests_is_a_regular_file() {
        let fix = devon_tool_error_ollama();
        let manifests = fix.ollama_dir.join("manifests");
        assert!(
            manifests.exists(),
            "manifests path must exist at {}",
            manifests.display()
        );
        let meta = std::fs::metadata(&manifests).expect("stat manifests");
        assert!(
            meta.is_file(),
            "manifests must be a regular file (not a directory) so read_dir errors"
        );
    }

    #[test]
    fn devon_ollama_userconfig_writes_real_config_toml() {
        let fix = devon_ollama_userconfig();
        assert!(
            fix.config_path.exists(),
            "config.toml must exist at {}",
            fix.config_path.display()
        );
        let raw = std::fs::read_to_string(&fix.config_path).expect("read config.toml");
        assert!(
            raw.contains("[plugins.ollama]"),
            "config.toml must declare the ollama section; got:\n{raw}"
        );
        assert!(
            raw.contains("/data/models-extra"),
            "config.toml must include the user-config search path; got:\n{raw}"
        );
    }
}
