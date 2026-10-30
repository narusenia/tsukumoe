#!/usr/bin/env bash
# One line, English, `<type>: <lowercase description>`.
set -euo pipefail

msg=$(grep -v '^#' "$1" | sed '/^[[:space:]]*$/d')

if [ "$(printf '%s\n' "$msg" | wc -l)" -ne 1 ]; then
  echo "commit message must be a single line" >&2
  exit 1
fi
if ! printf '%s\n' "$msg" | grep -Eq '^(feat|fix|refactor|docs|test|chore|perf|ci): [a-z0-9`]'; then
  echo "commit message must match '<feat|fix|refactor|docs|test|chore|perf|ci>: <lowercase description>'" >&2
  exit 1
fi
