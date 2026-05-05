#!/usr/bin/env bash
# cut-release.sh — one-shot release driver.
#
# Discovers the latest `vMAJOR.MINOR.PATCH` tag (pre-release tags ignored),
# computes the next version per flag, runs `cargo xtask release-prep` to
# bump Cargo.toml + regenerate CHANGELOG.md + run CI parity gates, commits
# the bump, creates the new tag, and (unless --no-push) pushes both. The
# tag-push triggers `.github/workflows/release.yml`.
#
# Usage:
#   ./cut-release.sh                # patch bump (default)
#   ./cut-release.sh --minor        # minor bump
#   ./cut-release.sh --major        # major bump
#   ./cut-release.sh --version 1.2.3   # explicit override
#   ./cut-release.sh --no-push      # commit + tag locally, skip push
#   ./cut-release.sh --yes          # skip the confirmation prompt
#
# Exit codes:
#   0  success
#   1  user abort
#   2  precondition failure (wrong branch, no tags, parse error, ...)

set -euo pipefail

# ---- args ------------------------------------------------------------------

bump="patch"        # one of: major | minor | patch | explicit
explicit_version=""
do_push=1
assume_yes=0

while (( $# )); do
  case $1 in
    --major)   bump="major"   ;;
    --minor)   bump="minor"   ;;
    --patch)   bump="patch"   ;;
    --version) bump="explicit"; explicit_version=${2:?--version requires X.Y.Z}; shift ;;
    --no-push) do_push=0    ;;
    --yes|-y)  assume_yes=1 ;;
    -h|--help)
      cat <<'EOF'
cut-release.sh — one-shot release driver.

Discovers the latest vMAJOR.MINOR.PATCH tag (pre-release tags ignored),
computes the next version per flag, runs `cargo xtask release-prep` to
bump Cargo.toml + regenerate CHANGELOG.md + run CI parity gates, commits
the bump, creates the new tag, and (unless --no-push) pushes both. The
tag-push triggers .github/workflows/release.yml.

Usage:
  ./cut-release.sh                   patch bump (default)
  ./cut-release.sh --minor           minor bump
  ./cut-release.sh --major           major bump
  ./cut-release.sh --version 1.2.3   explicit override
  ./cut-release.sh --no-push         commit + tag locally, skip push
  ./cut-release.sh --yes             skip the confirmation prompt

Exit codes:
  0  success
  1  user abort
  2  precondition failure (wrong branch, no tags, parse error, ...)
EOF
      exit 0
      ;;
    *)
      echo "unknown flag: $1" >&2
      exit 2
      ;;
  esac
  shift
done

# ---- preconditions ---------------------------------------------------------

repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"

current_branch=$(git rev-parse --abbrev-ref HEAD)
if [[ $current_branch != main ]]; then
  echo "refusing to cut release from branch '$current_branch'; switch to 'main' first" >&2
  exit 2
fi

# Best-effort tag sync (don't fail if offline — local tags are fine).
git fetch --tags --quiet origin 2>/dev/null || true

# ---- find the latest released tag (vMAJOR.MINOR.PATCH, no pre-release) -----

# `sort -V` gives us semver-aware ordering. We grep with a strict regex so a
# `v0.2.0-rc1` or `v0.2.0+build.7` doesn't shadow the latest stable.
latest_tag=$(git tag -l 'v*' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -1 || true)

if [[ -z $latest_tag ]]; then
  current=0.0.0
  echo "no prior released tag found — treating current version as 0.0.0"
else
  current=${latest_tag#v}
  echo "latest released tag: $latest_tag (version $current)"
fi

IFS=. read -r maj min pat <<<"$current"

# ---- compute next version --------------------------------------------------

case $bump in
  major)    next="$((maj + 1)).0.0" ;;
  minor)    next="$maj.$((min + 1)).0" ;;
  patch)    next="$maj.$min.$((pat + 1))" ;;
  explicit) next=$explicit_version ;;
esac

# Sanity: explicit must be parseable.
if ! [[ $next =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "computed/explicit version '$next' is not valid semver" >&2
  exit 2
fi

new_tag="v$next"

# Refuse if the tag already exists locally OR on the remote — we don't want
# to clobber a real release. The user should bump higher.
if git rev-parse --verify "refs/tags/$new_tag" >/dev/null 2>&1; then
  echo "tag $new_tag already exists locally — bump differently or delete it first" >&2
  exit 2
fi
if git ls-remote --tags origin "refs/tags/$new_tag" 2>/dev/null | grep -q .; then
  echo "tag $new_tag already exists on origin — bump differently" >&2
  exit 2
fi

echo "  next tag: $new_tag (bump=$bump)"

# ---- confirmation ----------------------------------------------------------

if (( ! assume_yes )); then
  printf 'cut release %s on main? [y/N] ' "$new_tag"
  read -r reply
  [[ $reply =~ ^[Yy]$ ]] || { echo "aborted"; exit 1; }
fi

# ---- prepare ---------------------------------------------------------------
# release-prep does the heavy lifting: Cargo.toml bump, CHANGELOG regen,
# fmt + clippy + test gates. PATH=/usr/bin avoids the pyenv `cc` shim on
# macOS dev machines (CI runners are unaffected).

PATH=/usr/bin:$PATH cargo xtask release-prep --version "$next"

git commit -am "chore(release): $new_tag"

git tag -a "$new_tag" -m "$new_tag"

# ---- push ------------------------------------------------------------------

if (( do_push )); then
  git push origin main
  git push origin "$new_tag"
  echo
  echo "release pipeline triggered. follow with: gh run watch"
else
  echo
  echo "skipped push (--no-push). when ready:"
  echo "  git push origin main"
  echo "  git push origin $new_tag"
fi
