# Shared Artifacts Registry — modeltap-tui

Single source of truth for every `${variable}` referenced in the journey artifacts. Untracked artifacts are the primary cause of horizontal integration failures.

## Conventions

- **Source of truth** is the canonical producer (a function, file, or process).
- **Consumers** are every place the value is displayed or referenced.
- **Integration risk** is the impact of inconsistency.

## Artifact Table

| Artifact | Source of Truth | Consumers | Integration Risk |
|---|---|---|---|
| `tool_count` | `core::Inventory.tools.len()` | left pane | LOW — displayed once |
| `tool.name` | `Plugin::name()` (per plugin impl) | left pane row, zap confirmation prompt, post-action summary, error messages | HIGH — typed-confirmation security depends on exact match |
| `tool.model_count` | `len(Plugin::list_models())` | left pane row | MEDIUM — drift hides newly-discovered models |
| `tool.disk_usage` | `sum(Model.size for m in Plugin::list_models())` | left pane row, summary bar Total | HIGH — silent inventory drift |
| `tool.unique_models` | `core::filter_unique_to(tool, inventory)` | zap dialog warning | HIGH — incorrect classification destroys data not retained anywhere |
| `total.model_count` | `sum(tool.model_count)` over all tools | summary bar | LOW |
| `total.disk_usage` | `sum(tool.disk_usage)` over all tools | summary bar | HIGH |
| `total.dedupable` | `core::Inventory::compute_dedupable_bytes()` | summary bar | MEDIUM — drives feature value perception |
| `model.id` | `Plugin::list_models()[i].id` | right pane row, detail screen | HIGH — used to disambiguate selection |
| `model.size` | `Plugin::list_models()[i].size` | right pane row, detail screen, reclaim math | HIGH |
| `model.format` | `Plugin::list_models()[i].format` | right pane row metadata, compatibility computation | HIGH — drives `!` indicator |
| `model.compatible_tools` | `core::compute_compatibility(model.format, plugins.accepted_formats)` | row indicator (o/*/!), detail screen | HIGH — wrong indicator misleads user about deletability |
| `model.dedup_key` | `core::compute_dedup_key(model)` — **STRATEGY OPEN (Q6)** | duplicate grouping, detail screen, canonical filename | CRITICAL — incorrect key conflates distinct models OR fails to dedupe true matches |
| `model.disk_paths[]` | `Plugin::list_models()[i].on_disk_path` per matching tool | detail screen, unify dialog target list | HIGH |
| `model.reclaimable_bytes` | `core::compute_reclaimable(model) = (copies-1) * size` | detail screen, unify dialog | MEDIUM |
| `canonical_path` | `config::store_root + "/" + model.dedup_key`, default `~/.modeltap/store/` | unify dialog, post-action verification, next-launch inventory | CRITICAL — drift between unify-time and inventory-time corrupts dedup detection forever |
| `hardlink_targets[]` | `Plugin::link_path_for(model)` per registered tool | unify dialog, post-action verification | HIGH |
| `running_tools[]` | `core::detect_running_tools()` (lsof or platform equivalent) | unify warning, zap warning | LOW — informational only in v1 |
| `last_action.bytes_reclaimed` | result of `Action::Unify` or `Action::Zap` | post-action right pane, summary bar delta | MEDIUM |
| `last_action.bytes_retained` | shared-models bytes kept after zap | post-action right pane | MEDIUM |
| `keyboard_shortcuts` | `ui::shortcuts::SHORTCUT_TABLE` (single const) | bottom bar, help screen, all dialog footers | HIGH — inconsistency makes the app feel buggy |
| `cli_vocabulary` | `docs/feature/modeltap-tui/discuss/journey-cleanup-and-unify-visual.md` (CLI vocabulary table) | every screen, every error message, README, --help | HIGH — terminology drift erodes user trust |

## Open Source-of-Truth Questions (DESIGN must close)

| ID | Artifact | Open question |
|---|---|---|
| Q1 | `canonical_path` | Default `store_root` confirmed as `~/.modeltap/store/` on both macOS and Linux? |
| Q2 | `hardlink_targets` | Exact `link_path_for(model)` per plugin (Ollama blob layout, llama-cli loose file, HF cache symlink farm, LM Studio config) |
| Q6 | `model.dedup_key` | Content-hash (sha256) vs HF-id+quant vs hybrid — strategy must be picked before any unify can be implemented safely |
| Q7 | `~/.modeltap/` index | Does modeltap maintain a persistent index/registry file (UMR-style) in addition to the store directory? |

## Validation Plan (during DESIGN review)

1. Every `${variable}` in `journey-*-visual.md` MUST appear in this table.
2. Every consumer of an artifact must read from the source of truth, not a hardcoded literal.
3. Cross-step appearances of the same artifact (e.g., `tool.name` in steps 1, 2, 4b, 5) MUST resolve to identical strings.
4. Open questions Q1/Q2/Q6/Q7 must be closed before any code is written for the corresponding artifact.

## Integration Checkpoints (cross-step invariants)

| Invariant | Steps involved | Failure mode |
|---|---|---|
| `total.disk_usage` == sum of `tool.disk_usage` | 1, 5 | Hidden inventory bug |
| `*` indicator implies `compatible_tools.len() >= 2` AND another tool actually has the dedup_key | 2, 3 | User offered unify on something not actually duplicated |
| `!` indicator implies `compatible_tools.len() == 1` | 2 | User shown red icon on a model that other tools could in fact accept — value undermined |
| Post-unify: every `hardlink_targets[i]` stats to the same inode as `canonical_path` | 4a | Unify silently produced copies instead of links — no disk reclaim |
| Post-zap: `Plugin::list_models()` for the zapped tool returns `[]` | 4b, 5 | Zap left orphan registrations |
| Post-action: `new total.disk_usage == old total.disk_usage - last_action.bytes_reclaimed` (rounding) | 5 | Reported reclaim is fictional |
| `keyboard_shortcuts` displayed in bottom bar matches the actual key handler dispatch table | all | App feels buggy / undiscoverable |
