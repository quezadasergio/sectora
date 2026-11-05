# Sectora

Cross-platform desktop app for **recovering data from failing external disks**.

Sectora runs on **Linux**, **Windows**, and **macOS Apple Silicon only** (`aarch64-apple-darwin`). Intel Macs are not supported.

## What it does

1. **Clone first** — create a recoverable disk image with **GNU ddrescue** (mapfile, pause/resume), written to a healthy drive.
2. **Explore** — list recoverable files from the image (FAT32, exFAT, NTFS) and export them.
3. **Carve** — signature-based recovery (JPEG, PNG, PDF, ZIP, GIF) when the filesystem metadata is damaged or incomplete.

**Core rule:** never write to the failing disk. Always image it onto healthy storage, then work on the `.img`.

## Urgent: disk already failing

Do **not** wait for Sectora to finish building. Clone now with system `ddrescue` onto a healthy drive:

```bash
# macOS / Linux example — mapfile allows safe resume
ddrescue -f -n /dev/DISK /path/on/healthy/drive/disk.img /path/on/healthy/drive/disk.img.map
ddrescue -d -r3 /dev/DISK /path/on/healthy/drive/disk.img /path/on/healthy/drive/disk.img.map
```

Then open the `.img` in Sectora (**Explore existing image**) to list, carve, and export.

Windows users can use a GNU ddrescue build (e.g. MSYS2) with the same idea: source device → image + `.map` on another disk.

## Stack

| Layer | Technology |
|-------|------------|
| UI | Tauri 2 + TypeScript (Vite) |
| Core | Rust |
| Cloning engine | Bundled **GNU ddrescue** sidecar (not reimplemented) |
| Filesystems (v1) | FAT32, exFAT, NTFS |
| Carving | Header/footer signatures |

Shared app code across OSes; raw disk access and privileges are platform-specific.

## Requirements

### Development

- **Node.js** 20+ (22 recommended)
- **Rust** stable (`rustup`)
- **GNU ddrescue** on the host (or prepared via `scripts/fetch-ddrescue.sh`)
- Platform toolchain for Tauri ([Tauri prerequisites](https://v2.tauri.app/start/prerequisites/))

### macOS

- Apple Silicon only (`aarch64-apple-darwin`)
- **Full Disk Access** for Sectora (or Terminal while developing) to read raw disks
- `brew install ddrescue` recommended for local sidecar setup

### Windows

- Administrator elevation to read `\\.\PhysicalDriveN`
- Visual Studio / MSVC build tools for Rust, plus WebView2 (usually present)

### Linux

- Membership in the `disk` group, or polkit/sudo for `/dev/sd*` / `nvme*`
- Package `gddrescue` (Debian/Ubuntu) or equivalent
- Tauri Linux deps (WebKitGTK, etc.)

## Project layout

```
sectora/
  src/                     # Frontend (TypeScript + CSS)
  src-tauri/               # Rust + Tauri
    binaries/              # ddrescue sidecars per target triple
    src/disk/              # Disk enumeration (per OS)
    src/ddrescue/          # Sidecar spawn, progress, pause/resume
    src/recovery/          # FAT / exFAT / NTFS + carving
  scripts/fetch-ddrescue.sh
  assets/app-icon.png      # App icon (transparent background)
  .github/workflows/release.yml
```

## Run locally (development)

```bash
git clone https://github.com/<you>/sectora.git
cd sectora
npm install
bash scripts/fetch-ddrescue.sh
npm run tauri dev
```

Useful scripts:

| Command | Purpose |
|---------|---------|
| `npm run tauri dev` | Dev app with hot reload |
| `npm run build` | Frontend production build only |
| `npm run tauri build` | Package the desktop app |
| `npm run fetch-ddrescue` | Prepare the ddrescue sidecar for this host |
| `npm run validate-release` | Check versions, files, and local ddrescue sidecar |
| `npm run test:rust` | Run Rust unit/integration tests |

### Demo image (optional)

A small synthetic “disk” can be used to try carving without a real drive:

```bash
# If you already generated demo/clonado.img during development:
# In the app: Explore existing image → path to demo/clonado.img → Carving
```

## App workflow

1. **Home** — start a new image job or open an existing `.img`.
2. **Clone** — pick source disk, choose destination `.img` on a healthy volume, start ddrescue (optional force overwrite `-f`).
3. **Progress** — watch rescued %, errors, pending regions; pause / resume / cancel (mapfile keeps progress).
4. **Explore** — list filesystem entries and/or run carving; select files and export to a healthy folder.

## Local packaging

```bash
bash scripts/fetch-ddrescue.sh
npm run tauri build
```

v1 targets:

| OS | Architecture | Typical artifacts |
|----|----------------|-------------------|
| macOS | aarch64 (Apple Silicon) | `.dmg` / `.app` |
| Windows | x86_64 | NSIS `.exe` |
| Linux | x86_64 | `.deb`, AppImage |

## GitHub Releases (CI)

Pushing a tag matching `v*` runs [`.github/workflows/release.yml`](.github/workflows/release.yml):

- Builds **unsigned** installers for macOS (Apple Silicon), Windows x64, and Linux x64
- Creates/updates the GitHub Release for that tag
- Attaches the installers as release assets
- Bundles a real **ddrescue** binary per platform (Homebrew / apt / MSYS2 build)
- Tags containing `rc` / `beta` / `alpha` are marked as **prerelease**

```bash
# Local prerequisite check
npm run validate-release

# Release candidate (recommended first)
git tag v0.1.0-rc.1
git push origin v0.1.0-rc.1

# Production tag
git tag v0.1.0
git push origin v0.1.0
```

You can also run a **dry-run build** from the Actions tab (`workflow_dispatch` with `dry_run=true`) to produce installers as CI artifacts without publishing a Release.

PRs and pushes to `main`/`master` run [`.github/workflows/ci.yml`](.github/workflows/ci.yml) (frontend build + Rust tests + sidecar check).

Then open the repository **Releases** page and download the assets for your OS.

### Unsigned builds

Releases are **not code-signed** (by design for now):

- **macOS Gatekeeper** may block first open — use *Right-click → Open*, or allow in System Settings.
- **Windows SmartScreen** may warn — choose “More info” → run anyway if you trust the build.

## Privileges & safety

- Grant only the access needed to **read** the failing disk and **write** the image/export destination.
- Confirm destination paths carefully before enabling force overwrite.
- Keep the mapfile (`.img.map`) next to the image so interrupted clones can resume.

## Third-party: GNU ddrescue

Sectora shells out to **GNU ddrescue** for imaging.

- Upstream: https://www.gnu.org/software/ddrescue/
- License: **GPLv2 or later**
- Author: Antonio Diaz Diaz

Details and redistribution notes: [THIRD_PARTY_DDRESCUE.md](THIRD_PARTY_DDRESCUE.md).

Prepare / refresh the sidecar:

```bash
# Unix (macOS Apple Silicon / Linux x86_64)
bash scripts/fetch-ddrescue.sh

# CI sets REQUIRE_DDRESCUE=1 so a missing real binary fails the build
```

## License

- Sectora application code: **MIT** — see [LICENSE](LICENSE)
- Bundled GNU ddrescue: **GPLv2+** — see [THIRD_PARTY_DDRESCUE.md](THIRD_PARTY_DDRESCUE.md)
