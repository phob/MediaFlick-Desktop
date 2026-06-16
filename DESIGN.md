---
name: MediaFlick Desktop
description: External mpv playback for Jellyfin with a Jellyfin-compatible desktop shell.
colors:
  media-black: "#101010"
  chrome-charcoal: "#1d1d1d"
  panel-charcoal: "#292929"
  raised-charcoal: "#303030"
  text-primary: "#e3e3e3"
  text-secondary: "#b3b3b3"
  text-disabled: "#808080"
  jellyfin-cyan: "#00a4dc"
  mediaflick-violet: "#aa5cc3"
  mediaflick-blue: "#00a4dc"
  warning-tomato: "#dd4919"
  danger-red: "#cc3333"
typography:
  headline:
    fontFamily: "Noto Sans, Segoe UI, system-ui, sans-serif"
    fontSize: "28px"
    fontWeight: 600
    lineHeight: 1.2
    letterSpacing: "-0.01em"
  title:
    fontFamily: "Noto Sans, Segoe UI, system-ui, sans-serif"
    fontSize: "22px"
    fontWeight: 500
    lineHeight: 1.25
  body:
    fontFamily: "Noto Sans, Segoe UI, system-ui, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.35
  label:
    fontFamily: "Noto Sans, Segoe UI, system-ui, sans-serif"
    fontSize: "14px"
    fontWeight: 600
    lineHeight: 1.25
rounded:
  media-sm: "3px"
  control-md: "4px"
  dialog-lg: "12px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "32px"
components:
  button-primary:
    backgroundColor: "{colors.jellyfin-cyan}"
    textColor: "{colors.media-black}"
    rounded: "{rounded.control-md}"
    padding: "12px 16px"
  button-secondary:
    backgroundColor: "{colors.raised-charcoal}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control-md}"
    padding: "12px 16px"
  media-card:
    backgroundColor: "{colors.media-black}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.media-sm}"
  input-field:
    backgroundColor: "{colors.panel-charcoal}"
    textColor: "{colors.text-primary}"
    rounded: "{rounded.control-md}"
    padding: "12px 14px"
---

# Design System: MediaFlick Desktop

## 1. Overview

**Creative North Star: "The Quiet Projection Room"**

MediaFlick Desktop should feel like a dim media room control surface wrapped around Jellyfin Web. The product's chrome is dark, restrained, and practical so posters, backdrops, and playback controls stay visually dominant. App-owned surfaces should sit comfortably beside Jellyfin's demo interface: black and charcoal layers, cyan progress and action states, compact sans typography, and minimal decoration.

The system explicitly rejects unrelated SaaS dashboards, neon gamer skins, marketing hero layouts, decorative glass, and heavy purple gradients across surfaces. MediaFlick can use its own violet-to-cyan gradient mark as identity, but the operational UI should remain calm and Jellyfin-compatible.

**Key Characteristics:**

- Dark-first, media-native surfaces.
- Jellyfin cyan for action, progress, focus, and selected state.
- Poster and backdrop imagery provide atmosphere; chrome stays quiet.
- Compact desktop density with predictable controls.
- Clear distinction between Jellyfin baseline compatibility and MediaFlick's own brand mark.

## 2. Colors

The palette is a restrained Jellyfin-compatible dark system with one operational accent and a small app-brand gradient reserved for marks and installer artwork.

### Primary

- **Jellyfin Cyan** (`#00a4dc`): Primary action, progress bars, selected tabs, focus rings, active links, and meaningful playback or update progress. Use sparingly so it remains a state signal.

### Secondary

- **MediaFlick Violet** (`#aa5cc3`): Brand-mark gradient only, paired with Jellyfin Cyan. Do not use it as a general UI accent.
- **Warning Tomato** (`#dd4919`): Transcoding, warning, or update caution states where cyan would imply normal progress.

### Neutral

- **Media Black** (`#101010`): Main app background, dark media wells, and surrounding shell surfaces.
- **Chrome Charcoal** (`#1d1d1d`): Top bars, drawer chrome, and persistent navigation.
- **Panel Charcoal** (`#292929`): Dialogs, setup panels, secondary buttons, and inactive raised controls.
- **Raised Charcoal** (`#303030`): Hovered or raised controls.
- **Primary Text** (`#e3e3e3`): Headings, labels, and primary readable text on dark surfaces.
- **Secondary Text** (`#b3b3b3`): Metadata, descriptions, helper text, and timestamps.
- **Disabled Text** (`#808080`): Disabled or unavailable UI states.

### Named Rules

**The Cyan Means State Rule.** Cyan is for action, progress, selection, and focus. If a cyan element does not communicate state or action, remove it.

**The Gradient Is a Signature Rule.** The violet-to-cyan gradient belongs to the MediaFlick mark, installer artwork, and rare brand moments. It should not wash across dialogs, buttons, or content surfaces.

## 3. Typography

**Display Font:** Noto Sans, Segoe UI, system-ui, sans-serif
**Body Font:** Noto Sans, Segoe UI, system-ui, sans-serif
**Label/Mono Font:** Use the body stack unless code paths or logs require a system monospace.

**Character:** Practical, familiar, and media-center native. Typography should feel close to Jellyfin Web and native desktop UI rather than editorial or promotional.

### Hierarchy

