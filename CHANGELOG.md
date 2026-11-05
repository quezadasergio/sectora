# Changelog

All notable changes to Sectora are documented in this file.

## [0.1.0] — 2026-11-04

### Added
- Disk imaging via bundled GNU ddrescue (mapfile, pause/resume)
- Explore FAT32 / exFAT / NTFS images and export files
- Signature carving (JPEG, PNG, PDF, ZIP, GIF)
- Clone safety checks: free space, source≠destination, internal-disk and force-overwrite confirmations
- Per-OS privilege checklist and actionable I/O error hints
- Persistent app log and automatic pause of stale “running” jobs after crash
- GitHub Actions CI (tests) and multi-platform unsigned releases on `v*` tags
- About panel with version and license notes (MIT + ddrescue GPLv2+)

### Notes
- macOS: Apple Silicon only
- APFS / HFS+ are out of scope for v1
- Installers are unsigned (Gatekeeper / SmartScreen may warn)
