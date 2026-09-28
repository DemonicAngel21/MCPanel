# ADR-0002: Tauri 2 + React/TypeScript + Rust

- Status: Accepted
- Date: 2026-09-28

## Context

MCPanel needs deep native access (processes and process trees, reparse-point-aware file
operations, Windows Job Objects, DPAPI/Credential Manager, hashing/compression/crypto of
multi-GB data) while sitting in the tray next to a memory-hungry JVM. Options compared:
Tauri 2 + Rust, Electron + Node, .NET (Avalonia/WinUI), Wails, Flutter.

## Decision

Tauri 2 (WebView2) host, React + strict TypeScript UI, Rust core. Stable Tauri 2.x is
used (Tauri 3 is alpha at the time of writing and is not adopted).

## Consequences

- Native process/filesystem control and a default-deny IPC capability model.
- Lower memory footprint than Electron (WebView2 is still multi-process; realistic
  savings are roughly 30–50%, not 10x).
- Cost: Rust learning curve and compile times; two languages.
