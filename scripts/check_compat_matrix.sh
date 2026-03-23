#!/usr/bin/env bash
set -euo pipefail

matrix_file="docs/mamba-compat-matrix.md"

if [[ ! -f "$matrix_file" ]]; then
  echo "error: missing $matrix_file" >&2
  exit 1
fi

awk -F'|' '
function trim(s) {
  gsub(/^[ \t]+|[ \t]+$/, "", s)
  return s
}
BEGIN { failures = 0 }
/^\|/ {
  if ($0 ~ /^\|---/) {
    next
  }

  col1 = trim($2)
  col2 = trim($3)
  col3 = trim($4)
  col4 = trim($5)

  if (col1 == "" || col1 == "Command" || col1 == "Area") {
    next
  }

  if (col3 == "") {
    printf("%s:%d: missing upstream reference for row `%s | %s`\n", FILENAME, NR, col1, col2) > "/dev/stderr"
    failures++
  }
  if (col4 == "") {
    printf("%s:%d: missing viper enforcement for row `%s | %s`\n", FILENAME, NR, col1, col2) > "/dev/stderr"
    failures++
  }

  if (col3 != "" && col3 !~ /mamba\//) {
    printf("%s:%d: upstream reference must cite mamba source/tests for `%s | %s`\n", FILENAME, NR, col1, col2) > "/dev/stderr"
    failures++
  }
  if (col4 != "" && col4 !~ /crates\//) {
    printf("%s:%d: viper enforcement must cite repository tests/code for `%s | %s`\n", FILENAME, NR, col1, col2) > "/dev/stderr"
    failures++
  }
}
END {
  if (failures > 0) {
    exit 1
  }
}
' "$matrix_file"

echo "compatibility matrix check passed"
