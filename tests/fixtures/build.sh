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

# -----------------------------------------------------------------------------
# GGUF helpers (US-07).
#
# A valid GGUF header (https://github.com/ggerganov/ggml/blob/master/docs/gguf.md):
#
#   magic         : 4 bytes  ASCII "GGUF" (0x47 0x47 0x55 0x46)
#   version       : u32 LE   (we emit 3)
#   tensor_count  : u64 LE   (we emit 0 — fixtures have no tensor metadata)
#   kv_count      : u64 LE   (we emit 2 — architecture + file_type)
#
#   then 2 KV entries:
#     "general.architecture" -> string value
#     "general.file_type"    -> uint32 value (Ollama-style quant id)
#
#   String value layout (gguf_value_type::STRING = 8):
#     value_type (u32 LE = 8)
#     length     (u64 LE)
#     bytes
#
#   Numeric value layout (gguf_value_type::UINT32 = 4):
#     value_type (u32 LE = 4)
#     value      (u32 LE)
#
#   Key layout: u64 LE len + bytes. (Same as a string value, minus the type tag.)
#
# We synthesize headers via printf + Python so build.sh stays portable across
# macOS BSD tooling and Linux GNU tooling.
write_gguf() {
  # Args: <gguf-path> <architecture> <file-type-uint32>
  local path="$1" arch="$2" file_type="$3"
  mkdir -p "$(dirname "$path")"
  GGUF_OUT="$path" GGUF_ARCH="$arch" GGUF_FT="$file_type" python3 - <<'PY'
import os, struct
path = os.environ["GGUF_OUT"]
arch = os.environ["GGUF_ARCH"].encode("utf-8")
ft = int(os.environ["GGUF_FT"])

# Header.
out = bytearray()
out += b"GGUF"                       # magic
out += struct.pack("<I", 3)          # version
out += struct.pack("<Q", 0)          # tensor_count
out += struct.pack("<Q", 2)          # kv_count

# KV[0]: "general.architecture" -> string(arch)
key = b"general.architecture"
out += struct.pack("<Q", len(key)) + key
out += struct.pack("<I", 8)          # GGUF_TYPE_STRING
out += struct.pack("<Q", len(arch)) + arch

# KV[1]: "general.file_type" -> uint32(ft)
key = b"general.file_type"
out += struct.pack("<Q", len(key)) + key
out += struct.pack("<I", 4)          # GGUF_TYPE_UINT32
out += struct.pack("<I", ft)

with open(path, "wb") as f:
    f.write(out)
PY
}

write_corrupt_gguf() {
  # Args: <path>
  # 4 bytes that are NOT the GGUF magic, then garbage.
  local path="$1"
  mkdir -p "$(dirname "$path")"
  printf 'XXXX\x00\x00\x00\x01' > "$path"
}

build_devon_llama_cli() {
  # Standalone llama-cli fixture for US-07 acceptance + plugin contract tests.
  # Lays out:
  #   <root>/llms/llama-3-8b-q4_K_M.gguf   (valid, ft=15 → "Q4_K_M")
  #   <root>/llms/mistral-7b-q4.gguf       (valid, ft=2  → "Q4_0")
  #   <root>/llms/corrupt.gguf             (truncated; bad magic)
  #   <root>/models/qwen-1_5b.gguf         (valid; in the alternate root)
  local root="$1"
  rm -rf "$root"
  mkdir -p "$root/llms"
  mkdir -p "$root/models"
  write_gguf "$root/llms/llama-3-8b-q4_K_M.gguf" "llama" 15
  write_gguf "$root/llms/mistral-7b-q4.gguf"     "mistral" 2
  write_corrupt_gguf "$root/llms/corrupt.gguf"
  write_gguf "$root/models/qwen-1_5b.gguf"       "qwen2"  15
}

build_devon_llama_cli_extra() {
  # For "Configured additional search path is honored" — a single extra root
  # outside the defaults. The default roots (<root>/llms, <root>/models) are
  # ABSENT; only the configured path holds models.
  local root="$1"
  rm -rf "$root"
  mkdir -p "$root/data/models"
  write_gguf "$root/data/models/extra.gguf" "llama" 2
}

