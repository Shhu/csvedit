#!/bin/sh
# Publishes a version of CSV Edit from this computer (no CI): builds the
# Windows .exe and the Flatpak, tags the sources, creates the GitHub release
# with them, where the website's download buttons point, and updates the
# shu-app.com Flatpak repository. The website itself (docs/) is served by
# GitHub Pages from main, so pushing main publishes it.
# Usage: packaging/publish.sh notes.md   (version taken from Cargo.toml)
# Can be run again after a failure: what is already done is kept.
set -e
cd "$(dirname "$0")/.."
notes=$1
[ -f "$notes" ] || { echo "Usage: $0 notes.md"; exit 1; }
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="v$version"
if gh release view "$tag" --repo Shhu/csvedit > /dev/null 2>&1; then
    echo "$tag is already published: bump the version in Cargo.toml"
    exit 1
fi
[ -z "$(git status --porcelain)" ] || { echo "Commit your changes first"; exit 1; }
[ "$(git rev-parse --abbrev-ref HEAD)" = main ] || { echo "Publish from main"; exit 1; }
git fetch -q origin main
[ "$(git rev-list --count HEAD..origin/main)" = 0 ] ||
    { echo "origin/main has commits this branch lacks: pull first"; exit 1; }
# The software centers show the version and notes of the metainfo.
grep -q "<release version=\"$version\"" packaging/flatpak/io.github.Shhu.CsvEdit.metainfo.xml ||
    { echo "Add <release version=\"$version\"> to the metainfo first"; exit 1; }
[ THIRD-PARTY-LICENSES.txt -nt Cargo.lock ] ||
    { echo "Cargo.lock changed: run packaging/licenses.py and commit"; exit 1; }
# A tag left by a failed run must be this commit's.
if git rev-parse -q --verify "refs/tags/$tag" > /dev/null; then
    [ "$(git rev-parse "$tag^{commit}")" = "$(git rev-parse HEAD)" ] ||
        { echo "Tag $tag exists on another commit"; exit 1; }
fi

packaging/windows/build.sh
packaging/flatpak/build.sh
out=$(mktemp -d)
trap 'rm -rf "$out"' EXIT
. packaging/windows/paths.sh
cp "$WIN_TARGET_WSL/release/csvedit.exe" "$out/"
cp packaging/flatpak/csvedit.flatpak "$out/"
cp THIRD-PARTY-LICENSES.txt "$out/"

# Main first: the tag only goes out once its commit is there.
git push origin main
git rev-parse -q --verify "refs/tags/$tag" > /dev/null || git tag "$tag"
git push origin "$tag"
gh release create "$tag" "$out/csvedit.exe" "$out/csvedit.flatpak" \
    "$out/THIRD-PARTY-LICENSES.txt" --verify-tag \
    --repo Shhu/csvedit --title "CSV Edit $version" --notes-file "$notes"
echo "Published: https://github.com/Shhu/csvedit/releases/tag/$tag"
# The shu-app.com Flatpak repository (shared with the author's other apps)
# takes the new bundle; the release stands even if this fails.
shared="$HOME/dev/shu-app/flatpak/publish.sh"
if [ -x "$shared" ]; then
    "$shared" || echo "Flatpak repository not updated: run $shared again" >&2
fi
