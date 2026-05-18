This directory defines the high-level concepts, business logic, and architecture of this project using markdown. It is managed by [lat.md](https://www.npmjs.com/package/lat.md) — a tool that anchors source code to these definitions. Install the `lat` command with `npm i -g lat.md` and run `lat --help`.

## Sections

Index of architecture and design notes covering subsystems whose rationale is non-obvious from the source.

- [[tui-icons]] — per-tool PNG icons in the left pane via `ratatui-image`
- [[inspect-types]] — `Tool::inspect_tool` / `inspect_model` extension and `InspectError` (per ADR-016)
- [[modeltap-store]] — SQLite-backed inventory cache (per ADR-015) for warm-start paint
- [[test-plugin-seam]] — `MODELTAP_TEST_PLUGINS` env-var seam + in-process `TestTool` (cfg-gated)
- [[warm-start]] — launch-time cache-paint via spawn_blocking; MODELTAP_CACHE_PATH adapter
- [[walking-skeleton-acceptance]] — M1 two-process end-to-end pattern + CACHE introspection seam
