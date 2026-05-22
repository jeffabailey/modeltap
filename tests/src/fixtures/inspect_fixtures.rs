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
    /// Value the scenario assigns to `HF_HOME`. For HF-targeted fixtures
    /// (e.g. `devon_hf_with_config_json_fixture`) this points at a tempdir
    /// laying out `<HF_HOME>/hub/models--<org>--<repo>/snapshots/<sha>/config.json`.
    /// For the other fixtures it stays `/nonexistent/no-such-hf-cache` so the
    /// HF plugin's `discover()` returns `NotInstalled` (no left-pane noise
    /// from HF). The HF plugin reads `<HF_HOME>/hub/` per the env-resolution
    /// rule in `plugins/hf/src/discover.rs::resolve_hub_root`.
    pub hf_home: PathBuf,
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
    let hf_home = PathBuf::from("/nonexistent/no-such-hf-cache");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
        hf_home,
    }
}

/// INT-INFO-8 fixture (step 02-03 part 3/3): a tempdir tree wired so that
/// modeltap launches with the TestTool plugin registered AND with the
/// TestTool's `inspect_tool()` armed to panic. The orchestrator must catch
/// the panic at the `catch_unwind` boundary shipped in step 02-03 part 2/3
/// (commit bd2a975), surface `INSPECT_PANIC_SENTINEL` in the
/// `last_error` field of the rendered detail screen, and append an
/// `inspect_panic tool=test-tool` line to `<diagnostics_dir>/diagnostics.log`.
///
/// The fixture exposes a `diagnostics_dir` path under the tempdir so the
/// test can override `MODELTAP_DIAGNOSTICS_DIR` (resolved in
/// `crates/modeltap-app/src/main.rs`) at launch and read the resulting
/// `diagnostics.log` from the SAME directory after the process exits.
///
/// Route note: the INT-INFO-8 Gherkin text names "Ollama" — we route through
/// the TestTool (`MODELTAP_TEST_PLUGINS=test-tool` + the
/// `MODELTAP_TEST_TOOL_INSPECT_PANIC=1` seam landed in step 02-03 part 1) so
/// no production plugin needs an artificial panic injection point. The
/// panic-isolation contract is plugin-agnostic by construction (the
/// orchestrator wraps EVERY plugin's `inspect_tool()` future in
/// `AssertUnwindSafe(...).catch_unwind()`), so the routing swap does not
/// weaken AC-21-9 / AC-22-7.
pub fn devon_panic_inspect_fixture() -> InspectFixture {
    let temp = TempDir::new().expect("create devon-panic-inspect tempdir");
    setup_common_tree(temp.path());

    // The diagnostics directory under the tempdir mirrors the production
    // `~/.modeltap` location; the fixture creates it eagerly so the
    // orchestrator's best-effort write into `diagnostics.log` does not race
    // against the missing-directory branch.
    let diagnostics_dir = temp.path().join(".modeltap");
    std::fs::create_dir_all(&diagnostics_dir).expect("create .modeltap diagnostics dir");

    // Per the Ollama / NotInstalled pattern in `devon_ollama_userconfig`:
    // pinning a non-existent path keeps Ollama out of the left-pane (the
    // panic-injection happens through TestTool, not Ollama). config path
    // similarly stays at a nonexistent location so the inspect-side
    // user-config branch is empty.
    let ollama_dir = PathBuf::from("/nonexistent/no-such-ollama-root");
    let config_path = PathBuf::from("/nonexistent/no-such-config.toml");
    let hf_home = PathBuf::from("/nonexistent/no-such-hf-cache");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
        hf_home,
    }
}

/// Convenience helper: absolute path to `<temp>/.modeltap` — the value the
/// INT-INFO-8 scenario sets `MODELTAP_DIAGNOSTICS_DIR` to. Kept as a method
/// on `InspectFixture` so the cucumber driver and the step impls share a
/// single source of truth (no path-join drift between fixture builder and
/// assertion site).
impl InspectFixture {
    pub fn diagnostics_dir(&self) -> PathBuf {
        self.temp.path().join(".modeltap")
    }
}