- **Headline** (600, 28px, 1.2): Dialog titles, first-run screen titles, and app-level page headings.
- **Title** (500 to 600, 22px, 1.25): Section headings such as settings groups, update panels, and media-related summaries.
- **Body** (400, 16px, 1.35): Descriptions, setup help, about text, and normal content. Keep prose to 65 to 75 characters where possible.
- **Metadata** (400, 14px, 1.35): URLs, mpv paths, version strings, playback details, and timestamps.
- **Label** (600, 14px, 1.25): Field labels, buttons, tabs, and compact controls.

### Named Rules

**The No Display Labels Rule.** Do not use decorative or display type for buttons, labels, settings, or metadata. Product UI earns trust through familiar text rendering.

## 4. Elevation

MediaFlick uses tonal layering first and shadows second. Jellyfin's own UI is mostly flat: black page background, charcoal chrome, slightly lighter controls, and poster cards with small shadows. App-owned dialogs may use ambient shadow to separate from embedded content, but normal panels should rely on background color, borders, and spacing.

### Shadow Vocabulary

- **Media Card Rest** (`box-shadow: 0 1px 4px rgba(0, 0, 0, 0.37)`): Poster and backdrop cards when a surface needs separation from black.
- **Dialog Ambient** (`box-shadow: 0 24px 72px rgba(0, 0, 0, 0.45)`): Setup, about, settings, and update dialogs over dark content.
- **Focus Glow** (`box-shadow: 0 0 0 3px rgba(0, 164, 220, 0.22)`): Keyboard focus on inputs and controls.

### Named Rules

**The Flat Until Needed Rule.** Surfaces are flat by default. Elevation appears for dialogs, overlays, focus, and hover feedback only.

## 5. Components

### Buttons

- **Shape:** Compact rounded rectangle, Jellyfin-like radius (`4px`).
- **Primary:** Jellyfin Cyan background with dark text when the control is the main action. Use for connect, save, install update, and similar committed actions.
- **Secondary:** Panel Charcoal or Raised Charcoal background with Primary Text. Use for browse, cancel, later, and utility actions.
- **Hover / Focus:** Hover lifts by tone, not by motion-heavy effects. Focus uses the cyan focus glow. Disabled states reduce opacity and remove hover effects.

### Chips

- **Style:** Use small dark pills only when filtering or showing status. Text uses Primary Text or Secondary Text.
- **State:** Selected chips may use cyan text or a subtle cyan-tinted background, never full saturation on inactive chips.

### Cards / Containers

- **Corner Style:** Media cards use a small radius (`3px`) matching Jellyfin's poster and backdrop language. Dialogs may use `12px` when they are clearly app-owned.
- **Background:** Media cards sit on Media Black. App-owned panels use Panel Charcoal with a subtle border or tonal contrast.
- **Shadow Strategy:** Use Media Card Rest for poster-like cards and Dialog Ambient for overlays only.
- **Border:** Prefer subtle neutral borders over decorative accents.
- **Internal Padding:** Compact controls use 12 to 16px. Dialog groups use 20 to 24px.

### Inputs / Fields

- **Style:** Dark field background (`#292929`), Primary Text, Secondary Text placeholders, `4px` radius, and a subtle neutral border.
- **Focus:** Cyan border or cyan focus glow. Never rely on color alone; keep visible outlines.
- **Error / Disabled:** Error text uses Danger Red with clear copy. Disabled fields use lower opacity and keep labels readable.

### Navigation

- **Style:** Fixed dark chrome, compact icon buttons, text tabs, and cyan or bright text for selected state. Inactive tabs use Secondary Text.
- **Behavior:** Navigation should be predictable and Jellyfin-compatible. Do not invent unusual tab, sidebar, or drawer mechanics for app-level controls.
- **Mobile / Narrow:** Collapse secondary actions before reducing readability. Keep primary playback and setup actions reachable by keyboard.

### Signature Component

**MediaFlick Brand Mark:** Use the existing rounded dark tile with violet-to-cyan play geometry. It can appear on first launch, about, installer, update, and app-level empty states. Keep it separate from Jellyfin's logo and avoid placing both marks as if they were a single brand.

## 6. Do's and Don'ts

### Do:

- **Do** use Media Black (`#101010`) and charcoal layers as the default surface language.
- **Do** use Jellyfin Cyan (`#00a4dc`) for action, progress, selection, and focus.
- **Do** let posters, backdrops, and media metadata provide visual richness.
- **Do** keep setup and settings controls compact, predictable, and keyboard accessible.
- **Do** preserve reduced-motion behavior and WCAG AA contrast on app-owned surfaces.
- **Do** make mpv handoff, update, and validation states explicit through text plus visual state.
- **Do** keep dialog copy terse. About dialogs should show identity, version, creator, links, and actions without long product descriptions.

### Don't:

- **Don't** make MediaFlick look like an unrelated SaaS dashboard, generic Electron settings app, or neon gamer skin.
- **Don't** use decorative glassmorphism, heavy purple gradients across surfaces, marketing-style hero sections, or oversized metric cards.
- **Don't** copy Jellyfin logo assets or imply official Jellyfin ownership.
- **Don't** use cyan as decoration when no action, focus, selection, or progress state is present.
- **Don't** use side-stripe accent borders, gradient text, or repeated identical card grids for app-owned UI.
- **Don't** let app chrome visually compete with embedded Jellyfin Web content.
- **Don't** write brochure copy in product dialogs. If the user opened About, Settings, or Update, answer that narrow intent and stop.
