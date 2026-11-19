# Design — Simple STT Settings

The settings editor is a quiet workbench: fast to scan, explicit about pending changes, and friendly to both first-time and technical users.

## Genre and structure

Modern-minimal desktop utility. App pages use a persistent section rail and one focused task column. Common choices appear first; technical details use disclosures.

## Theme and type

Low-chroma cool paper and ink with one cyan-blue action colour. Display uses Segoe UI Variable Display; body uses Segoe UI Variable Text; Cascadia Mono is reserved for paths and JSON. The interface follows a four-point spacing scale.

## Interaction

Explicit Save with a sticky pending-changes bar. Long device and model lists use searchable comboboxes. Windows shortcuts are captured by AutoHotkey; Linux shortcuts remain compositor-owned. Feedback is inline, motion is restrained, and reduced-motion is respected.

## Responsive behavior

At narrow widths the section rail becomes a horizontal tab strip. Controls stack without horizontal scrolling down to 320 CSS pixels and clickable labels stay on one line.
