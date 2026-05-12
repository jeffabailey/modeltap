//! Unit tests for the folder-group pure-logic surface (Step 01-02).
//!
//! Per `docs/feature/folder-group-bulk-delete/design/component-boundaries.md`
//! § "New module: modeltap-core::logic::folder_group", `architecture-design.md`
//! §4.3, and `data-models.md` §3 (classification mapping).
//!
//! Test-budget calculation (per `quality-framework`):
//!   distinct behaviors:
//!     B1: `group_by_hf_repo` partitions HF inventory by `<author>/<repo>`
//!         prefix of `id_in_tool` into one `FolderGroup` per repo, pairing
//!         each with sidecars supplied by the HF plugin.
//!     B2: `classify_unique_vs_shared` puts every `Shared`-indicator model
//!         in the `shared` bucket paired with its other-tool list (D-FGD-4 /
//!         AC-13).
//!     B3: `classify_unique_vs_shared` puts `Compatible | FormatLocked |
//!         Unknown` indicators in the `unique` bucket (conservative-when-
//!         uncertain — data-models §3 mapping).
//!     B4: `build_folder_delete_plan` enforces INT-FGD-2: the file-count
//!         invariant (paths_to_unlink_fully + paths_to_unlink_hf_only sum to
//!         folder.file_count()).
//!     B5: `build_folder_delete_plan` enforces INT-FGD-3:
//!         `bytes_to_reclaim + bytes_to_retain == folder.total_bytes()`
//!         within 1-byte rounding.
//!     B6: PROPERTY — for any synthetic inventory composed exclusively of
//!         `DedupKey::Tentative`, `classify_unique_vs_shared` MUST NOT yield
//!         a `shared` classification (R1 mitigation from architecture-design
//!         §10 / ADR-002 conservative-when-uncertain).
//!   budget = 6 × 2 = 12 unit tests max. We use 6 (one per behavior; the
//!   property test counts as one even though proptest runs it ≥ 256 cases).
//!
//! These are port-to-port at domain scope: each pure function IS its own
//! driving port (the function signature is the public interface).

use std::collections::BTreeMap;
use std::path::PathBuf;

use modeltap_core::domain::RowIndicator;
use modeltap_core::logic::compatibility::{Inventory, InventoryEntry, PluginCapabilityMap};
use modeltap_core::logic::folder_group::{
    build_folder_delete_plan, classify_unique_vs_shared, group_by_hf_repo,
};
use modeltap_core::types::{FolderGroup, Sidecar, SidecarKind};
use modeltap_core::{
    ContentHash, DedupKey, DiscoveredModel, DisplayLabel, Format, ModelMeta, ModelStatus, ToolId,
};
use proptest::prelude::*;

const HASH_A: ContentHash = ContentHash([0xAA; 32]);
const HASH_B: ContentHash = ContentHash([0xBB; 32]);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn hf_model(repo: &str, file: &str, size: u64, dedup_key: DedupKey) -> ModelMeta {
    let id = format!("{repo}/{file}");
    let (author, name) = repo.split_once('/').expect("test fixture: author/repo");
    ModelMeta {
        tool: ToolId("hf"),
        id_in_tool: id.clone(),
        on_disk_path: PathBuf::from(format!(
            "/hf/hub/models--{author}--{name}/snapshots/abc/{file}"
        )),
        size_bytes: size,
        format: Format::Gguf,
        display_label: DisplayLabel::from(id),
        status: ModelStatus::Healthy,
        dedup_key,
    }
}

fn sidecar(repo: &str, filename: &str, kind: SidecarKind, size: u64) -> Sidecar {
    let (author, name) = repo.split_once('/').expect("test fixture: author/repo");
    Sidecar {
        path: PathBuf::from(format!(
            "/hf/hub/models--{author}--{name}/snapshots/abc/{filename}"
        )),
        size_bytes: size,
        kind,
    }
}

fn meta_to_entry(m: &ModelMeta) -> InventoryEntry {
    let content_hash = match &m.dedup_key {
        DedupKey::Content(h) => Some(*h),
        DedupKey::Tentative(_) => None,
    };
    InventoryEntry {
        tool: m.tool,
        model: DiscoveredModel {
            id_in_tool: m.id_in_tool.clone(),
            on_disk_path: m.on_disk_path.clone(),
            size_bytes: m.size_bytes,
            format: m.format,
            display_label: m.display_label.clone(),
            status: m.status.clone(),
        },
        content_hash,
    }
}

