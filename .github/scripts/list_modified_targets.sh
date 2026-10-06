#!/usr/bin/env bash
# Which workspace members a change can affect, so CI tests only those.
#
#   list_modified_targets.sh <base-ref>   key=value lines for $GITHUB_OUTPUT (report on stderr):
#                            any, test-args, and test-<member>=true / build-<member>=true
#                            for each selected member (absent, so false, otherwise)
#   list_modified_targets.sh --all        everything (main)
#   list_modified_targets.sh verify       after a build: check the rules below against the
#                            inputs cargo recorded (rustc dep-info, build-script
#                            rerun-if-changed); exit 1 on a gap
#
# A changed file selects the member whose directory holds it and the packages
# INPUTS lists it for; if neither, nothing when IGNORE matches it, else (a file
# in no package: Cargo.toml, .cargo/, rust-toolchain.toml, CI…) everything.
# Cargo.lock selects the packages whose entry changed. Selection then spreads
# through `cargo metadata`'s resolve graph: normal, build and artifact edges
# transitively, dev edges one hop. (`cargo tree --invert` would do the same
# walk, but it panics on this workspace's artifact dependencies.)
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

# Changes here select nothing. `verify` fails if a build reads one of them.
IGNORE=(
  '*.md' LICENSE 'docs/*' 'examples/*' '.vscode/*' .gitignore
  .github/workflows/release.yml .github/workflows/markdown-link-check-config.json
)
# Files outside a member that it reads: "<glob> <package>".
INPUTS=(
  'libs/cpp/* test-cpp'      # cmake builds of the C++ modules (build.rs)
  'libs/cpp/* test-cpp-2'
  'libs/cpp/* arora-nao'
  'crates/arora-behavior-tree-types-yaml/records/* arora-nao'
  'crates/arora-engine/wit/* test-rust-component'   # wit_bindgen::generate!
)

meta=$(cargo metadata --format-version 1 --all-features)
root=$(jq -r .workspace_root <<<"$meta")
# "dir name" per member, innermost directory first.
members=$(jq -r --arg r "$root/" '.workspace_members as $m | .packages[]
    | select(.id as $i | $m | index($i))
    | "\(.manifest_path | ltrimstr($r) | rtrimstr("/Cargo.toml")) \(.name)"' <<<"$meta" |
  awk '{ print length($1), $0 }' | sort -rn | cut -d' ' -f2-)

