---
name: EasyMySQL Website
description: A restrained dark product page with real application evidence and clear installation controls.
colors:
  bg: "#0b0e14"
  surface: "#131820"
  fg: "#f1f4fa"
  mute: "#a5b0c0"
  line: "#29313e"
  blue: "#91bcff"
  action: "#2363db"
  ok: "#8ed4ae"
  control: "#212b39"
  selected-bg: "#243a59"
  selected-fg: "#d5e6ff"
  command-bg: "#171e29"
  copy-bg: "#283549"
typography:
  display:
    fontFamily: '"Instrument Sans", system-ui, sans-serif'
    fontSize: "clamp(2.9rem, 4.5vw, 4.4rem)"
    fontWeight: 600
    lineHeight: 1.07
    letterSpacing: "-0.04em"
  headline:
    fontFamily: '"Instrument Sans", system-ui, sans-serif'
    fontSize: "clamp(2rem, 3.5vw, 3rem)"
    fontWeight: 600
    lineHeight: 1.12
    letterSpacing: "-0.035em"
  title:
    fontFamily: '"Instrument Sans", system-ui, sans-serif'
    fontSize: "1.3rem"
    fontWeight: 600
    lineHeight: 1.35
    letterSpacing: "-0.02em"
  body:
    fontFamily: '"Instrument Sans", system-ui, sans-serif'
    fontSize: "1rem"
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily: '"Instrument Sans", system-ui, sans-serif'
    fontSize: "0.9rem"
    fontWeight: 600
  code:
    fontFamily: '"Martian Mono", ui-monospace, Consolas, monospace'
    fontSize: "0.75rem"
    fontWeight: 400
    lineHeight: 1.7
rounded:
  inset: "8px"
  control: "12px"
  surface: "16px"
  pill: "100px"
spacing:
  gutter: "clamp(1.25rem, 4.5vw, 4rem)"
  compact: "0.5rem"
  item: "1rem"
  group: "1.5rem"
  column: "2rem"
  wide: "3rem"
components:
  button-primary:
    backgroundColor: "{colors.action}"
    textColor: "#fff"
    rounded: "{rounded.control}"
    padding: "0.7rem 1.05rem"
    typography: "{typography.label}"
  button-secondary:
    backgroundColor: "{colors.control}"
    textColor: "{colors.fg}"
    rounded: "{rounded.control}"
    padding: "0.7rem 1.05rem"
  tab-selected:
    backgroundColor: "{colors.selected-bg}"
    textColor: "{colors.selected-fg}"
    rounded: "{rounded.pill}"
    padding: "0.6rem 1.1rem"
  feature-container:
    backgroundColor: "{colors.surface}"
    rounded: "{rounded.surface}"
  command:
    backgroundColor: "{colors.command-bg}"
    rounded: "{rounded.control}"
    padding: "0.5rem 0.5rem 0.5rem 1rem"
---

# Design System: EasyMySQL Website

## Overview

**Creative North Star: "The working database, in view"**

This descriptive name records the existing refinement direction; it is not a new visual-world selection. The website pairs a quiet dark blue canvas with actual application screenshots, readable sans typography and compact installation controls. Rounded surfaces soften a technical product without replacing its established identity.

**Key Characteristics:**
- Real application imagery provides the main visual evidence.
- Blue distinguishes actions, selection and useful numeric emphasis.
- Compact controls sit within spacious sections.

Source of truth: `public/style.css`, sampled against `public/index.html` and `public/app.js`. This system applies to the website only. No seed, generated comp or world-replacement workshop was used for this scoped refinement.

## Colors

The palette is a near-black navy foundation with cool readable text and restrained blue emphasis; frontmatter records the canonical values.

### Primary
- **Action blue** (`action`): filled primary download controls.
- **Light blue** (`blue`): headline emphasis, benchmark figures, brand mark and focus outline.
- **Selected blue** (`selected-bg`, `selected-fg`): selected feature and operating-system controls.

### Secondary
- **Status green** (`ok`): the small status dot beside the application preview caption.

### Neutral
- **Canvas** (`bg`), **raised surface** (`surface`): page and screenshot/container distinction.
- **Reading white** (`fg`), **muted text** (`mute`): hierarchy for primary and supporting content.
- **Divider** (`line`): comparison tables, fact rows and platform separators.
- **Control navy** (`control`), **command navy** (`command-bg`), **copy navy** (`copy-bg`): compact interactive and command surfaces.

**The Useful Blue Rule.** Use blue for actions, selected state and meaningful emphasis, as the page already does.

## Typography