build_devon_hf_cache() {
  # Standalone Hugging Face cache fixture for US-12 acceptance + plugin
  # contract tests. Mirrors the HF hub layout:
  #
  #   <root>/hub/
  #     models--meta-llama--Llama-3-8B/
  #       blobs/<sha256>                            ← real sparse files
  #       snapshots/<rev-sha>/
  #         model.safetensors  → ../../blobs/<sha-A>   (relative symlink)
  #         config.json        → ../../blobs/<sha-B>
  #       refs/main                                 ← text holding <rev-sha>
  #     models--mistralai--Mistral-7B-v0.3/
  #       blobs/<sha256>
  #       snapshots/<rev-sha>/model.gguf → ../../blobs/<sha-C>
  #     models--corrupt-org--corrupt-repo/
  #       blobs/  (empty — broken symlink targets a missing blob)
  #       snapshots/<rev-sha>/model.bin → ../../blobs/<sha-MISSING>
  #
  # AC-4 (format inference) is exercised via the .safetensors + .gguf + .bin
  # mix. AC-5 (broken symlink) is exercised by the corrupt-org entry.
  local root="$1"
  rm -rf "$root"
  local hub="$root/hub"

  # Model 1: meta-llama/Llama-3-8B with a .safetensors snapshot (healthy).
  local m1="$hub/models--meta-llama--Llama-3-8B"
  local rev1="abc123def4567890abc123def4567890abc12345"
  local blob1a="aaaa1111111111111111111111111111111111111111111111111111111111aa"
  local blob1b="bbbb2222222222222222222222222222222222222222222222222222222222bb"
  mkdir -p "$m1/blobs"
  mkdir -p "$m1/snapshots/$rev1"
  mkdir -p "$m1/refs"
  sparse_file "$m1/blobs/$blob1a" 16000000000   # 16 GB sparse "model" blob
  sparse_file "$m1/blobs/$blob1b" 4096          # 4 KB config blob
  ( cd "$m1/snapshots/$rev1" && ln -sf "../../blobs/$blob1a" "model.safetensors" )
  ( cd "$m1/snapshots/$rev1" && ln -sf "../../blobs/$blob1b" "config.json" )
  printf '%s' "$rev1" > "$m1/refs/main"

  # Model 2: mistralai/Mistral-7B-v0.3 with a .gguf snapshot (healthy).
  local m2="$hub/models--mistralai--Mistral-7B-v0.3"
  local rev2="987654321098765432109876543210987654abcd"
  local blob2="cccc3333333333333333333333333333333333333333333333333333333333cc"
  mkdir -p "$m2/blobs"
  mkdir -p "$m2/snapshots/$rev2"
  mkdir -p "$m2/refs"
  sparse_file "$m2/blobs/$blob2" 4400000000     # 4.4 GB
  ( cd "$m2/snapshots/$rev2" && ln -sf "../../blobs/$blob2" "model.gguf" )
  printf '%s' "$rev2" > "$m2/refs/main"

  # Model 3: corrupt-org/corrupt-repo with a BROKEN snapshot symlink.
  local m3="$hub/models--corrupt-org--corrupt-repo"
  local rev3="deadbeefdeadbeefdeadbeefdeadbeefdeadbeef"
  local blob3_missing="9999999999999999999999999999999999999999999999999999999999999999"
  mkdir -p "$m3/blobs"
  mkdir -p "$m3/snapshots/$rev3"
  mkdir -p "$m3/refs"
  # Snapshot points at a blob that does NOT exist.
  ( cd "$m3/snapshots/$rev3" && ln -sf "../../blobs/$blob3_missing" "model.bin" )
  printf '%s' "$rev3" > "$m3/refs/main"
}

build_devon_long_list() {
  # 31 distinct Ollama manifest entries — enough to exercise right-pane
  # scroll position indicator (US-03 "Down Arrow scrolls a long model list").
  # Each manifest points at its own blob; sizes are tiny stand-ins (1 KB
  # each) since this fixture is only about row count, not bytes.
  local root="$1"
  rm -rf "$root"
  mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library"
  mkdir -p "$root/.ollama/models/blobs"

  local i
  for i in $(seq -f "%02g" 1 31); do
    local repo="model${i}"
    local tag="v1"
    # Distinct 64-hex blob hashes (pad i to 60 chars after 4-hex prefix).
    local blob="aaaa$(printf '%060d' "${i}")"
    mkdir -p "$root/.ollama/models/manifests/registry.ollama.ai/library/${repo}"
    sparse_file "$root/.ollama/models/blobs/sha256-${blob}" 1024
    write_manifest "$root/.ollama/models/manifests/registry.ollama.ai/library/${repo}/${tag}" "$blob" 1024
  done
}

case "$NAME" in
  devon-only-ollama)        build_devon_only_ollama "$TARGET" ;;
  devon-multi-tool)         build_devon_multi_tool "$TARGET" ;;
  devon-permission-denied)  build_devon_permission_denied "$TARGET" ;;
  devon-empty)              build_devon_empty "$TARGET" ;;
  devon-long-list)          build_devon_long_list "$TARGET" ;;
  devon-llama-cli)          build_devon_llama_cli "$TARGET" ;;
  devon-llama-cli-extra)    build_devon_llama_cli_extra "$TARGET" ;;
  devon-hf-cache)           build_devon_hf_cache "$TARGET" ;;
  all)
    build_devon_only_ollama "tests/fixtures/.build/devon-only-ollama"
    build_devon_multi_tool "tests/fixtures/.build/devon-multi-tool"
    build_devon_empty "tests/fixtures/.build/devon-empty"
    build_devon_long_list "tests/fixtures/.build/devon-long-list"
    build_devon_llama_cli "tests/fixtures/.build/devon-llama-cli"
    build_devon_llama_cli_extra "tests/fixtures/.build/devon-llama-cli-extra"
    build_devon_hf_cache "tests/fixtures/.build/devon-hf-cache"
    ;;
  *)
    echo "unknown fixture: $NAME" >&2
    exit 64
    ;;
esac

echo "built: $NAME -> $TARGET"