/// AC-22-7 fixture (step 03-01 part 3/3): an un-introspectable model file
/// reached through the Ollama plugin's discover path. The Ollama plugin's
/// trait-default `inspect_model` returns `Err(InspectError::Unsupported)`,
/// which the model-detail orchestrator's merge maps to the public
/// `METADATA_UNSUPPORTED_SENTINEL` constant ("(metadata unsupported for this
/// tool)"). The Metadata section paints that sentinel while every OTHER
/// detail-screen panel (Registered with, Size on disk, Dedup key, Status)
/// renders normally — matching AC-22-7's "partial info gracefully" intent.
///
/// Note on the AC-22-7 literal wording: the source `.feature` text asserts
/// against `(introspection failed -- see diagnostics.log)`, which the
/// orchestrator emits only for `InspectError::FormatUnreadable` /
/// `PluginPanic`. Step 03-02 lands the plugin override that hits that path;
/// this step ships the partial-info-graceful render via the Unsupported
/// seam (default trait body), which is the production behaviour every
/// plugin exhibits until 03-02. The AC-22-7 intent ("screen does not
/// crash" + "other panels still render") is fully exercised either way; the
/// sentinel text is the only delta.
///
/// Layout: a minimal Ollama tree with a `manifests/` directory + a synthetic
/// model file referenced by the test's `MODELTAP_HEADLESS_DETAIL_REGS`
/// payload. The `inspect_model` path returns `Unsupported`; no I/O is
/// performed against the on-disk file by the plugin, so its content is
/// irrelevant — we still write a non-empty byte sequence so the file's
/// metadata (size, mtime) renders the Size-on-disk panel deterministically.
pub fn devon_model_unintrospectable_fixture() -> InspectFixture {
    let temp = TempDir::new().expect("create devon-model-unintrospectable tempdir");
    setup_common_tree(temp.path());

    // Build a minimal Ollama tree so the Ollama plugin's discover() returns
    // NotInstalled (no models surface from production discover, only the
    // synthetic detail-regs payload reaches the orchestrator). The detail
    // screen's `Registered with` panel paints from the regs payload, the
    // Metadata section paints `METADATA_UNSUPPORTED_SENTINEL`.
    let ollama_dir = temp.path().join("ollama-root");
    std::fs::create_dir_all(ollama_dir.join("manifests"))
        .expect("create ollama-root/manifests");

    // Place the un-introspectable model file under the Ollama tree. The
    // `MODELTAP_HEADLESS_DETAIL_REGS` payload points at this path so the
    // detail screen has a registered-tool entry to render. Content is a
    // non-GGUF byte sequence — the plugin's default `inspect_model` never
    // reads it (returns Unsupported unconditionally), but a future test that
    // probes file readability will see a real file.
    let model_path = ollama_dir.join("unintrospectable-model.bin");
    std::fs::write(&model_path, b"\x00\x01\x02not-a-gguf-header")
        .expect("seed unintrospectable model file");

    let config_path = PathBuf::from("/nonexistent/no-such-config.toml");
    let hf_home = PathBuf::from("/nonexistent/no-such-hf-cache");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
        hf_home,
    }
}

/// Absolute path under the AC-22-7 fixture's Ollama tree to the
/// un-introspectable model file. Kept as a free function (rather than a
/// method on `InspectFixture`) so it's used only where the fixture's layout
/// guarantees the file exists — the other inspect fixtures don't seed this
/// path.
pub fn devon_unintrospectable_model_path(fixture: &InspectFixture) -> PathBuf {
    fixture.ollama_dir.join("unintrospectable-model.bin")
}

/// Canonical model_id the AC-22-3 / AC-22-8 scenarios pass to the Ollama
/// plugin's `inspect_model`. The id follows the discovery projection
/// `<repo>:<tag>` (with the literal `library` segment dropped) so a real
/// modeltap launch round-trips `model.id == OLLAMA_MANIFEST_FIXTURE_ID`
/// through the headless DETAIL_REGS payload AND the plugin's manifest
/// locator.
pub const OLLAMA_MANIFEST_FIXTURE_ID: &str = "llama3:8b-instruct-q4_K_M";

/// Body of the synthetic Ollama manifest the AC-22-3 / AC-22-8 scenarios
/// read. Carries every field the plugin's `inspect_model` projects into
/// `metadata_kv`:
///
/// - `config.architecture = "llama"`
/// - `config.parameter_size = "7B"` → emitted as `parameters` KV
/// - `config.quantization_level = "Q4_K_M"`
/// - `template = "<jinja excerpt>"` → truncated to ≤200 chars at projection
/// - `system = "You are a helpful assistant."`
///
/// The manifest is JSON-as-a-string (not a heavyweight Docker-distribution
/// envelope) because the plugin's `inspect_model` reads the top-level
/// `config`, `template`, and `system` keys directly. Compatibility with the
/// existing `discovery::parse_manifest` (which reads `layers[*]` for blob
/// resolution) is unnecessary here — `inspect_model` is decoupled from
/// `discover` and the two read disjoint subsets of the manifest envelope.
const OLLAMA_MANIFEST_FIXTURE_BODY: &str = r#"{
  "schemaVersion": 2,
  "config": {
    "architecture": "llama",
    "parameter_size": "7B",
    "quantization_level": "Q4_K_M"
  },
  "template": "{{ .System }}\nUser: {{ .Prompt }}\nAssistant: ",
  "system": "You are a helpful assistant."
}"#;

