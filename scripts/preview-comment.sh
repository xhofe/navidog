#!/usr/bin/env bash
# Create or update the pull request's single macOS preview comment.
#
# Usage: preview-comment.sh building|success|failure
#        preview-comment.sh --check
#
# building runs at the start of the workflow. success and failure replace that
# comment when the build job finishes. A newer push cancels the previous run;
# the cancelled run does not call this script, so it cannot overwrite the new
# comment.
#
# Requires GH_TOKEN, GITHUB_REPOSITORY, PR_NUMBER, RUN_URL, RUN_ID, and
# HEAD_SHA. success also requires ARTIFACT and SHA256.
set -euo pipefail

marker="<!-- navidog-preview -->"

render() {
  local status="$1"
  local head="${HEAD_SHA:?}"
  local short="${head:0:7}"
  local repo="${GITHUB_REPOSITORY:?}"
  local run_url="${RUN_URL:?}"
  local run_id="${RUN_ID:?}"
  local commit_url="${GITHUB_SERVER_URL:-https://github.com}/${repo}/commit/${head}"
  local commit="[\`$short\`](${commit_url})"
  local body
  case "$status" in
    building)
      body="$(cat <<EOF
${marker}
### macOS preview build

Building ${commit}...

- **Run:** ${run_url}
- **Status:** in progress

This comment is updated when the build finishes. A newer push replaces this run.
EOF
)"
      ;;
    success)
      local artifact="${ARTIFACT:?}"
      local sha256="${SHA256:?}"
      body="$(cat <<EOF
${marker}
### macOS preview build

- **Run:** ${run_url}
- **Commit:** ${commit}
- **Artifact:** \`${artifact}\` (expires in 7 days)
- **CLI:** \`gh run download ${run_id} -R ${repo} -n ${artifact}\`
- **SHA-256:** \`${sha256}\`

The app is ad-hoc signed and not notarized. Open the DMG, move \`Navidog.app\`
to \`~/Applications\` (create it if needed), then allow it in **System Settings
→ Privacy & Security → Open Anyway** if macOS blocks the first launch.

This build comes from pull request code; review the diff before running it.
EOF
)"
      ;;
    failure)
      body="$(cat <<EOF
${marker}
### macOS preview build

The build of ${commit} failed.

- **Run:** ${run_url}

See the run log for the error.
EOF
)"
      ;;
    *)
      echo "Usage: preview-comment.sh building|success|failure" >&2
      exit 2
      ;;
  esac
  printf '%s\n' "$body"
}

# Update the newest matching comment. A pull request should have one; if an
# older duplicate is still there, the bottom one is the one readers see.
upsert() {
  local body="$1"
  local repo="${GITHUB_REPOSITORY:?}"
  local pr="${PR_NUMBER:?}"
  local id
  id="$(
    gh api --paginate "repos/${repo}/issues/${pr}/comments" \
      --jq ".[] | select((.body // \"\") | startswith(\"${marker}\")) | .id" \
      | tail -n 1
  )"
  if [ -n "$id" ]; then
    gh api -X PATCH "repos/${repo}/issues/comments/${id}" -f body="$body" >/dev/null
  else
    gh api -X POST "repos/${repo}/issues/${pr}/comments" -f body="$body" >/dev/null
  fi
}

if [ "${1:-}" = "--check" ]; then
  export GITHUB_REPOSITORY="example/navidog"
  export GITHUB_SERVER_URL="https://github.com"
  export RUN_URL="https://github.com/example/navidog/actions/runs/1"
  export RUN_ID="1"
  export HEAD_SHA="0123456789abcdef0123456789abcdef01234567"
  export ARTIFACT="Navidog-preview-macos-aarch64"
  export SHA256="deadbeef"
  for status in building success failure; do
    body="$(render "$status")"
    first="${body%%$'\n'*}"
    if [ "$first" != "$marker" ]; then
      echo "$status: marker is not the first line" >&2
      exit 1
    fi
  done
  building="$(render building)"
  success="$(render success)"
  failure="$(render failure)"
  printf '%s' "$building" | grep -q "in progress"
  printf '%s' "$success" | grep -q "Navidog-preview-macos-aarch64"
  printf '%s' "$success" | grep -q "deadbeef"
  printf '%s' "$failure" | grep -q "failed"
  printf '%s' "$building" | grep -q "0123456"
  echo "ok"
  exit 0
fi

status="${1:?Usage: preview-comment.sh building|success|failure}"
upsert "$(render "$status")"
