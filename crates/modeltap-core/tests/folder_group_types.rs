//! Unit tests for the folder-group bulk-delete type surface (Step 01-01).
//!
//! Per `docs/feature/folder-group-bulk-delete/design/data-models.md` §§1–4 & §7,
//! and per ADR-010 (`Tool::delete_folder` default body).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: `FolderGroup` construction enforces the `<author>/<repo>` path
//!         regex AND `tool == ToolId("hf")` (smart-constructor invariants).
//!     B2: `FolderDeletePlan` invariants — reclaim + retain ≈ total within
//!         1-byte rounding (INT-FGD-3).
//!     B3: `DeleteError::Unsupported` carries the offending `ToolId`.
//!     B4: `Tool::delete_folder` default body returns
//!         `Err(DeleteError::Unsupported { tool: self.name() })` for any
//!         concrete plugin that does NOT override it.
//!   budget = 4 × 2 = 8 tests max. We use 4 (one per behavior, all using
//!   parametrized inputs where multiple variants exercise the same outcome).
//!
//! These tests are port-to-port at the domain scope: each calls the type's
//! public smart-constructor / public method / trait method directly. The
//! function signature IS the driving port for a pure-data type.

use std::path::PathBuf;

use modeltap_core::types::{
    FolderClassification, FolderDeletePlan, FolderGroup, Sidecar, SidecarKind,
};
use modeltap_core::{
    DedupKey, DeleteError, DisplayLabel, Format, ModelMeta, ModelStatus, Tool, ToolId,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hf_model(repo: &str, file: &str, size: u64) -> ModelMeta {
    let id = format!("{repo}/{file}");
    ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: id.clone(),
        on_disk_path: PathBuf::from(format!(
            "/hf/hub/{}/snapshots/abc/{file}",
            encode_repo(repo)
        )),
        size_bytes: size,
        format: Format::Gguf,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
        dedup_key: DedupKey::Tentative(DisplayLabel::from(file)),
    }
}

fn encode_repo(repo: &str) -> String {
    let (author, name) = repo.split_once('/').expect("test fixture: author/repo");
    format!("models--{author}--{name}")
}

fn sidecar(filename: &str, kind: SidecarKind, size: u64) -> Sidecar {
    Sidecar {
        path: PathBuf::from(format!("/hf/hub/models--bartowski--demo/{filename}")),
        size_bytes: size,
        kind,
    }
}

// ---------------------------------------------------------------------------
// B1: FolderGroup construction enforces invariants
// ---------------------------------------------------------------------------

