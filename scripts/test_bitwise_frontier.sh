#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_dir="$(cd -- "${script_dir}/.." && pwd)"
cd "${repo_dir}"

if [[ -n "$(git status --porcelain)" ]]; then
    worktree_state="dirty"
else
    worktree_state="clean"
fi

echo "baseline_commit=$(git rev-parse HEAD)"
echo "baseline_branch=$(git branch --show-current)"
echo "baseline_worktree=${worktree_state}"
echo "rustc=$(rustc --version)"
echo "cargo=$(cargo --version)"
echo "profile=release features=all bit_count=64 semantic_checks=200"

echo "[1/3] Focused unit tests"
cargo test -p rumba-core --lib --all-features

echo "[2/3] QSynth EA lines 53, 249, 260, 369 and 481"
cargo run \
    -p rumba-core \
    --example bitwise_frontier_baseline \
    --release \
    --features parse

echo "[3/3] Complete dataset corpus"
cargo test datasets --release --all-features -- --nocapture
