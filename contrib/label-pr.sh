#!/usr/bin/env bash
# Labels a PR based on which crates it touches.

set -euo pipefail

# FORGEJO_REF is the base branch on pull_request_target, so read the PR number
# from the event payload instead.
pr=$(grep -o '"number":[[:space:]]*[0-9]\+' "${FORGEJO_EVENT_PATH:-/dev/null}" | head -1 | grep -o '[0-9]\+' || true)
if ! [[ "$pr" =~ ^[0-9]+$ ]]; then
    echo "Could not determine PR number from event payload" >&2
    exit 1
fi

api="${FORGEJO_API_URL:-${FORGEJO_SERVER_URL}/api/v1}/repos/${FORGEJO_REPOSITORY}"
auth="Authorization: token ${FORGEJO_TOKEN}"

echo "Labeling PR #${pr} in ${FORGEJO_REPOSITORY}"

# Map a changed-file path prefix to the label for that crate.
declare -A label_map=(
    [bitcoind/]="C-bitcoind"
    [bitreq/]="C-bitreq"
    [client/]="C-client"
    [electrsd/]="C-electrsd"
    [integration_test/]="C-integration-test"
    [jsonrpc/]="C-jsonrpc"
    [types/]="C-types"
    [verify/]="C-verify"
)

# The labels this script manages, so it never touches unrelated ones.
declare -A managed
for prefix in "${!label_map[@]}"; do
    managed["${label_map[$prefix]}"]=1
done

# Split a compact JSON array into one object per line for line-based parsing.
split_objects() { sed 's/},[[:space:]]*{/}\n{/g'; }

# Labels that should be on the PR based on its changed files.
declare -A want
while IFS= read -r filename; do
    for prefix in "${!label_map[@]}"; do
        if [[ "$filename" == "$prefix"* ]]; then
            want["${label_map[$prefix]}"]=1
            break
        fi
    done
done < <(curl -sf -H "$auth" "${api}/pulls/${pr}/files" | grep -o '"filename":"[^"]*"' | cut -d'"' -f4)

# Map every repository label name to its id.
declare -A label_id
while IFS= read -r obj; do
    name=$(grep -o '"name":"[^"]*"' <<< "$obj" | head -1 | cut -d'"' -f4)
    id=$(grep -o '"id":[0-9]\+' <<< "$obj" | head -1 | grep -o '[0-9]\+')
    [[ -n "$name" && -n "$id" ]] && label_id["$name"]="$id"
done < <(curl -sf -H "$auth" "${api}/labels" | split_objects)

# Labels currently on the PR.
declare -A have
while IFS= read -r name; do
    have["$name"]=1
done < <(curl -sf -H "$auth" "${api}/issues/${pr}/labels" | grep -o '"name":"[^"]*"' | cut -d'"' -f4)

# Add wanted labels that are missing.
for label in "${!want[@]}"; do
    if [[ -z "${have[$label]+x}" && -n "${label_id[$label]+x}" ]]; then
        curl -sf -X POST -H "$auth" -H "Content-Type: application/json" \
            -d "{\"labels\":[${label_id[$label]}]}" "${api}/issues/${pr}/labels" > /dev/null
        echo "added ${label}"
    fi
done

# Remove managed labels that no longer apply.
for label in "${!have[@]}"; do
    if [[ -n "${managed[$label]+x}" && -z "${want[$label]+x}" && -n "${label_id[$label]+x}" ]]; then
        curl -sf -X DELETE -H "$auth" "${api}/issues/${pr}/labels/${label_id[$label]}" > /dev/null
        echo "removed ${label}"
    fi
done
