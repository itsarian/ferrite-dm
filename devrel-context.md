# Developer Relations Context: Ferrite DM

## General Project Context
- **Repository:** itsarian/ferrite-dm
- **Project Type:** Desktop application (macOS GPUI) + CLI tool and background daemon (Rust)
- **Primary Audience:** Solo developers & desktop power users (individual adoption)
- **Primary Outcome:** Installs/active users (frictionless download & first run) + GitHub stars/visibility
- **Primary Alternative:** Free Download Manager (FDM 6.x), aria2, curl/wget
- **Key Differentiator:** 100% local-first and telemetry-free; native GPUI (no Electron/Chromium); unified Unix domain socket IPC (`ferrite.sock`) between GUI and CLI; multi-protocol (HTTP, BitTorrent, HLS/DASH); FDM browser extension compatibility.
- **Licence:** Dual MIT / Apache-2.0 (`LICENSE-MIT`, `LICENSE-APACHE`)
- **Status:** Actively maintained, early release (`v0.1.0`), macOS verified, Linux CLI/daemon compatible.

## Tasks
### 2026-11-13: README Optimization Audit & Rewrite (Completed)
- Post-rewrite mechanical audit via `scripts/readme-audit.sh` passed.
- Band A: 5/5 pass (A1 pass, A2 pass, A3 pass, A4 pass, A5 pass).
- Band B: 15/16 (exceeds 14/16 threshold).
- Cold-reader test: 5/5 pass.