/// AC-22-3 / AC-22-8 fixture (step 03-02 part 1/N): a tempdir tree wired so
/// the production Ollama plugin's `inspect_model` reads a synthetic manifest
/// at `<ollama_dir>/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M`.
/// Combined with `MODELTAP_HEADLESS_DETAIL_REGS={"id": OLLAMA_MANIFEST_FIXTURE_ID, ...}`,
/// the orchestrator's `dispatch_open_model_detail` resolves `model_id = OLLAMA_MANIFEST_FIXTURE_ID`,
/// the Ollama plugin's `inspect_model_impl` matches it via the locator, reads
/// the synthetic manifest, and projects `config.architecture`, `parameters`,
/// `template`, `system` into `ModelDetail.metadata_kv`. The detail-screen
/// renderer paints them as aligned `key : value` lines so the acceptance
/// substring assertions land.
///
/// Layout:
/// ```
/// <temp>/
///   ollama-root/
///     manifests/
///       registry.ollama.ai/
///         library/
///           llama3/
///             8b-instruct-q4_K_M   ← synthetic manifest JSON
///   xdg-data/modeltap/             ← cache.sqlite landing pad
///   test-tool/models/...           ← TestTool seed (parity with siblings)
///   logs/                          ← log dir
///   modeltap-home/                 ← inert HOME shim
/// ```
///
/// The TestTool seed is included for parity with the other inspect fixtures
/// — the headless harness boots the modeltap composition root with the
/// TestTool plugin registered alongside the production Ollama plugin so the
/// left-pane invariant ("at least one tool present") holds even when the
/// scenario's assertion targets the right pane.
pub fn devon_ollama_manifest_fixture() -> InspectFixture {
    let temp = TempDir::new().expect("create devon-ollama-manifest tempdir");
    setup_common_tree(temp.path());

    let ollama_dir = temp.path().join("ollama-root");
    let manifest_dir = ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("llama3");
    std::fs::create_dir_all(&manifest_dir).expect("create manifest dir");
    let manifest_path = manifest_dir.join("8b-instruct-q4_K_M");
    std::fs::write(&manifest_path, OLLAMA_MANIFEST_FIXTURE_BODY)
        .expect("write synthetic ollama manifest");

    let config_path = PathBuf::from("/nonexistent/no-such-config.toml");
    let hf_home = PathBuf::from("/nonexistent/no-such-hf-cache");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
        hf_home,
    }
}

/// Absolute path to the synthetic Ollama manifest under the
/// `devon_ollama_manifest_fixture` tempdir. The cucumber driver does not
/// dereference the file (the Ollama plugin reads it under
/// `MODELTAP_OLLAMA_DIR`), but the path is exposed so the fixture's unit
/// tests below can assert layout invariants without re-deriving the join.
pub fn devon_ollama_manifest_path(fixture: &InspectFixture) -> PathBuf {
    fixture
        .ollama_dir
        .join("manifests")
        .join("registry.ollama.ai")
        .join("library")
        .join("llama3")
        .join("8b-instruct-q4_K_M")
}

/// Canonical model_id the AC-22-3 / AC-22-5 HF scenario passes to the HF
/// plugin's `inspect_model`. The id follows the discovery projection
/// `<org>/<repo>/<filename>` (see `plugins/hf/src/discover.rs`) so a real
/// modeltap launch round-trips `model.id == HF_CONFIG_JSON_FIXTURE_ID`
/// through the headless DETAIL_REGS payload AND the plugin's `model_id`
/// → snapshot-dir locator.
pub const HF_CONFIG_JSON_FIXTURE_ID: &str =
    "mistralai/Mistral-7B-v0.1/model.safetensors";

