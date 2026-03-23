#!/usr/bin/env bash
set -euo pipefail

matrix_file="docs/mamba-compat-matrix.md"

if [[ ! -f "$matrix_file" ]]; then
  echo "error: missing $matrix_file" >&2
  exit 1
fi

failures=0

trim() {
  local s="$1"
  s="${s#${s%%[![:space:]]*}}"
  s="${s%${s##*[![:space:]]}}"
  printf '%s' "$s"
}

is_path_like() {
  local token="$1"
  [[ "$token" == */* ]] || [[ "$token" =~ \.(rs|py|cpp|md|snap|yml|yaml|sh)$ ]]
}

normalize_path_token() {
  local token="$1"
  printf '%s' "${token%%#*}"
}

resolve_existing_path() {
  local token="$1"
  if [[ -e "$token" ]]; then
    printf '%s' "$token"
    return 0
  fi
  if [[ -e "crates/viper-cli/$token" ]]; then
    printf '%s' "crates/viper-cli/$token"
    return 0
  fi
  return 1
}

extract_backtick_tokens() {
  local text="$1"
  grep -oE '`[^`]+`' <<<"$text" | tr -d '`' || true
}

while IFS=$'\t' read -r line_no command behavior upstream viper; do
  command="$(trim "$command")"
  behavior="$(trim "$behavior")"
  upstream="$(trim "$upstream")"
  viper="$(trim "$viper")"

  if [[ -z "$upstream" ]]; then
    echo "$matrix_file:$line_no: missing upstream reference for row '$command | $behavior'" >&2
    failures=$((failures + 1))
  fi
  if [[ -z "$viper" ]]; then
    echo "$matrix_file:$line_no: missing viper enforcement for row '$command | $behavior'" >&2
    failures=$((failures + 1))
  fi

  if [[ -n "$upstream" && "$upstream" != *"mamba/"* ]]; then
    echo "$matrix_file:$line_no: upstream reference must cite mamba source/tests for '$command | $behavior'" >&2
    failures=$((failures + 1))
  fi
  if [[ -n "$viper" && "$viper" != *"crates/"* ]]; then
    echo "$matrix_file:$line_no: viper enforcement must cite repository tests/code for '$command | $behavior'" >&2
    failures=$((failures + 1))
  fi

  mapfile -t upstream_tokens < <(extract_backtick_tokens "$upstream")
  mapfile -t viper_tokens < <(extract_backtick_tokens "$viper")

  upstream_paths=()
  for token in "${upstream_tokens[@]}"; do
    if is_path_like "$token"; then
      upstream_paths+=("$token")
    fi
  done

  viper_paths=()
  viper_symbols=()
  for token in "${viper_tokens[@]}"; do
    if is_path_like "$token"; then
      viper_paths+=("$token")
    elif [[ "$token" != --* && "$token" != *" "* ]]; then
      viper_symbols+=("$token")
    fi
  done

  if (( ${#upstream_paths[@]} == 0 )); then
    echo "$matrix_file:$line_no: upstream reference must include at least one concrete file path in backticks for '$command | $behavior'" >&2
    failures=$((failures + 1))
  fi

  if (( ${#viper_paths[@]} == 0 )); then
    echo "$matrix_file:$line_no: viper enforcement must include at least one concrete file path in backticks for '$command | $behavior'" >&2
    failures=$((failures + 1))
  fi

  for path in "${upstream_paths[@]}"; do
    resolved_path="$(normalize_path_token "$path")"
    if ! resolved_path="$(resolve_existing_path "$resolved_path")"; then
      echo "$matrix_file:$line_no: upstream path '$path' does not exist" >&2
      failures=$((failures + 1))
    fi
  done

  for path in "${viper_paths[@]}"; do
    resolved_path="$(normalize_path_token "$path")"
    if ! resolved_path="$(resolve_existing_path "$resolved_path")"; then
      echo "$matrix_file:$line_no: viper path '$path' does not exist" >&2
      failures=$((failures + 1))
    fi
  done

  for symbol in "${viper_symbols[@]}"; do
    found=0
    for path in "${viper_paths[@]}"; do
      resolved_path="$(normalize_path_token "$path")"
      if ! resolved_path="$(resolve_existing_path "$resolved_path")"; then
        continue
      fi
      if rg -nF "$symbol" "$resolved_path" >/dev/null 2>&1; then
        found=1
        break
      fi
    done
    if (( found == 0 )); then
      echo "$matrix_file:$line_no: symbol '$symbol' was not found in referenced viper paths for '$command | $behavior'" >&2
      failures=$((failures + 1))
    fi
  done
done < <(
  awk -F'|' '
  function trim(s) {
    gsub(/^[ \t]+|[ \t]+$/, "", s)
    return s
  }
  /^\|/ {
    if ($0 ~ /^\|---/) {
      next
    }
    c1 = trim($2)
    if (c1 == "" || c1 == "Command" || c1 == "Area") {
      next
    }
    printf "%d\t%s\t%s\t%s\t%s\n", NR, c1, trim($3), trim($4), trim($5)
  }
  ' "$matrix_file"
)

if (( failures > 0 )); then
  exit 1
fi

echo "compatibility matrix check passed"