**Display and body font:** self-hosted Instrument Sans, with system-ui and sans-serif fallbacks.
**Code font:** self-hosted Martian Mono, with ui-monospace, Consolas and monospace fallbacks.

The hierarchy uses modest weight differences and tighter headline tracking. Reading copy stays in sans; commands use mono. Frontmatter records the desktop hierarchy. Supporting text commonly uses 0.75–0.95rem; the lead uses 1.14rem and a 37ch measure. Ordinary descriptive blocks range around 31–58ch.

At widths up to 800px the display becomes `clamp(2.9rem, 8vw, 4rem)`; up to 420px it becomes `clamp(2.25rem, 10vw, 2.6rem)`. The lead reduces to 1.05rem on the smallest layout. Hero command text is 0.68rem. Benchmark figures use tabular numerals.

**The Reading and Commands Rule.** Keep explanatory prose in Instrument Sans and executable commands in Martian Mono.

## Layout

The centered container caps at 88rem with the fluid gutter token. Desktop hero columns use a 1:1.22 ratio and a 3rem gap; performance and integration sections use equal columns with 5rem gaps. Platform requirements form three columns. Section top spacing is 5.5rem, with major content blocks at 7rem.

At 1100px, the hero becomes equal columns and gaps tighten. At 800px, primary content, platform requirements and script options stack, navigation links disappear, and section spacing becomes 4.5rem. At 420px, the three direct-download controls form an equal-width grid with format labels beneath names. At 1500px the hero gap expands to 4rem. The comparison table preserves its 800px minimum width inside horizontal scrolling; the benchmark table overrides that minimum. Feature tabs scroll horizontally.

The current landing page places terminal installation before file downloads and the real screenshot; its composition belongs to this surface rather than every future page.

## Elevation & Depth

Tonal layering and rounded clipping provide most separation. Shadows are soft and limited: the hero media uses `0 24px 60px -12px #0009`; the transient toast uses `0 8px 25px #0006`. Ordinary feature containers and buttons remain flat.

The desktop hero preview rests at a slight negative rotation with a counter-rotated caption. Its arrival animation runs for 700ms with the source easing; mobile removes rotation and animation. Reduced-motion preferences disable transitions, animations and smooth scrolling.

## Shapes

Controls and command surfaces use the control radius; major media and table containers use the surface radius. Screenshot insets and copy buttons use the smaller inset radius. Feature tabs, OS selectors and the header download link are pills. The language control and status dot are circular. Borders are thin separators, not heavy outlines around every region.

## Components

### Buttons

Download links are compact rounded filled controls, with a minimum height of 50px. The primary uses action blue; secondary downloads use control navy. Hover lightens the fill and moves downloads up 2px over 180ms with the source easing. Unavailable downloads use a muted fill/text treatment and no movement. All interactive elements inherit a light-blue 2px focus outline with a 5px offset. At small widths hero downloads are at least 58px high.

### Selection controls

Feature tabs are pill buttons with a selected tonal fill, at least 46px high. The OS selector uses the same selected colors and at least 44px targets. Source JS maintains selected states, roving tab focus and keyboard navigation; these controls are functional selection, not decorative tags.

### Media containers

Actual screenshots sit inside softly clipped dark containers. Feature captions have 1.5rem by 2rem padding on desktop; at 800px they stack with 1.3rem padding. Screenshots keep their natural aspect ratio.

### Commands and copy

Command rows pair wrapping monospace text with a compact copy button of at least 44px height. The copy button has an inset radius and lightens on hover. OS selection changes both command and shell label. Copy feedback appears in a temporary rounded toast.

### Navigation and data

The header is a simple brand, inline links, language switch and download link. Navigation links are muted at rest and brighten on hover, with at least 44px targets. Comparison tables use subtle row dividers and a toned EasyMySQL column. Benchmarks pair descriptive row labels with large right-aligned blue values and a native details disclosure for methodology. There is no text-input primitive on this page.

## Do's and Don'ts

### Do:
- **Do** use real application screenshots with truthful captions.
- **Do** preserve the existing dark palette, locally hosted fonts and bilingual layouts.
- **Do** retain visible keyboard focus, selection states and reduced-motion behavior.
- **Do** use rounded controls and measured section spacing from the shipped CSS.

### Don't:
- **Don't** replace the established website identity during routine extensions.
- **Don't** add decorative filler or an oversized script demonstration.
- **Don't** turn this landing page's installation sequence into a mandatory composition for unrelated surfaces.

Not canonized: no craft-floor defect was identified in the sampled components; the small hero-command typography remains a local implementation detail, not a general body-text rule. No UI was repaired by this documentation pass.
