#!/usr/bin/env bash
# Fixture builder for modeltap-tui acceptance tests.
#
# Per `docs/feature/modeltap-tui/distill/acceptance-test-plan.md` §3, the
# fixture trees are synthetic-but-realistic temp trees: sparse files with
# correct apparent sizes, real Ollama manifest JSON pointing at blob hashes.
#
# Usage:
#   tests/fixtures/build.sh devon-multi-tool [TARGET_DIR]
#   tests/fixtures/build.sh devon-only-ollama [TARGET_DIR]
#   tests/fixtures/build.sh devon-permission-denied [TARGET_DIR]
#   tests/fixtures/build.sh devon-empty [TARGET_DIR]
#   tests/fixtures/build.sh all
#
# Idempotent: re-running on the same TARGET_DIR rebuilds it deterministically.

set -euo pipefail

NAME="${1:-}"
TARGET="${2:-tests/fixtures/.build/${NAME}}"

if [[ -z "$NAME" ]]; then
  echo "usage: $0 <fixture-name> [target-dir]" >&2
  echo "  available: devon-multi-tool devon-only-ollama devon-permission-denied devon-empty all" >&2
  exit 64
fi

# Use truncate to make sparse files. macOS truncate (BSD) and GNU truncate
# both accept -s. On macOS, BSD truncate is at /usr/bin/truncate.
sparse_file() {
  local path="$1" size="$2"
  mkdir -p "$(dirname "$path")"
  truncate -s "$size" "$path"
}

write_manifest() {
  # Args: <manifest-path> <blob-sha> <blob-size>
  # Writes a minimal Ollama-style manifest JSON with one layer entry.
  local path="$1" blob="$2" size="$3"
  mkdir -p "$(dirname "$path")"
  cat > "$path" <<MANIFEST
{
  "schemaVersion": 2,
  "mediaType": "application/vnd.docker.distribution.manifest.v2+json",
  "config": {
    "mediaType": "application/vnd.docker.container.image.v1+json",
    "digest": "sha256:${blob}",
    "size": 412
  },
  "layers": [
    {
      "mediaType": "application/vnd.ollama.image.model",
      "digest": "sha256:${blob}",
      "size": ${size}
    }
  ]
}
MANIFEST
}

build_devon_only_ollama() {
  local root="$1"
  rm -rf "$root"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/llama3"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/mistral"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/qwen2.5"
  mkdir -p "$root/.ollama/models/blobs"

  # Three distinct blobs.
  local blob_llama="8f3eaaa11111111111111111111111111111111111111111111111111111c102"
  local blob_mistral="4b9eaaa22222222222222222222222222222222222222222222222222222d203"
  local blob_qwen="2c8eaaa33333333333333333333333333333333333333333333333333333e304"

  # Sparse blob files.
  sparse_file "$root/.ollama/models/blobs/sha256-${blob_llama}" 4700000000
  sparse_file "$root/.ollama/models/blobs/sha256-${blob_mistral}" 4400000000
  sparse_file "$root/.ollama/models/blobs/sha256-${blob_qwen}" 8900000000

  # Manifests.
  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M" "$blob_llama" 4700000000
  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/mistral/7b-instruct-q4_K_M" "$blob_mistral" 4400000000
  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/qwen2.5/14b-q4_K_M" "$blob_qwen" 8900000000
}

build_devon_multi_tool() {
  local root="$1"
  # For step 01-02 we only populate the Ollama subtree of devon-multi-tool;
  # other tools land in 01-04+. Includes a SHARED blob (two manifests pointing
  # at the same sha256-) so the dedup-size test has signal.
  rm -rf "$root"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/llama3"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/mistral"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/codellama"
  mkdir -p "$root/.ollama/models/blobs"

  local blob_llama="8f3eaaa11111111111111111111111111111111111111111111111111111c102"
  local blob_mistral="4b9eaaa22222222222222222222222222222222222222222222222222222d203"
  # Note: codellama-q4 and codellama-instruct intentionally share the same blob.
  local blob_codellama="ababababababababababababababababababababababababababababcdcdcdcd"

  sparse_file "$root/.ollama/models/blobs/sha256-${blob_llama}" 4700000000
  sparse_file "$root/.ollama/models/blobs/sha256-${blob_mistral}" 4400000000
  sparse_file "$root/.ollama/models/blobs/sha256-${blob_codellama}" 3700000000

  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/llama3/8b-instruct-q4_K_M" "$blob_llama" 4700000000
  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/mistral/7b-instruct-q4_K_M" "$blob_mistral" 4400000000
  # Both codellama tags point at the same blob (the blob is counted once in
  # the total even though two manifest rows reference it).
  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/codellama/13b-q4_K_M" "$blob_codellama" 3700000000
  write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/codellama/13b-instruct-q4_K_M" "$blob_codellama" 3700000000
}

build_devon_permission_denied() {
  local root="$1"
  rm -rf "$root"
  mkdir -p "$root/.ollama/models/manifests"
  # Make the manifests dir unreadable. Tests must restore mode for cleanup.
  chmod 0000 "$root/.ollama/models/manifests"
}

build_devon_empty() {
  local root="$1"
  rm -rf "$root"
  # No .ollama directory at all — "tool not installed".
  mkdir -p "$root"
}

case "$NAME" in
  devon-only-ollama)        build_devon_only_ollama "$TARGET" ;;
  devon-multi-tool)         build_devon_multi_tool "$TARGET" ;;
  devon-permission-denied)  build_devon_permission_denied "$TARGET" ;;
  devon-empty)              build_devon_empty "$TARGET" ;;
  all)
    build_devon_only_ollama "tests/fixtures/.build/devon-only-ollama"
    build_devon_multi_tool "tests/fixtures/.build/devon-multi-tool"
    build_devon_empty "tests/fixtures/.build/devon-empty"
    ;;
  *)
    echo "unknown fixture: $NAME" >&2
    exit 64
    ;;
esac

echo "built: $NAME -> $TARGET"
