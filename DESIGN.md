---
name: Simple STT Settings
description: Compact, task-focused settings for desktop dictation.
colors:
  paper: "#fafbfd"
  rail: "#f0f3f8"
  raised: "#fff"
  ink: "#202b3c"
  muted: "#526176"
  line: "#dce3ec"
  accent: "#2859c6"
  accent-soft: "#e5edfc"
  focus: "#3566d8"
  danger: "#b93442"
  success: "#27724c"
  paper-dark: "#151a23"
  rail-dark: "#1b212c"
  raised-dark: "#202835"
  ink-dark: "#e4eaf3"
  muted-dark: "#a5b3c7"
  line-dark: "#333e50"
  accent-dark: "#8bafff"
  accent-soft-dark: "#263955"
  focus-dark: "#9cbbff"
  danger-dark: "#ff97a3"
  success-dark: "#86ccaa"
typography:
  body:
    fontFamily: 'ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif'
    fontSize: "14px"
    fontWeight: 400
  headline:
    fontFamily: 'ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif'
    fontSize: "26px"
    fontWeight: 650
    lineHeight: 1.2
    letterSpacing: "-0.025em"
  title:
    fontFamily: 'ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif'
    fontSize: "14px"
    fontWeight: 650
  label:
    fontFamily: 'ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif'
    fontSize: "13px"
    fontWeight: 500
rounded:
  control: "7px"
  dialog: "12px"
spacing:
  row-compact: "5px"
  control: "8px"
  page: "48px"
components:
  button-primary:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.raised}"
    rounded: "{rounded.control}"
    height: "34px"
    padding: "6px 12px"
  field:
    backgroundColor: "{colors.raised}"
    textColor: "{colors.ink}"
    rounded: "{rounded.control}"
    height: "36px"
    padding: "7px 10px"
---

# Design System: Simple STT Settings

## Overview

**Creative North Star: "A Clear Home for Every Decision"**

Simple STT Settings is a compact, quiet workspace for configuring desktop dictation. The navigation names six task destinations, and each choice has one clear home. Flat aligned rows keep the interface practical and let labels, values, and control state carry the meaning.

The inherited Simple STT visual identity uses a restrained blue accent, neutral light and dark surfaces, concise labels, and authored SVG icons. Search helps users find settings across destinations. On narrow screens the task navigation adapts while retaining visible names.

**Key Characteristics:**
- Task-led navigation with six named destinations.
- Compact flat rows and aligned controls.
- One blue accent over neutral light or dark surfaces.
- Searchable choices and sliders paired with exact numeric values.

## Colors

A restrained blue accent marks selection and focus against cool neutral surfaces; semantic red and green are reserved for error and success states.

### Primary
- **Simple Blue**: The light-theme accent for active navigation, primary actions, selected values, and slider fill.
- **Night Blue**: The dark-theme accent for the same interactive roles.

### Neutral
- **Paper / Night Paper**: Main page backgrounds in light and dark themes.
- **Rail / Night Rail**: Sidebar backgrounds, distinct from the page surface.
- **Raised / Night Raised**: Controls and floating surfaces.
- **Ink / Night Ink**: Main text.
- **Muted / Night Muted**: Secondary labels and supporting information.
- **Line / Night Line**: Dividers, control borders, and outlines.
- **Soft Blue / Night Soft Blue**: Selected navigation, search results, and hover surfaces.
- **Focus Blue / Night Focus Blue**: Visible keyboard focus outline.
- **Error Red / Night Error Red** and **Success Green / Night Success Green**: Concise status feedback.

**The One Accent Rule.** Use the blue accent for interaction and hierarchy; keep broad surfaces neutral.

## Typography

**Display Font:** No separate display face; use the system UI sans-serif stack.
**Body Font:** `ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif`.
**Label/Mono Font:** Labels use the body stack. Code and configuration use a monospace stack.

**Character:** Compact and familiar, with a restrained weight change between labels, section titles, and page headings. Arabic uses the bundled Noto Sans Arabic face while retaining the same compact scale.

### Hierarchy
- **Headline** (650, 26px, 1.2): Page title; reduces to 23px on narrow screens.
- **Title** (650, 14px): Setting group headings.
- **Body** (400, 14px): Default application text.
- **Label** (500, 13px): Field labels; supporting and metadata text steps down to 12px and 11px.

## Layout

The desktop shell uses a fixed 200px navigation rail and a centered content column capped at 840px. Main content has 39px top and 48px side padding at wide widths, tightening at the 950px and 700px breakpoints. Settings are arranged as aligned label/control rows, with a subtle divider at each group heading. At 700px and below, the rail becomes a top region and the six destinations form a three-column grid with icon and visible name; fields stack vertically except compact switches. Arabic direction is supported with logical spacing and mirrored directional icons.

## Elevation & Depth

The content system is flat at rest. Dividers, distinct neutral surfaces, and soft accent fills establish hierarchy. Search menus, dropdowns, the save bar, and notices use a restrained diffuse shadow to separate overlays from the page.

### Shadow Vocabulary
- **Floating surface** (`0 8px 24px #0002`): Search results and choice menus.
- **Save and notice** (`0 6px 24px #0002`, `0 6px 22px #0002`): Persistent save actions and transient notices.

## Shapes

Controls and ordinary buttons use gently rounded corners (7px). Dialogs and sign-in surfaces use a broader 12px radius; the save bar uses 9px. Fields have a fine neutral border, while section rows remain unboxed and separated by rules. Switches and status marks use circular geometry.

## Components

### Buttons
- **Character:** Compact and direct, with a quiet bordered default and a blue primary save action.
- **Shape:** Gently rounded (7px), at least 34px tall.
- **Primary:** Accent fill with raised-surface text and semibold weight.
- **Hover / Active:** Soft accent surface on hover and line-colored surface while pressed; keyboard focus has a visible 2px outline.
- **Quiet / icon:** Transparent at rest; icon actions use a 34px target.

### Inputs / Fields
- **Style:** Raised surface, fine line border, 7px corners, and a 36px minimum height.
- **Focus:** A 2px focus-colored outline with 3px offset.
- **Range:** Pair the slider with an editable exact-value field; use tabular numerals for the value.
- **Search and choice:** Searchable comboboxes filter choices and keep menus clear of clipping containers.

### Navigation
- **Style:** A fixed desktop rail with concise SVG icons and visible task names. Active destination uses the soft accent surface and accent text.
- **Mobile:** Three-column top navigation retains both icon and short name; supporting descriptions are hidden.

### Cards / Containers
- Settings groups use open rows and dividers rather than enclosing cards. Floating menus and the save bar use a raised surface, fine border, and restrained shadow.

## Do's and Don'ts

### Do:
- **Do** give each decision one clear task destination.
- **Do** keep setting labels visible and align their controls consistently.
- **Do** pair sliders with an editable exact value.
- **Do** preserve visible task names on mobile and accessible names for icon controls.
- **Do** support light, dark, automatic theme, RTL content, and reduced motion.

### Don't:
- **Don't** make broad page regions blue; reserve the accent for interaction and hierarchy.
- **Don't** hide task names on narrow screens.
- **Don't** replace aligned settings rows with oversized decorative containers.