owner() { local dir name; while read -r dir name; do [[ $1 == "$dir"/* ]] && { echo "$name"; break; }; done <<<"$members"; return 0; }
ignored() { local g; for g in "${IGNORE[@]}"; do [[ $1 == $g ]] && return 0; done; return 1; }
readers() { local r; for r in "${INPUTS[@]}"; do [[ $1 == ${r% *} ]] && echo "${r#* }"; done; return 0; }

# Seeds (package ids, one per line) → {build, test}: member names a change to
# the seeds can reach.
reach() {
  jq -c --arg seeds "$(cat)" '
    ($seeds | split("\n") | map(select(. != "")) | map({(.): true}) | add // {}) as $s
    | [.resolve.nodes[] | .id as $from | .deps[]
        | {to: .pkg, from: $from, dev: ([.dep_kinds[].kind] | all(. == "dev"))}] as $e
    | def close: . as $set | ([$e[] | select((.dev | not) and $set[.to]) | {(.from): true}] | add // {}) as $n
        | ($set + $n) as $u | if ($u | length) == ($set | length) then $set else ($u | close) end;
    ($s | close) as $build
    | ($build + ([$e[] | select(.dev and $build[.to]) | {(.from): true}] | add // {})) as $test
    | .workspace_members as $m
    | def names($set): [.packages[] | select($set[.id] and (.id as $i | $m | index($i))) | .name] | sort;
    {build: names($build), test: names($test)}' <<<"$meta"
}
id_of() { jq -r --arg n "$1" '.packages[] | select(.name == $n and .source == null) | .id' <<<"$meta"; }

# One line per [[package]] block of a Cargo.lock.
lock_entries() { awk -v RS= '/^\[\[package\]\]/ { gsub(/\n/, " "); print }' | sort; }

if [[ ${1-} == verify ]]; then
  # (package, input) pairs: rustc's own dep-info files open with a rule for the
  # `.d` itself, crate root first (cargo's merged copies next to uplifted
  # artifacts don't, and are skipped); build scripts' rerun-if-changed paths
  # are relative to their package.
  pairs=$(
    find target -name incremental -prune -o -path '*/build/*/out' -prune -o -name '*.d' -print |
    while read -r d; do
      line=$(head -n1 "$d"); [[ ${line%%: *} == *.d ]] || continue
      read -ra deps <<<"${line#*: }"; ((${#deps[@]})) || continue
      pkg=$(owner "${deps[0]#"$root"/}") && [[ -n $pkg ]] || continue
      printf "$pkg %s\n" "${deps[@]}"
    done
    find target -path '*/build/*' -name output -print | while read -r out; do
      name=$(basename "$(dirname "$out")"); name=${name%-*}
      dir=$(awk -v n="$name" '$2 == n { print $1 }' <<<"$members"); [[ -n $dir ]] || continue
      sed -n 's/^cargo::\{0,1\}rerun-if-changed=//p' "$out" | while read -r f; do
        [[ $f == /* ]] && echo "$name $f" || echo "$name $root/$dir/$f"
      done
    done)
  declare -A reached=()
  gaps=0 checked=0
  while read -r pkg f; do
    f=$(realpath -m "$( [[ $f == /* ]] && echo "$f" || echo "$root/$f")"); f=${f#"$root"/}
    [[ $f == /* || $f == target/* ]] && continue   # toolchain, registry, generated
    checked=$((checked + 1))
    if readers "$f" | grep -qx "$pkg"; then continue; fi
    if ignored "$f"; then echo "$pkg reads $f, which IGNORE matches"; gaps=$((gaps + 1)); continue; fi
    # Global, or its own; else a change to it must reach the reader's build.
    o=$(owner "$f"); [[ -z $o || $o == "$pkg" ]] && continue
    [[ -v reached[$o] ]] || reached[$o]=$(id_of "$o" | reach | jq -r '.build[]')
    grep -qx "$pkg" <<<"${reached[$o]}" && continue
    echo "$pkg reads $f, owned by $o, which $pkg does not depend on: add '${f%/*}/* $pkg' to INPUTS"
    gaps=$((gaps + 1))
  done < <(sort -u <<<"$pairs")
  echo "verify: $checked input(s) checked, $gaps gap(s)" >&2
  exit $((gaps > 0))
fi

all() { echo "everything: $1" >&2; jq -n '{build: ["*"], test: ["*"]}'; }
if [[ ${1-} == --all ]]; then
  result=$(all "--all")
elif [[ -n ${1-} ]]; then
  base=$(git merge-base "$1" HEAD)
  result=""
  seeds=""
  while read -r f; do
    [[ -z $f ]] && continue
    if [[ $f == Cargo.lock ]]; then
      changed=$(comm -13 <(git show "$base:Cargo.lock" | lock_entries) <(lock_entries <Cargo.lock))
      seeds+=$(jq -r --arg c "$changed" '($c | split("\n")) as $lines | .packages[]
          | select(("name = \"\(.name)\" version = \"\(.version)\"") as $k | any($lines[]; startswith("[[package]] \($k)")))
          | .id' <<<"$meta")$'\n'
      continue
    fi
    r=$(readers "$f")
    if [[ -z $r ]]; then
      ignored "$f" && continue
      [[ -n $(owner "$f") ]] || { result=$(all "$f is in no package"); break; }
    fi
    r+=" $(owner "$f")"
    for p in $r; do echo "$f → $p" >&2; seeds+=$(id_of "$p")$'\n'; done
  done < <(git diff --name-only --no-renames "$base"; git ls-files --others --exclude-standard)
  [[ -n $result ]] || result=$(printf '%s' "$seeds" | reach)
else
  sed -n '2,8s/^# \{0,1\}//p' "$0"; exit 2
fi

# Expand "*" to every member; test-args covers the default members only.
result=$(jq -c --argjson r "$result" '
  .workspace_members as $m | [.packages[] | select(.id as $i | $m | index($i)) | .name] as $all
  | $r | map_values(if . == ["*"] then ($all | sort) else . end)' <<<"$meta")
defaults=$(jq -r '.workspace_default_members as $d | .packages[] | select(.id as $i | $d | index($i)) | .name' <<<"$meta")
args=$(jq -r '.test[]' <<<"$result" | grep -Fxf <(echo "$defaults") | sed 's/^/-p /' | paste -sd' ' || true)
jq -r '"build: \(.build | join(" "))\ntest:  \(.test | join(" "))"' <<<"$result" >&2
echo "any=$([[ -n $args ]] && echo true || echo false)"
echo "build=$(jq -c .build <<<"$result")"
echo "test=$(jq -c .test <<<"$result")"
echo "test-args=$args"
# One flag per selected member: `if: steps.affected.outputs.test-arora-engine`.
jq -r '(.build[] | "build-\(.)=true"), (.test[] | "test-\(.)=true")' <<<"$result"
