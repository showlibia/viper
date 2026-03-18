# Viper Micromamba MVP

This repository now contains a Rust workspace with:

- `crates/viper-core`: core command execution, config, prefix state management.
- `crates/viper-cli`: `viper` CLI compatible with core micromamba workflows.

## Implemented commands

- `create`
- `install`
- `remove` (including `--all`)
- `list`
- `info`
- `config list|get|set`

## Implemented high-frequency global options

- `-r, --root-prefix`
- `-p, --prefix`
- `-n, --name`
- `-c, --channel`
- `--json`
- `-y, --yes`
- `--dry-run`
- `--no-rc`
- `--offline`
- `-v`

## Current architecture note

The current core implements environment metadata and package state tracking through
`conda-meta/viper-state.json` to keep CLI behavior and integration points stable.
The SAT solver / repodata download / package linking path is the next milestone
that will replace state-only install/remove internals while keeping the same CLI contract.
