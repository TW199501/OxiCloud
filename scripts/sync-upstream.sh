#!/usr/bin/env bash
# Personal patch fork sync helper.
# Brings dev up to date with upstream/main, then rebuilds the docker image
# and recreates the running container.
#
# Usage:   scripts/sync-upstream.sh
# Requires: docker compose v2, git, bash, an `upstream` remote configured.

set -euo pipefail

# Sanity: must be on dev
current=$(git rev-parse --abbrev-ref HEAD)
if [[ "$current" != "dev" ]]; then
  echo "ERROR: must be on dev branch (currently on $current)" >&2
  echo "Run: git checkout dev" >&2
  exit 1
fi

# Sanity: working tree must be clean
if [[ -n "$(git status --porcelain)" ]]; then
  echo "ERROR: working tree has uncommitted changes or untracked files" >&2
  echo "Commit, stash, or clean them first. Run 'git status' to see what." >&2
  exit 1
fi

echo "==> Fetching upstream..."
git fetch upstream

behind=$(git rev-list --count dev..upstream/main)
if [[ "$behind" == "0" ]]; then
  echo "==> Already up to date with upstream/main."
else
  echo "==> $behind upstream commit(s) to merge:"
  git log --oneline dev..upstream/main

  echo "==> Merging upstream/main into dev..."
  git merge upstream/main -m "merge: sync upstream/main into dev (patch fork sync)" || {
    echo "ERROR: merge has conflicts. Resolve them, then 'git merge --continue' or 'git merge --abort' before re-running this script." >&2
    exit 1
  }

  echo "==> Pushing dev to origin..."
  git push origin dev
fi

echo "==> Rebuilding docker image..."
docker compose build oxicloud

echo "==> Recreating running container..."
docker compose up -d --force-recreate oxicloud

echo "==> Done. Stack status:"
docker compose ps
