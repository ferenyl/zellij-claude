# Changelog

All notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

A section named after a release version (`## [1.2.3] - YYYY-MM-DD`) is copied into
the GitHub Release notes for that version.

## [Unreleased]

## [0.1.0] - 2026-10-16

### Added

- Picker: floating list of all Claude Code sessions across tabs and Zellij sessions,
  grouped by Zellij session, with type-to-filter, jump-to-waiting and `Enter` to jump
  to the pane (also in other Zellij sessions).
- Status bar: total and per-state counts, horizontal or vertical, left/right and
  top/bottom alignment, colors, optional blinking for waiting sessions.
- Claude Code hook script writing per-pane status files and notifying the plugin
  over `zellij pipe`.
- Configurable icons (Nerd Font and Unicode sets), colors, keys and behavior.
- Cleanup of ended, closed and stale sessions and their status files.
- Optional fallback detection of Claude panes without hooks.
- Optional `bind_open_key` to register the picker key at load time.
