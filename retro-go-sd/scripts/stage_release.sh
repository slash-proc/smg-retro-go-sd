#!/bin/sh
# Stage a locally built bundle without putting the user's ISO or assets in CI.
set -eu
cd "$(dirname "$0")/../.."
tag=${1:?usage: sh retro-go-sd/scripts/stage_release.sh TAG RELEASE_DIRECTORY}
files=${2:?usage: sh retro-go-sd/scripts/stage_release.sh TAG RELEASE_DIRECTORY}
files=$(cd "$files" && pwd)
case "$tag" in v[0-9]*.[0-9]*.[0-9]*) ;; *) echo 'Expected a version tag' >&2; exit 1 ;; esac
commit=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["source"]["commit"])' "$files/manifest.json")
if git rev-parse --verify -q "refs/tags/$tag" >/dev/null; then
    test "$(git rev-parse "refs/tags/$tag^{commit}")" = "$commit" || { echo 'Bundle and tag commits differ' >&2; exit 1; }
else
    test "$(git rev-parse HEAD)" = "$commit" || { echo 'Bundle does not match HEAD' >&2; exit 1; }
fi
remote=$(git ls-remote origin "refs/tags/$tag" | cut -f1)
if [ -n "$remote" ]; then
    test "$remote" = "$(git rev-parse "refs/tags/$tag")" || { echo 'Local and remote tags differ' >&2; exit 1; }
fi
test -f "$files/smg_iso_verifier.wasm" || { echo 'Missing ISO verifier' >&2; exit 1; }
stage=$(mktemp -d "${TMPDIR:-/tmp}/smg-release-input-XXXXXXXX")
created=0
cleanup() {
    git worktree remove --force "$stage/worktree" >/dev/null 2>&1 || true
    if [ "$created" = 1 ]; then git branch -D "release-input/$tag" >/dev/null 2>&1 || true; fi
    rm -rf "$stage"
}
trap cleanup EXIT HUP INT TERM
python3 retro-go-sd/scripts/validate_release.py --bundle "$files/smg-$tag-bundle.zip" --tag "$tag" --commit "$commit" --out "$stage/validated"
node tools/wasm_converter/verify.mjs "$stage/validated/smg_asset_converter.wasm" '{"range-input":1}'
node tools/wasm_converter/test-verifier.mjs "$files/smg_iso_verifier.wasm"
git worktree add --detach "$stage/worktree" HEAD
git -C "$stage/worktree" switch -c "release-input/$tag"
created=1
mkdir -p "$stage/worktree/release-input"
cp "$files/smg-$tag-bundle.zip" "$stage/worktree/release-input/bundle.zip"
cp "$files/smg_iso_verifier.wasm" "$stage/worktree/release-input/"
git -C "$stage/worktree" add release-input
git -C "$stage/worktree" commit -m "Stage $tag release artifacts"
git -C "$stage/worktree" push origin "release-input/$tag"
echo "Staged $tag; GitHub Actions will validate and publish it."
