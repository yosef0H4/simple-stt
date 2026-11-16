# Model selection: mode-based settings

Research note, 2026-11-05. Scope: local speech, reading, and OCR model assignment in the existing settings UI.

## Evidence

- Apple's [layout guidance](https://developer.apple.com/design/human-interface-guidelines/layout) recommends grouping related controls and using progressive disclosure to reduce the amount initially displayed. It also recommends adapting reading order for right-to-left languages and allowing layouts to reflow for larger text.
- Apple's [settings guidance](https://developer.apple.com/design/human-interface-guidelines/settings) recommends minimizing settings and using good defaults; its [writing guidance](https://developer.apple.com/design/human-interface-guidelines/writing) recommends practical, simple labels and explanations where a label alone is insufficient.
- Apple's [disclosure guidance](https://developer.apple.com/design/human-interface-guidelines/disclosure-controls) recommends keeping commonly used controls visible, placing advanced details behind a clearly labeled disclosure, and keeping the disclosure near its content. This supports hiding model-specific cleanup options rather than hiding the active model itself.

These sources are general design guidance, not evidence that a particular selector has been tested with this application's users. The proposal below is an application-specific inference.

## Final interaction

The user explicitly rejected general fallbacks and inherited settings. This requirement takes precedence over the earlier fallback proposal.

1. Keep Mode first: one model, follow keyboard language, or (for reading) detect text language.
2. One-model mode edits a single assignment. Language modes select the language and edit its runtime, model, device, speed, applicable voice and cleanup once. Selecting a language changes only the editor context, not the active routing mode.
3. Automatic reading has only explicit language assignments, with Add language and Remove language. There is no general assignment, Other languages entry, override concept or unclear-language setting. An unassigned detected language requires configuration instead of silently using another language's assignment. Even a multilingual model must be assigned separately to each language the user wants to read.
4. Keep model-specific cleanup details close to their assignment. Background timers and selection-copy behavior remain module-wide controls rather than extra assignment layers.
5. Preserve inactive mode assignments and unsaved changes through navigation, Save and reopen. Editing or removing Arabic must not change English. New local speeds default to 1× without a hidden global speed inheritance.
6. Verify English/Arabic RTL, narrow screens, empty setup, independent language values, and missing-language runtime rejection with isolated browser and Rust tests.
