# Settings information architecture

The settings overhaul keeps the existing compact visual system and gives every
control one primary home. Six destinations describe tasks and ownership:
General (appearance/startup), Dictation (recording/formatting/delivery), Reading
(read-along/playback/text preparation), Models (downloads/provider/model/language
assignments and engine-specific options), Text cleanup (instructions and shared
processing), System (memory/server/privacy/diagnostics/JSON).

## Research and decisions

- [NN/g information architecture guidance](https://www.nngroup.com/articles/ia-study-guide/)
  treats clear labels and information scent as findability requirements. Menu
  names use user tasks, with short scope descriptions rather than internal
  process names or ambiguous sections.
- [VS Code Settings](https://code.visualstudio.com/docs/configure/settings)
  provides searchable structured settings alongside JSON. Simple STT retains
  both, and search includes field labels, help, aliases and owning page.
- [GOV.UK select guidance](https://design-system.service.gov.uk/components/select/)
  advises choosing controls for the number of options rather than defaulting to
  selects. [Scottish Government autocomplete](https://designsystem.gov.scot/components/autocomplete)
  supports typing to find a suggestion. Long model/runtime/language/voice lists
  use the same searchable, keyboard-operable picker across features.
- [NN/g slider guidance](https://www.nngroup.com/articles/gui-slider-controls/)
  highlights precise-value and touch-control difficulties. Range controls have
  a labeled slider and synchronized exact-number input; units remain visible.
- A [KDE user discussion](https://www.reddit.com/r/kde/comments/1snlhbc/the_settings_menu_is_fine/)
  includes complaints about unexpectedly nested settings and controls available
  only through unrelated context menus. This is anecdotal feedback, not a
  prevalence estimate. The overhaul avoids duplicate model editors and gives
  search results a stable destination.

## Preservation

Existing JSON keys, model assignments, voices and per-language values are
preserved. Shared reading preferences show mixed values until explicitly
changed; new language assignments inherit the common defaults. Changing a
model/runtime no longer resets shared text preparation. Model-specific options
such as Supertonic synthesis steps and Kokoro preprocessing remain in Models.

Finish delay defaults to 250 ms for new/reset configurations. Saved custom
values remain intact. Server authentication still uses a single bearer token
across its inference endpoints. Provider credentials are kept in the existing
secret stores, never JSON or frontend storage.