/// FolderGroup::new accepts a canonical `<author>/<repo>` path with the HF
/// tool id and yields a value whose `total_bytes` and `file_count` agree with
/// the supplied models + sidecars (INT-FGD-2). Rejects malformed paths and
/// wrong tool ids by returning an error.
#[test]
fn folder_group_construction_enforces_path_regex_and_hf_tool() {
    // Happy: canonical author/repo, HF tool — succeeds.
    let models = vec![hf_model("bartowski/demo", "demo.Q4_K_M.gguf", 1_000)];
    let sidecars = vec![sidecar("README.md", SidecarKind::Readme, 200)];
    let group = FolderGroup::new(
        "bartowski/demo".to_string(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("hf"),
        models,
        sidecars,
    )
    .expect("canonical hf folder-group must construct");
    assert_eq!(group.path, "bartowski/demo");
    assert_eq!(group.tool, ToolId("hf"));
    assert_eq!(group.file_count(), 2, "1 model + 1 sidecar");
    assert_eq!(group.total_bytes(), 1_200, "1000 + 200");

    // Rejection: empty path string.
    let err = FolderGroup::new(
        String::new(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("hf"),
        vec![],
        vec![],
    )
    .expect_err("empty path must be rejected");
    assert!(matches!(
        err,
        modeltap_core::types::FolderGroupError::InvalidPath { .. }
    ));

    // Rejection: multi-slash path (more than one author/repo segment).
    let err = FolderGroup::new(
        "a/b/c".to_string(),
        PathBuf::from("/hf/hub/models--a--b"),
        ToolId("hf"),
        vec![],
        vec![],
    )
    .expect_err("multi-segment path must be rejected");
    assert!(matches!(
        err,
        modeltap_core::types::FolderGroupError::InvalidPath { .. }
    ));

    // Rejection: wrong tool id (B-FGD-1 invariant — HF only in v1).
    let err = FolderGroup::new(
        "bartowski/demo".to_string(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("ollama"),
        vec![],
        vec![],
    )
    .expect_err("non-hf tool must be rejected");
    assert!(matches!(
        err,
        modeltap_core::types::FolderGroupError::WrongTool { .. }
    ));
}

// ---------------------------------------------------------------------------
// B2: FolderDeletePlan invariants
// ---------------------------------------------------------------------------

/// FolderDeletePlan::new enforces that `bytes_to_reclaim + bytes_to_retain`
/// equals `folder.total_bytes()` within a 1-byte rounding tolerance
/// (AC-7 / INT-FGD-3).
#[test]
fn folder_delete_plan_invariants_reclaim_plus_retain_equals_total_within_one_byte() {
    let models = vec![
        hf_model("bartowski/demo", "a.gguf", 1_000),
        hf_model("bartowski/demo", "b.gguf", 2_500),
    ];
    let sidecars = vec![sidecar("README.md", SidecarKind::Readme, 200)];
    let folder = FolderGroup::new(
        "bartowski/demo".to_string(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("hf"),
        models.clone(),
        sidecars.clone(),
    )
    .expect("fixture folder");
    // total = 1000 + 2500 + 200 = 3700.
    assert_eq!(folder.total_bytes(), 3_700);

    let unique = models.clone();
    let classification = FolderClassification {
        unique: unique.clone(),
        shared: vec![],
    };
    let paths_to_unlink_fully: Vec<PathBuf> = unique
        .iter()
        .map(|m| m.on_disk_path.clone())
        .chain(sidecars.iter().map(|s| s.path.clone()))
        .collect();

    // Exact split (reclaim=3700, retain=0) — accepted.
    let plan = FolderDeletePlan::new(
        folder.clone(),
        classification.clone(),
        paths_to_unlink_fully.clone(),
        vec![],
        3_700,
        0,
    )
    .expect("exact split must validate");
    assert_eq!(plan.bytes_to_reclaim + plan.bytes_to_retain, 3_700);

    // 1-byte rounding: reclaim=3699, retain=0 — accepted (within tolerance).
    let plan_within = FolderDeletePlan::new(
        folder.clone(),
        classification.clone(),
        paths_to_unlink_fully.clone(),
        vec![],
        3_699,
        0,
    )
    .expect("1-byte rounding must validate");
    assert_eq!(plan_within.bytes_to_reclaim, 3_699);

    // 2-byte mismatch — REJECTED.
    let err = FolderDeletePlan::new(
        folder.clone(),
        classification,
        paths_to_unlink_fully,
        vec![],
        3_698,
        0,
    )
    .expect_err("2-byte mismatch must be rejected");
    assert!(matches!(
        err,
        modeltap_core::types::FolderDeletePlanError::ReclaimRetainMismatch { .. }
    ));
}

// ---------------------------------------------------------------------------
// B3: DeleteError::Unsupported carries the offending ToolId
// ---------------------------------------------------------------------------

/// The new `DeleteError::Unsupported` variant carries the offending plugin's
/// `ToolId`. Display rendering uses the tool name (per data-models §6).
#[test]
fn delete_error_unsupported_carries_tool_id() {
    let err = DeleteError::Unsupported {
        tool: ToolId("ollama"),
    };
    match &err {
        DeleteError::Unsupported { tool } => assert_eq!(*tool, ToolId("ollama")),
        other => panic!("expected Unsupported, got {other:?}"),
    }
    let rendered = format!("{err}");
    assert!(
        rendered.contains("ollama"),
        "Display impl must include the tool id; got {rendered:?}"
    );
    assert!(
        rendered.contains("folder"),
        "Display impl must mention folder-delete unsupported; got {rendered:?}"
    );
}

// ---------------------------------------------------------------------------
// B4: Tool::delete_folder default body returns Unsupported
// ---------------------------------------------------------------------------

/// A concrete plugin that implements `Tool` but does NOT override
/// `delete_folder` must inherit the default body, which returns
/// `Err(DeleteError::Unsupported { tool: self.name() })` (per ADR-010 / §7).
#[test]
fn tool_delete_folder_default_body_returns_unsupported_for_any_plugin() {
    use async_trait::async_trait;
    use modeltap_core::{DeleteOutcome, DiscoverError, DiscoveredModel, LinkError, LinkOutcome};
    use std::path::Path;

    /// Minimal stub plugin. Implements every required method with a stub
    /// body; deliberately does NOT override `delete_folder` so the default
    /// body is the one under test.
    struct StubPlugin;

    #[async_trait]
    impl Tool for StubPlugin {
        fn name(&self) -> ToolId {
            ToolId("stub")
        }
        fn accepted_formats(&self) -> &'static [Format] {
            &[Format::Gguf]
        }
        async fn discover(&self) -> Result<Vec<DiscoveredModel>, DiscoverError> {
            Ok(vec![])
        }
        async fn link(
            &self,
            _canonical_src: &Path,
            _model: &ModelMeta,
        ) -> Result<LinkOutcome, LinkError> {
            Err(LinkError::NotYetImplemented("stub".to_string()))
        }
        async fn delete_one(&self, _model: &ModelMeta) -> Result<DeleteOutcome, DeleteError> {
            Err(DeleteError::NotYetImplemented("stub".to_string()))
        }
        async fn delete_all(&self) -> Result<Vec<DeleteOutcome>, DeleteError> {
            Err(DeleteError::NotYetImplemented("stub".to_string()))
        }
        // NOTE: no `delete_folder` override — the default body is what we test.
    }

    // Synthesize a trivial plan; the default body never inspects it.
    let folder = FolderGroup::new(
        "x/y".to_string(),
        PathBuf::from("/hf/hub/models--x--y"),
        ToolId("hf"),
        vec![],
        vec![],
    )
    .expect("trivial folder must construct");
    let plan = FolderDeletePlan::new(
        folder,
        FolderClassification {
            unique: vec![],
            shared: vec![],
        },
        vec![],
        vec![],
        0,
        0,
    )
    .expect("trivial plan must construct");

    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    let stub = StubPlugin;
    let result = runtime.block_on(stub.delete_folder(&plan));
    match result {
        Err(DeleteError::Unsupported { tool }) => assert_eq!(tool, ToolId("stub")),
        other => panic!("expected Unsupported(stub), got {other:?}"),
    }
}