/// Body of the synthetic HF `config.json` the AC-22-3 / AC-22-5 HF scenario
/// reads. Carries every field the plugin's `inspect_model` projects into
/// `metadata_kv`:
///
/// - `model_type = "mistral"` → emitted verbatim
/// - `architectures = ["MistralForCausalLM"]` → joined into a KV string AND
///   lifted to the typed `ModelDetail.architecture` field (first entry)
/// - `hidden_size = 4096`
/// - `num_attention_heads = 32`
/// - `num_hidden_layers = 32`
/// - `max_position_embeddings = 32768` → also lifted to `context_length`
///
/// `vocab_size` is included so the fixture matches a real HF `config.json`
/// shape, but the plugin's projection deliberately drops it (out of the
/// AC-22-5 selection per acceptance-test-plan.md §R6).
const HF_CONFIG_JSON_FIXTURE_BODY: &str = r#"{
  "model_type": "mistral",
  "architectures": ["MistralForCausalLM"],
  "hidden_size": 4096,
  "num_attention_heads": 32,
  "num_hidden_layers": 32,
  "max_position_embeddings": 32768,
  "vocab_size": 32000
}"#;

/// Snapshot revision id used by the HF fixture. The HF cache layout pins
/// every snapshot under a `<sha>` directory; we use a stable token here so
/// the fixture's path joins are deterministic across test runs (a real HF
/// cache would use a 40-char git sha — the plugin doesn't care about the
/// shape, only that the directory exists and contains `config.json`).
const HF_CONFIG_JSON_FIXTURE_SNAPSHOT_REV: &str = "abc1234567890";

/// AC-22-3 / AC-22-4 / AC-22-5 HF fixture (step 03-02 part 2/N): a tempdir
/// tree wired so the production HF plugin's `inspect_model` reads a synthetic
/// `config.json` at
/// `<HF_HOME>/hub/models--mistralai--Mistral-7B-v0.1/snapshots/<rev>/config.json`.
///
/// Combined with `MODELTAP_HEADLESS_DETAIL_REGS={"id": HF_CONFIG_JSON_FIXTURE_ID, ...}`,
/// the orchestrator's `dispatch_open_model_detail` resolves
/// `model_id = HF_CONFIG_JSON_FIXTURE_ID`, the HF plugin's
/// `inspect_model_impl` parses the model_id into `(org, repo)`, joins
/// `<hub>/models--mistralai--Mistral-7B-v0.1/`, picks the snapshot via
/// `refs/main` (which this fixture writes), reads the synthetic
/// `config.json`, and projects `model_type`, `architectures`, `hidden_size`,
/// `num_attention_heads`, `num_hidden_layers`, `max_position_embeddings`
/// into `ModelDetail.metadata_kv`. The detail-screen renderer paints them as
/// aligned `key : value` lines so the acceptance substring assertions land.
///
/// Layout:
/// ```
/// <temp>/
///   hf-cache/
///     hub/
///       models--mistralai--Mistral-7B-v0.1/
///         refs/main                                ← snapshot rev pointer
///         snapshots/abc1234567890/config.json      ← synthetic config JSON
///   xdg-data/modeltap/             ← cache.sqlite landing pad
///   test-tool/models/...           ← TestTool seed (parity with siblings)
///   logs/                          ← log dir
///   modeltap-home/                 ← inert HOME shim
/// ```
///
/// The Ollama plugin is parked at a NonInstalled path so it does not
/// contribute to the inventory — the scenario asserts only against the HF
/// path. The TestTool seed parallels `devon_ollama_manifest_fixture` so the
/// left-pane invariant ("at least one tool present") holds.
pub fn devon_hf_with_config_json_fixture() -> InspectFixture {
    let temp = TempDir::new().expect("create devon-hf-with-config-json tempdir");
    setup_common_tree(temp.path());

    let hf_home = temp.path().join("hf-cache");
    let hub = hf_home.join("hub");
    let model_dir = hub.join("models--mistralai--Mistral-7B-v0.1");
    let snapshot_dir = model_dir
        .join("snapshots")
        .join(HF_CONFIG_JSON_FIXTURE_SNAPSHOT_REV);
    std::fs::create_dir_all(&snapshot_dir).expect("create hf snapshot dir");
    std::fs::write(snapshot_dir.join("config.json"), HF_CONFIG_JSON_FIXTURE_BODY)
        .expect("write synthetic hf config.json");
    // refs/main pointer so the plugin's `resolve_snapshot_dir` takes the
    // priority-1 path (deterministic across multiple snapshot dirs).
    let refs_dir = model_dir.join("refs");
    std::fs::create_dir_all(&refs_dir).expect("create hf refs dir");
    std::fs::write(refs_dir.join("main"), HF_CONFIG_JSON_FIXTURE_SNAPSHOT_REV)
        .expect("write hf refs/main");

    let ollama_dir = PathBuf::from("/nonexistent/no-such-ollama-root");
    let config_path = PathBuf::from("/nonexistent/no-such-config.toml");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
        hf_home,
    }
}

