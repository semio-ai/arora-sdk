#!/usr/bin/env bash
# Build revisions in isolated worktrees, then measure one process per build/round.
# Usage: run.sh OUT LABEL=REV LABEL=REV ... [-- workload.json ...]
set -euo pipefail
if (( $# < 3 )); then echo 'usage: run.sh OUT LABEL=REV LABEL=REV ... [-- workload.json ...]' >&2; exit 2; fi
out=$1; shift
mkdir -p "$out"
out=$(cd "$out" && pwd)
[[ ! -e "$out/manifest.json" ]] || { echo 'output already contains a run' >&2; exit 2; }
root=$(git rev-parse --show-toplevel)
labels=(); revisions=(); workloads=()
while (( $# )); do
  if [[ $1 == -- ]]; then shift; break; fi
  [[ $1 == *=* ]] || { echo "expected LABEL=REV: $1" >&2; exit 2; }
  label=${1%%=*}; revision=${1#*=}
  [[ $label =~ ^[a-zA-Z0-9_][a-zA-Z0-9_-]*$ && -n $revision ]] || { echo 'invalid build label or revision' >&2; exit 2; }
  for previous in ${labels[@]+"${labels[@]}"}; do [[ $previous != "$label" ]] || { echo "duplicate build: $label" >&2; exit 2; }; done
  labels+=("$label"); revisions+=("$(git -C "$root" rev-parse --verify "$revision^{commit}")")
  shift
done
(( ${#labels[@]} >= 2 )) || { echo 'at least two revisions required' >&2; exit 2; }
for path in "$@"; do
  workloads+=("$(cd "$(dirname "$path")" && pwd)/$(basename "$path")")
done
rounds=${ROUNDS:-8}
[[ $rounds =~ ^[1-9][0-9]*$ ]] || { echo 'ROUNDS must be positive' >&2; exit 2; }
(( rounds >= 4 && rounds % 2 == 0 )) || { echo 'ROUNDS must be even and at least 4' >&2; exit 2; }
created=()
cleanup() {
  local status=$?
  trap - EXIT
  for path in ${created[@]+"${created[@]}"}; do git -C "$root" worktree remove --force "$path" || true; done
  exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
# Whitespace-separated cargo flags, e.g. BENCH_CARGO_FLAGS=--offline.
extra_cargo_flags=()
read -r -a extra_cargo_flags <<< "${BENCH_CARGO_FLAGS:-}"
mkdir -p "$out/worktrees" "$out/runs" "$out/targets"
for (( i=0; i<${#labels[@]}; i++ )); do
  label=${labels[i]}
  git -C "$root" worktree add --detach "$out/worktrees/$label" "${revisions[i]}"
  created+=("$out/worktrees/$label")
  mkdir -p "$out/runs/$label"
  (cd "$out/worktrees/$label" && CARGO_TARGET_DIR="$out/targets/$label" cargo build --locked ${extra_cargo_flags[@]+"${extra_cargo_flags[@]}"} --release -p arora-behavior-tree-bench --bin bench-kit)
done
python3 - "$out/manifest.json" "$rounds" "${BENCH_FORMS:-}" "${labels[@]}" -- "${revisions[@]}" <<'PY'
import json, sys
path, rounds, forms, *args = sys.argv[1:]
at = args.index('--')
labels, revisions = args[:at], args[at + 1:]
with open(path, 'w') as out:
    json.dump({'schema': 1, 'rounds': int(rounds), 'forms': forms.split(',') if forms else None,
               'builds': dict(zip(labels, revisions))}, out, indent=2)
PY
for (( round=1; round<=rounds; round++ )); do
  for (( position=0; position<${#labels[@]}; position++ )); do
    if (( round % 2 )); then i=$position; else i=$((${#labels[@]} - 1 - position)); fi
    label=${labels[i]}
    "$out/targets/$label/release/bench-kit" "$out/runs/$label/$round.json" ${workloads[@]+"${workloads[@]}"}
  done
  echo "round $round/$rounds complete" >&2
done
python3 "$root/crates/arora-behavior-tree-bench/scripts/analyze.py" "$out" --baseline "${labels[0]}"
