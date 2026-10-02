# Simple STT

<!-- impeccable:product-schema 1 -->

## Platform
web

## Stack
Svelte, strict TypeScript, and Vite for browser Settings; Rust serves embedded static assets. Windows uses an AutoHotkey shell; Linux has a Rust shell. Node is a development/build dependency only.

## Product Purpose
Lightweight desktop dictation. Users install speech models, select one model or assign models to configured keyboard languages, and dictate into their current application.

## Operating Context
Windows and Linux. Settings is an occasional task in the user's existing browser, served by a disposable authenticated loopback process. Dictation and inference remain separate processes.

## Capabilities and Constraints
Explicit Save drafts; schema 9; nullable None assignments; separate installer; physical Vulkan GPU selection; optional AI cleanup and screen context. Preserve model routing, worker isolation, clipboard safety, platform shortcuts, and existing assignments.

## Product Principles
Keep resident RAM and idle work low. Make common settings visible and fast to change. Use minimal text and clear icons without losing accessible names. Preserve behavior during the visual overhaul.

## Evidence on Hand
Rust tests, authenticated Settings API tests, deterministic browser fixtures, real Linux English/Arabic and GPU tests. Native Windows runtime validation requires a Windows host. No invented performance claims.
