#!/usr/bin/env bash
set -euo pipefail

# Make sure there are no uncommitted changes
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "Error: You have uncommitted changes."
  exit 1
fi

echo "Updating dev..."
git switch dev
git pull origin dev

echo "Updating main..."
git switch main
git pull origin main

echo "Merging dev into main..."
git merge dev

# Find latest semantic version tag, e.g. v1.2.3
latest_tag=$(git tag --list 'v[0-9]*.[0-9]*.[0-9]*' --sort=-v:refname | head -n1)

if [[ -z "$latest_tag" ]]; then
  new_tag="v0.0.1"
else
  version="${latest_tag#v}"
  IFS='.' read -r major minor patch <<<"$version"
  patch=$((patch + 1))
  new_tag="v${major}.${minor}.${patch}"
fi

new_version="${new_tag#v}"

echo "Updating sources to $new_version..."
VERSION="$new_version" perl -0pi -e \
  's/^version\s*=\s*"[^"]+"/version = "$ENV{VERSION}"/m' \
  backend/Cargo.toml
printf 'return "%s"\n' "$new_version" > plugin/vtabs/version.lua

# Workspace crates inherit workspace.package.version, so refresh their locked versions too.
cargo metadata --manifest-path backend/Cargo.toml --format-version 1 >/dev/null

git add backend/Cargo.toml backend/Cargo.lock plugin/vtabs/version.lua
git commit -m "Release $new_tag"

echo "Creating release $new_tag..."
git tag -a "$new_tag" -m "Release $new_tag"

echo "Pushing main and $new_tag..."
git push --atomic origin main "$new_tag"

echo "Switching to dev"
git switch dev

echo "Released $new_tag"