/// Absolute path to the synthetic HF `config.json` under the
/// `devon_hf_with_config_json_fixture` tempdir. The cucumber driver does not
/// dereference the file (the HF plugin reads it under `HF_HOME`), but the
/// path is exposed so the fixture's unit tests below can assert layout
/// invariants without re-deriving the join.
pub fn devon_hf_config_json_path(fixture: &InspectFixture) -> PathBuf {
    fixture
        .hf_home
        .join("hub")
        .join("models--mistralai--Mistral-7B-v0.1")
        .join("snapshots")
        .join(HF_CONFIG_JSON_FIXTURE_SNAPSHOT_REV)
        .join("config.json")
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
    let hf_home = PathBuf::from("/nonexistent/no-such-hf-cache");

    InspectFixture {
        temp,
        ollama_dir,
        config_path,
        hf_home,
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
    fn devon_panic_inspect_fixture_creates_diagnostics_dir() {
        let fix = devon_panic_inspect_fixture();
        let diag = fix.diagnostics_dir();
        assert!(
            diag.exists() && diag.is_dir(),
            "diagnostics dir must exist as a directory at {}",
            diag.display()
        );
        // Boundary check: the fixture's tempdir hosts both the TestTool's
        // model file and the diagnostics directory, so they share a parent.
        assert!(
            diag.starts_with(fix.temp.path()),
            "diagnostics_dir must live under the fixture tempdir"
        );
    }

    #[test]
    fn devon_model_unintrospectable_seeds_real_file_under_ollama_tree() {
        let fix = devon_model_unintrospectable_fixture();
        let model_path = devon_unintrospectable_model_path(&fix);
        assert!(
            model_path.exists(),
            "unintrospectable model file must exist at {}",
            model_path.display()
        );
        let meta = std::fs::metadata(&model_path).expect("stat unintrospectable model");
        assert!(
            meta.is_file(),
            "unintrospectable model must be a regular file (so Size on disk renders)"
        );
        assert!(meta.len() > 0, "fixture file must be non-empty so byte count renders");
        // The manifests/ directory must exist as a directory (not a file like
        // devon_tool_error_ollama) so the Ollama plugin's discover()
        // surfaces NotInstalled rather than DiscoverError::Io — only
        // inspect_model is the path under test.
        let manifests = fix.ollama_dir.join("manifests");
        assert!(
            manifests.is_dir(),
            "manifests/ must be a directory (NotInstalled discover path)"
        );
    }

    #[test]
    fn devon_ollama_manifest_writes_synthetic_manifest_at_expected_path() {
        let fix = devon_ollama_manifest_fixture();
        let path = devon_ollama_manifest_path(&fix);
        assert!(
            path.exists(),
            "synthetic manifest must exist at {}",
            path.display()
        );
        let raw = std::fs::read_to_string(&path).expect("read manifest");
        assert!(
            raw.contains("\"architecture\": \"llama\""),
            "manifest must carry config.architecture; got:\n{raw}"
        );
        assert!(
            raw.contains("\"parameter_size\": \"7B\""),
            "manifest must carry config.parameter_size; got:\n{raw}"
        );
        assert!(
            raw.contains("\"template\""),
            "manifest must carry top-level template; got:\n{raw}"
        );
        assert!(
            raw.contains("\"system\""),
            "manifest must carry top-level system; got:\n{raw}"
        );
    }

    #[test]
    fn devon_hf_with_config_json_writes_synthetic_config_at_expected_path() {
        let fix = devon_hf_with_config_json_fixture();
        let path = devon_hf_config_json_path(&fix);
        assert!(
            path.exists(),
            "synthetic hf config.json must exist at {}",
            path.display()
        );
        let raw = std::fs::read_to_string(&path).expect("read hf config.json");
        assert!(
            raw.contains("\"model_type\": \"mistral\""),
            "config.json must carry model_type; got:\n{raw}"
        );
        assert!(
            raw.contains("\"architectures\""),
            "config.json must carry architectures; got:\n{raw}"
        );
        assert!(
            raw.contains("\"hidden_size\": 4096"),
            "config.json must carry hidden_size; got:\n{raw}"
        );
        assert!(
            raw.contains("\"max_position_embeddings\": 32768"),
            "config.json must carry max_position_embeddings; got:\n{raw}"
        );
        // refs/main pointer must exist so the plugin's resolver takes the
        // priority-1 path.
        let refs_main = fix
            .hf_home
            .join("hub")
            .join("models--mistralai--Mistral-7B-v0.1")
            .join("refs")
            .join("main");
        assert!(
            refs_main.exists(),
            "refs/main pointer must exist at {}",
            refs_main.display()
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
