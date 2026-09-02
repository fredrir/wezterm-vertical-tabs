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

echo "Pushing main..."
git push origin main

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

echo "Creating release $new_tag..."
git tag -a "$new_tag" -m "Release $new_tag"

echo "Pushing $new_tag..."
git push origin "$new_tag"

echo "Switching to dev"
git switch dev

echo "Released $new_tag"
