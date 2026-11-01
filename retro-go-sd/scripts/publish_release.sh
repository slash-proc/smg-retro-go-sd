#!/bin/sh
# Publish only a clean, already-pushed tag whose main-branch CI passed.
set -eu
cd "$(dirname "$0")/../.."
tag=${1:?usage: sh retro-go-sd/scripts/publish_release.sh vMAJOR.MINOR.PATCH}
test -z "$(git status --porcelain)" || { echo 'working tree must be clean' >&2; exit 1; }
commit=$(git rev-parse HEAD)
test "$(git rev-parse "refs/tags/$tag^{commit}")" = "$commit" || { echo 'tag does not point to HEAD' >&2; exit 1; }
test "$(git ls-remote origin refs/heads/main | cut -f1)" = "$commit" || { echo 'remote main does not point to HEAD' >&2; exit 1; }
test "$(git ls-remote origin "refs/tags/$tag" | cut -f1)" = "$(git rev-parse "refs/tags/$tag")" || { echo 'tag is not pushed' >&2; exit 1; }
# The release workflow independently verifies the latest CI result and the tag.

# These builds use the user's local ISO; neither the ISO nor generated asset
# sidecar is uploaded. Build again after checkout so all outputs match the tag.
make build/assets/assets_gen.h
tools/wasm_converter/build.sh
make -C retro-go-sd docker CORE_VERSION="$tag"
test -z "$(git status --porcelain)" || { echo 'build changed tracked source; inspect it before release' >&2; exit 1; }
node tools/wasm_converter/verify.mjs "$PWD/tools/wasm_converter/web/smg_asset_converter.wasm" '{"range-input":1}'
node tools/wasm_converter/test-verifier.mjs "$PWD/tools/wasm_converter/web/smg_iso_verifier.wasm"
SMG_TEST_GREP='convert the verified USA ISO' tools/wasm_converter/browser-test/run.sh
mkdir -p tmp
stage=$(mktemp -d "$PWD/tmp/smg-release-XXXXXXXX")
python3 retro-go-sd/scripts/package_local_bundle.py --tag "$tag" --dist-dir "$stage/files" --bundle "$stage/files/smg-$tag-bundle.zip"
curl -fsSL https://slash-proc.github.io/gwrg-dist-spec/schema/manifest.schema.json -o "$stage/public-manifest.schema.json"
node tools/validate_manifest.mjs "$stage/files/manifest.json" "$stage/public-manifest.schema.json"
echo "Prepared release files in $stage/files"
sh retro-go-sd/scripts/stage_release.sh "$tag" "$stage/files"