fn peer_entry(tool: &'static str, id: &str, format: Format, hash: ContentHash) -> InventoryEntry {
    InventoryEntry {
        tool: ToolId(tool),
        model: DiscoveredModel {
            id_in_tool: id.to_string(),
            on_disk_path: PathBuf::from(format!("/{tool}/{id}")),
            size_bytes: 1_000,
            format,
            display_label: DisplayLabel::from(id),
            status: ModelStatus::Healthy,
        },
        content_hash: Some(hash),
    }
}

fn default_caps() -> PluginCapabilityMap {
    let mut m = PluginCapabilityMap::new();
    m.insert(ToolId("hf"), vec![Format::Gguf, Format::Safetensors]);
    m.insert(ToolId("ollama"), vec![Format::Gguf, Format::OllamaBlob]);
    m.insert(ToolId("llama-cli"), vec![Format::Gguf]);
    m.insert(ToolId("lm-studio"), vec![Format::Gguf]);
    m
}

fn empty_sidecars() -> BTreeMap<String, Vec<Sidecar>> {
    BTreeMap::new()
}

// ---------------------------------------------------------------------------
// B1: group_by_hf_repo partitions a mixed-author inventory by <author>/<repo>
// ---------------------------------------------------------------------------

/// Given an HF inventory with TWO repos (one with 2 files, one with 1 file),
/// `group_by_hf_repo` returns 2 `FolderGroup`s — one per repo — and pairs each
/// with the sidecars the caller supplied for that repo's path key.
///
/// Deterministic order (BTreeMap iteration → alphabetic).
#[test]
fn group_by_hf_repo_partitions_mixed_author_inventory() {
    let models = vec![
        hf_model(
            "bartowski/demo",
            "demo.Q4_K_M.gguf",
            1_000,
            DedupKey::Content(HASH_A),
        ),
        hf_model(
            "meta-llama/Llama-3",
            "model.gguf",
            5_000,
            DedupKey::Content(HASH_B),
        ),
        hf_model(
            "bartowski/demo",
            "demo.Q8_0.gguf",
            2_000,
            DedupKey::Tentative(DisplayLabel::from("demo.Q8_0.gguf")),
        ),
    ];
    let mut sidecars = BTreeMap::new();
    sidecars.insert(
        "bartowski/demo".to_string(),
        vec![sidecar(
            "bartowski/demo",
            "README.md",
            SidecarKind::Readme,
            100,
        )],
    );
    sidecars.insert(
        "meta-llama/Llama-3".to_string(),
        vec![sidecar(
            "meta-llama/Llama-3",
            "config.json",
            SidecarKind::Other,
            50,
        )],
    );

    let groups = group_by_hf_repo(&models, &sidecars);

    assert_eq!(groups.len(), 2, "expected one FolderGroup per repo");
    // Alphabetic ordering (BTreeMap-keyed grouping by path).
    assert_eq!(groups[0].path, "bartowski/demo");
    assert_eq!(groups[0].tool, ToolId("hf"));
    assert_eq!(
        groups[0].models.len(),
        2,
        "bartowski/demo has 2 model files"
    );
    assert_eq!(groups[0].sidecars.len(), 1, "bartowski/demo: 1 sidecar");
    assert_eq!(groups[0].file_count(), 3, "2 models + 1 sidecar");
    assert_eq!(groups[0].total_bytes(), 1_000 + 2_000 + 100);

    assert_eq!(groups[1].path, "meta-llama/Llama-3");
    assert_eq!(groups[1].models.len(), 1);
    assert_eq!(groups[1].sidecars.len(), 1);
    assert_eq!(groups[1].file_count(), 2);

    // Sanity: empty inventory yields empty Vec.
    assert!(group_by_hf_repo(&[], &empty_sidecars()).is_empty());
}

// ---------------------------------------------------------------------------
// B2: classify_unique_vs_shared routes Shared models into `shared` with
//     other_tools — single source of truth invariant (D-FGD-4 / AC-13).
// ---------------------------------------------------------------------------

