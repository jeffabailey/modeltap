# modeltap

Terminal UI for discovering, inspecting, and cleaning up local AI model files
across multiple tools (Ollama, Hugging Face cache, LM Studio, Atomic Chat).

## Supported platforms (v1)

| Platform | Status |
|---|---|
| macOS x86_64 (Intel) | Supported |
| macOS aarch64 (Apple Silicon) | Supported |
| Linux x86_64 | Supported |
| Linux aarch64 | Supported |
| Windows via [WSL2](https://learn.microsoft.com/windows/wsl/install) | Supported (treated as Linux) |
| Windows native (`x86_64-pc-windows-*`) | **Not supported** — refuses with exit code 64 |

WSL is treated identically to native Linux. There is no WSL-specific code path
because, from `modeltap`'s perspective, the WSL filesystem is a Linux
filesystem.

If you run the native Windows binary, `modeltap` exits 64 and prints:

```
Windows is supported only via WSL — see https://learn.microsoft.com/windows/wsl/install
```

## Installing

```sh
cargo install --path crates/modeltap-app
```

(Windows users: install WSL2 first, then run the above inside your WSL shell.)

## Continuous integration

The CI matrix (`.github/workflows/ci.yml`) runs `cargo build` + `cargo test`
on `ubuntu-latest` and `macos-latest`. Both must pass before any PR can merge.
The K3 first-paint smoke benchmark (`k3-bench` job) runs on `ubuntu-latest`
only — performance budgets are validated against a single canonical runner
to keep the signal stable.

For per-OS testing in a single CI job, set `MODELTAP_FORCE_PLATFORM` to one of:

- `macos-x86_64`
- `macos-aarch64`
- `linux-x86_64`
- `linux-aarch64`
- `windows-x86_64`

The override takes precedence over the host's compiled-in target triple. An
unrecognized value falls back to the host platform — a typo cannot brick the
binary in production.

## License

Apache-2.0 OR MIT (per workspace `Cargo.toml`).