/// A folder with 3 models: one whose SHA256 matches a peer in Ollama → Shared.
/// The other two are single-tool, format-compatible → unique. The function
/// MUST drive each decision through `compatibility::compute_indicator`.
#[test]
fn classify_unique_vs_shared_routes_shared_indicator_into_shared_bucket() {
    let shared_model = hf_model(
        "bartowski/demo",
        "shared.gguf",
        1_000,
        DedupKey::Content(HASH_A),
    );
    let unique_model_1 = hf_model(
        "bartowski/demo",
        "unique-a.gguf",
        2_000,
        DedupKey::Content(HASH_B),
    );
    let unique_model_2 = hf_model(
        "bartowski/demo",
        "unique-b.gguf",
        3_000,
        DedupKey::Tentative(DisplayLabel::from("unique-b.gguf")),
    );

    let folder = FolderGroup::new(
        "bartowski/demo".to_string(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("hf"),
        vec![
            shared_model.clone(),
            unique_model_1.clone(),
            unique_model_2.clone(),
        ],
        vec![],
    )
    .expect("fixture folder");

    // Inventory: the folder's three models + one Ollama peer sharing HASH_A.
    let inventory = Inventory {
        entries: vec![
            meta_to_entry(&shared_model),
            meta_to_entry(&unique_model_1),
            meta_to_entry(&unique_model_2),
            peer_entry("ollama", "mistral:7b", Format::Gguf, HASH_A),
        ],
    };
    let caps = default_caps();

    let classification = classify_unique_vs_shared(&folder, &inventory, &caps);

    assert_eq!(
        classification.shared.len(),
        1,
        "exactly one model matches the Ollama peer's hash"
    );
    assert_eq!(
        classification.shared[0].model.id_in_tool,
        "bartowski/demo/shared.gguf"
    );
    assert_eq!(
        classification.shared[0].other_tools,
        vec![ToolId("ollama")],
        "other_tools must list every peer tool"
    );
    assert_eq!(
        classification.unique.len(),
        2,
        "the other two models go to unique"
    );
    let unique_ids: Vec<&str> = classification
        .unique
        .iter()
        .map(|m| m.id_in_tool.as_str())
        .collect();
    assert!(unique_ids.contains(&"bartowski/demo/unique-a.gguf"));
    assert!(unique_ids.contains(&"bartowski/demo/unique-b.gguf"));
}

// ---------------------------------------------------------------------------
// B3: Compatible/FormatLocked/Unknown indicators map to `unique` (data-models §3)
// ---------------------------------------------------------------------------

/// Each non-Shared `RowIndicator` value MUST map to the `unique` bucket
/// (conservative-when-uncertain). Tests all three: Compatible, FormatLocked,
/// Unknown.
#[test]
fn classify_unique_vs_shared_routes_non_shared_indicators_into_unique() {
    // Model with a Compatible indicator: single-tool, format accepted by
    // another plugin (Gguf accepted by ollama/llama-cli).
    let compatible = hf_model(
        "x/y",
        "compat.gguf",
        1_000,
        DedupKey::Content(HASH_A), // distinct hash, no peer match
    );
    // Model with a FormatLocked indicator: single-tool, format unique to HF.
    let format_locked = ModelMeta {
        format: Format::Awq,
        ..hf_model("x/y", "fmt-locked.awq", 2_000, DedupKey::Content(HASH_B))
    };
    // Model with an Unknown indicator: format Other.
    let unknown = ModelMeta {
        format: Format::Other,
        ..hf_model(
            "x/y",
            "mystery.bin",
            3_000,
            DedupKey::Tentative(DisplayLabel::from("mystery.bin")),
        )
    };

    let folder = FolderGroup::new(
        "x/y".to_string(),
        PathBuf::from("/hf/hub/models--x--y"),
        ToolId("hf"),
        vec![compatible.clone(), format_locked.clone(), unknown.clone()],
        vec![],
    )
    .expect("fixture folder");

    // Capability map: Awq accepted only by hf (FormatLocked path); Gguf by
    // ollama + llama-cli (Compatible path); Other handled by Rule 1.
    let mut caps = PluginCapabilityMap::new();
    caps.insert(ToolId("hf"), vec![Format::Gguf, Format::Awq, Format::Other]);
    caps.insert(ToolId("ollama"), vec![Format::Gguf]);
    caps.insert(ToolId("llama-cli"), vec![Format::Gguf]);

    let inventory = Inventory {
        entries: vec![
            meta_to_entry(&compatible),
            meta_to_entry(&format_locked),
            meta_to_entry(&unknown),
        ],
    };

    let classification = classify_unique_vs_shared(&folder, &inventory, &caps);

    assert!(
        classification.shared.is_empty(),
        "no peer with matching hash exists → shared must be empty"
    );
    assert_eq!(
        classification.unique.len(),
        3,
        "all three indicator variants (Compatible, FormatLocked, Unknown) → unique"
    );

    // Sanity: cross-check by computing indicators directly.
    use modeltap_core::logic::compatibility::compute_indicator;
    let r_compat = compute_indicator(&meta_to_entry(&compatible), &inventory, &caps);
    assert_eq!(
        r_compat,
        RowIndicator::Compatible,
        "fixture must exercise Compatible branch; got {r_compat:?}"
    );
    let r_locked = compute_indicator(&meta_to_entry(&format_locked), &inventory, &caps);
    assert_eq!(
        r_locked,
        RowIndicator::FormatLocked,
        "fixture must exercise FormatLocked branch; got {r_locked:?}"
    );
    let r_unknown = compute_indicator(&meta_to_entry(&unknown), &inventory, &caps);
    assert_eq!(
        r_unknown,
        RowIndicator::Unknown,
        "fixture must exercise Unknown branch; got {r_unknown:?}"
    );
}

// ---------------------------------------------------------------------------
// B4: build_folder_delete_plan satisfies INT-FGD-2 (file-count invariant)
// ---------------------------------------------------------------------------

/// `paths_to_unlink_fully.len() + paths_to_unlink_hf_only.len() ==
/// folder.file_count()`. Fully-unlinked paths = unique models + sidecars;
/// hf-only-unlinked paths = shared models.
#[test]
fn build_folder_delete_plan_satisfies_file_count_invariant() {
    let shared_model = hf_model(
        "bartowski/demo",
        "shared.gguf",
        1_000,
        DedupKey::Content(HASH_A),
    );
    let unique_model = hf_model(
        "bartowski/demo",
        "unique.gguf",
        2_500,
        DedupKey::Content(HASH_B),
    );
    let sidecars = vec![
        sidecar("bartowski/demo", "README.md", SidecarKind::Readme, 100),
        sidecar("bartowski/demo", "config.json", SidecarKind::Other, 50),
    ];
    let folder = FolderGroup::new(
        "bartowski/demo".to_string(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("hf"),
        vec![shared_model.clone(), unique_model.clone()],
        sidecars.clone(),
    )
    .expect("fixture folder");
    assert_eq!(folder.file_count(), 4, "2 models + 2 sidecars");

    let inventory = Inventory {
        entries: vec![
            meta_to_entry(&shared_model),
            meta_to_entry(&unique_model),
            peer_entry("ollama", "mistral:7b", Format::Gguf, HASH_A),
        ],
    };
    let classification = classify_unique_vs_shared(&folder, &inventory, &default_caps());
    let plan = build_folder_delete_plan(&folder, &classification);

    // INT-FGD-2: total paths == folder.file_count().
    assert_eq!(
        plan.paths_to_unlink_fully.len() + plan.paths_to_unlink_hf_only.len(),
        folder.file_count(),
        "INT-FGD-2: path totals must match folder.file_count()"
    );
    // unique (1) + sidecars (2) = 3 fully-unlinked paths.
    assert_eq!(plan.paths_to_unlink_fully.len(), 3);
    // shared (1) = 1 hf-only path.
    assert_eq!(plan.paths_to_unlink_hf_only.len(), 1);

    // The shared model's path is the one in paths_to_unlink_hf_only.
    assert_eq!(
        plan.paths_to_unlink_hf_only[0], shared_model.on_disk_path,
        "shared model's HF-side path lands in paths_to_unlink_hf_only"
    );
}

// ---------------------------------------------------------------------------
// B5: build_folder_delete_plan satisfies INT-FGD-3 (reclaim+retain == total)
// ---------------------------------------------------------------------------

/// `bytes_to_reclaim` = unique-models bytes + all sidecar bytes;
/// `bytes_to_retain` = shared-models bytes. Sum equals folder.total_bytes()
/// within 1-byte rounding (AC-7 / INT-FGD-3).
#[test]
fn build_folder_delete_plan_satisfies_reclaim_plus_retain_total_invariant() {
    let shared = hf_model(
        "bartowski/demo",
        "shared.gguf",
        7_000,
        DedupKey::Content(HASH_A),
    );
    let unique = hf_model(
        "bartowski/demo",
        "unique.gguf",
        12_000,
        DedupKey::Content(HASH_B),
    );
    let sidecars = vec![
        sidecar("bartowski/demo", "README.md", SidecarKind::Readme, 300),
        sidecar("bartowski/demo", "license.txt", SidecarKind::Other, 200),
    ];
    let folder = FolderGroup::new(
        "bartowski/demo".to_string(),
        PathBuf::from("/hf/hub/models--bartowski--demo"),
        ToolId("hf"),
        vec![shared.clone(), unique.clone()],
        sidecars,
    )
    .expect("fixture folder");
    assert_eq!(folder.total_bytes(), 19_500);

    let inventory = Inventory {
        entries: vec![
            meta_to_entry(&shared),
            meta_to_entry(&unique),
            peer_entry("ollama", "shared-peer", Format::Gguf, HASH_A),
        ],
    };
    let classification = classify_unique_vs_shared(&folder, &inventory, &default_caps());
    let plan = build_folder_delete_plan(&folder, &classification);

    // bytes_to_reclaim = unique (12_000) + sidecars (300 + 200) = 12_500.
    assert_eq!(plan.bytes_to_reclaim, 12_500);
    // bytes_to_retain = shared (7_000).
    assert_eq!(plan.bytes_to_retain, 7_000);
    // INT-FGD-3 invariant.
    let sum = plan.bytes_to_reclaim + plan.bytes_to_retain;
    let total = folder.total_bytes();
    assert!(
        sum.abs_diff(total) <= 1,
        "INT-FGD-3: reclaim ({}) + retain ({}) = {} must equal total ({}) within 1-byte rounding",
        plan.bytes_to_reclaim,
        plan.bytes_to_retain,
        sum,
        total
    );
}

// ---------------------------------------------------------------------------
// B6 / PROPERTY: Tentative dedup keys NEVER yield Shared classification
//                (R1 mitigation / ADR-002 conservative-when-uncertain)
// ---------------------------------------------------------------------------

prop_compose! {
    fn arb_tentative_model(repo: &'static str)(
        idx in 0u32..1000u32,
        size in 1u64..1_000_000u64,
    ) -> ModelMeta {
        let filename = format!("file-{idx}.gguf");
        hf_model(
            repo,
            &filename,
            size,
            DedupKey::Tentative(DisplayLabel::from(filename.clone())),
        )
    }
}

prop_compose! {
    fn arb_peer_with_hash()(
        tool_idx in 0u8..3u8,
        hash_idx in 0u8..2u8,
        id_seed in 0u32..1000u32,
    ) -> InventoryEntry {
        let tool = match tool_idx {
            0 => "ollama",
            1 => "llama-cli",
            _ => "lm-studio",
        };
        let hash = if hash_idx == 0 { HASH_A } else { HASH_B };
        peer_entry(tool, &format!("peer-{id_seed}"), Format::Gguf, hash)
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 256,
        .. ProptestConfig::default()
    })]

    /// PROPERTY (architecture-design §10 R1 mitigation): a folder whose models
    /// ALL carry `DedupKey::Tentative` MUST NOT produce any `shared`
    /// classification, regardless of the peer inventory composition. This pins
    /// the ADR-002 conservative-when-uncertain contract through the
    /// folder-group classification surface.
    #[test]
    fn tentative_dedup_keys_never_yield_shared_classification(
        models in prop::collection::vec(arb_tentative_model("bartowski/demo"), 1..6),
        peers in prop::collection::vec(arb_peer_with_hash(), 0..6),
    ) {
        let folder = FolderGroup::new(
            "bartowski/demo".to_string(),
            PathBuf::from("/hf/hub/models--bartowski--demo"),
            ToolId("hf"),
            models.clone(),
            vec![],
        )
        .expect("fixture folder");

        let mut entries: Vec<InventoryEntry> = models.iter().map(meta_to_entry).collect();
        entries.extend(peers);
        let inventory = Inventory { entries };

        let classification = classify_unique_vs_shared(&folder, &inventory, &default_caps());

        prop_assert!(
            classification.shared.is_empty(),
            "Tentative dedup keys must never yield Shared classification (ADR-002); got {} shared entries",
            classification.shared.len()
        );
        prop_assert_eq!(
            classification.unique.len(),
            models.len(),
            "every Tentative-key model must land in unique"
        );
    }
}
