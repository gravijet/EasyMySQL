---
name: EasyMySQL Website
description: A WinMorph-derived full-viewport product scene that becomes a pinned
  application gallery.
colors:
  bg: '#07080c'
  fg: '#f4f1ea'
  mute: '#b2b6c1'
  line: '#343945'
  blue: '#a7c3ff'
  scene: '#537bdf'
  scene-data: '#337e7c'
  scene-er: '#9a6e39'
  scene-builder: '#745ea8'
  scene-code: '#467a55'
  scene-git: '#446682'
  scrollbar: '#6d788c'
  nav-solid: '#07080cf0'
  primary-hover: '#fff'
  veil: '#000b'
  text-shadow: '#0008'
  screen: '#171c23'
typography:
  display:
    fontFamily: '"Inter Tight", system-ui, sans-serif'
    fontSize: clamp(4rem, 12.3vw, 11rem)
    fontWeight: 750
    lineHeight: 0.9
    letterSpacing: -0.04em
  headline:
    fontFamily: '"Inter Tight", system-ui, sans-serif'
    fontSize: clamp(2.6rem, 6vw, 6rem)
    fontWeight: 700
    lineHeight: 1
    letterSpacing: -0.04em
  body:
    fontFamily: '"Inter Tight", system-ui, sans-serif'
    fontSize: 17px
    fontWeight: 400
    lineHeight: 1.5
  lead:
    fontFamily: '"Inter Tight", system-ui, sans-serif'
    fontSize: clamp(1.05rem, 1.8vw, 1.5rem)
    lineHeight: 1.35
  label:
    fontFamily: '"Inter Tight", system-ui, sans-serif'
    fontSize: 0.88rem
    fontWeight: 600
  gallery-note:
    fontFamily: '"Inter Tight", system-ui, sans-serif'
    fontSize: 0.75rem
  code:
    fontFamily: '"Martian Mono", Consolas, monospace'
    fontSize: 0.75rem
    fontWeight: 400
    lineHeight: 1.7
spacing:
  gutter: clamp(20px, 4vw, 64px)
  small: 1rem
  group: 1.5rem
  column: 2rem
  wide: 4rem
  section: clamp(72px, 10svh, 120px)
components:
  button-primary:
    backgroundColor: '{colors.fg}'
    textColor: '{colors.bg}'
    padding: 0.5rem 0.95rem
  button-primary-hover:
    backgroundColor: '{colors.primary-hover}'
    textColor: '{colors.bg}'
  download-link:
    textColor: '{colors.blue}'
  tab-selected:
    textColor: '{colors.fg}'
    padding: 15px 10px 6px 0
  command:
    textColor: '{colors.fg}'
    padding: 0.25rem 0 0.5rem
  comparison:
    backgroundColor: '{colors.bg}'
    textColor: '{colors.fg}'
---

# Design System: EasyMySQL Website

## Overview

**Creative North Star: "The application becomes the stage"**

The actual WinMorph template supplies the visual structure: an application screenshot fills the viewport behind a centered name and immediately usable installation controls. Scrolling reduces that image to a readable screen, then advances six real application views in the same pinned stage. Quiet moving color fields connect the states; comparison and MCP continue on the same dark ground.

This records the implemented adaptation of the user's named WinMorph template, not a newly proposed metaphor or an approved comp. Scope is the website; native application UI has its own rules. The earlier static dark/light comparison system is superseded. Inter Tight and the radial color fields are explicitly scoped, user-directed template choices supported by the source, not general defaults for unrelated surfaces.

**Key Characteristics:**
- Full-viewport application imagery becomes a contained screen through scroll.
- Terminal and file installation share the first viewport.
- Six features share one pinned stage and progress rail.
- Dark surfaces, square controls and restrained template-derived color fields.

## Colors

Frontmatter records all actual CSS and scene colors, including translucent functional values. There is no light comparison palette.

### Primary
- **Link blue** (`blue`): links, OS selection and selected progress-rail base; also selection background.
- **Feature hues** (`scene`, `scene-data`, `scene-er`, `scene-builder`, `scene-code`, `scene-git`): the six sequential radial-field hues in `public/scene.js`. They provide background continuity rather than action semantics.

### Neutral
- **Stage** (`bg`), **warm foreground** (`fg`), **muted copy** (`mute`) and **fine rule** (`line`): all content, comparison and MCP surfaces.
- **Solid navigation** (`nav-solid`): translucent dark header after scrolling.
- **Screen backing** (`screen`): behind actual app images.
- **Veil** (`veil`) and **text shadow** (`text-shadow`): foreground readability over the full-viewport screenshot; the veil color also supplies the screen shadow.
- **Primary hover** (`primary-hover`): brighter light download fill.
- **Scrollbar** (`scrollbar`): page, rail and comparison overflow.

## Typography

**Display and body:** self-hosted Inter Tight, with system-ui and sans-serif fallbacks. **Commands:** self-hosted Martian Mono, with Consolas and monospace fallbacks. The font files expose variable weights; frontmatter records the roles actually used.

The single product-name heading is heavy and tightly tracked. Supporting copy is centered at up to 52ch. Installation labels stay compact. Runtime figures use tabular numerals at 1.55rem; supporting text commonly uses 0.78-0.97rem. The one gallery sample-data label is 0.75rem (12px at the default root size), a local annotation rather than a general reading-text size.

At widths up to 600px the name uses 14.8vw and lead copy becomes 1.05rem with a 32ch measure. In short landscape layouts up to 650px high the name uses `clamp(3.5rem, 9vw, 6rem)`. Hero commands use 0.78rem and wrap. Pinned gallery captions use `clamp(1.1rem, 2.1vw, 1.8rem)`; up to 760px they use `clamp(1.25rem, 4.7vw, 1.8rem)` and a 31ch measure. Runtime figures reduce to 1.12rem below 800px and 1.02rem below 600px.

**The Reading and Commands Rule.** Use Inter Tight for reading and Martian Mono for executable commands.

## Layout

A fixed header is 68px tall. Normal content caps at 1440px with the fluid gutter token. The centered installation grid is at most 1040px wide, with 1.25:1 columns and a 4rem gap. It stacks below 800px, where its maximum width is 540px. Below 600px navigation and installation spacing tighten without a second installation region.

With JavaScript, the gallery scene is `calc(100svh + 6 * 52svh)` high and holds a 100svh sticky stage. Its first 65vh of scroll transforms a viewport-covering image into an 8:5 screen. The final desktop screen width is `min(88vw, calc((100svh - 300px) * 1.6))`; up to 760px it becomes 92vw. Captions sit beneath the fixed header and the progress rail sits near the bottom. A single sample-data annotation sits below the rail.

The remaining scroll range advances the six feature states. Gallery links open full-size images. Requirements, numerical/feature comparison and MCP follow in normal flow. Major sections use the fluid section-spacing token. Comparison remains dark: desktop uses a four-column table with a 760px minimum width; below 800px each row label spans the full width above three equal app columns. One sticky app-heading row sits below the fixed header. Numeric units stay together. MCP stacks below 800px; its two-column fact list becomes one column below 600px.

At heights up to 650px with a landscape aspect ratio the pinned composition compresses. At heights up to 500px and an aspect ratio of at least 1:1, pinning is removed and the installer, rail, images and captions flow normally. Without JavaScript the page also uses normal flow, retaining the initial screenshot, feature structure and download links.

## Elevation & Depth

Depth comes from the actual application image: it begins behind a dark contrast veil with a 3px backdrop blur, then shrinks toward the reader as the veil clears. The screen uses `0 50px 120px -30px #000b`; the installation overlay uses `0 2px 20px #0008` text shadow. These soft template effects do not establish card elevation elsewhere.

The three template color fields are radial gradients ending in transparency at 62%. Their opacities are 0.23, 0.2 and 0.1; they drift at 38s, 46s and 30s with staggered negative delays. Feature hue transitions take 1.4s. This radial-field treatment is a deliberate exception from the user's actual WinMorph template, not a license for unrelated ornamental glows.

Overlay arrival lasts 1.2s, with the installation delayed 0.12s. A successfully changed image reveals with a 12% right inset and opacity 0.65 to 1 over 520ms. The easing is `cubic-bezier(0.19, 1, 0.22, 1)`. Pointer movement can tilt the contained screen; scroll drives scale and progress, with no timed slideshow. The fading installation becomes inert to prevent focus on hidden controls.

Reduced motion stops ambient drift, transitions, entrance and wipe effects. The full-stage composition remains, with a direct cut between cover and contained-screen states instead of interpolated zoom. Pointer tilt is disabled. Short landscape uses the separate normal-flow fallback regardless of motion preference.

**The Shared Stage Rule.** Keep scroll, keyboard and tab selection connected to the same decoded screenshot and caption.

## Shapes

There is no radius scale: controls, media and structural regions are square. Fine rules organize tables and commands. OS selection uses a two-pixel bottom border. Progress-rail tracks and fills are two pixels high above each label. The SVG database mark keeps its native rounded line caps; the no-pill direction concerns containers, not icon stroke geometry.

## Components

### Downloads and commands

The primary download is a light rectangle with dark text, 0.5rem by 0.95rem padding and a 44px minimum height; hover brightens the fill. Other files are open links. Missing packages become muted, transparent, unpadded links with unavailable status; API failure exposes the release fallback. Copy is an underlined blue text button with a 44px target. Command rows wrap above a fine divider. Clipboard feedback uses a fixed light rectangle with dark text, a status role and a 2200ms dismissal.

### Gallery progress rail

Six open buttons share the stage, with muted idle labels, light selected labels, a blue selected base track and a foreground progress fill driven by `--p`. Each has at least 48px height and 15px top padding to separate its label from the track. At narrow widths the rail scrolls horizontally. Arrow keys, Home and End use roving tab stops and ARIA selection. Direct selection updates scroll position and hue; scroll selection updates the same content.

A requested image is decoded before selected state, caption and visibility change. A newer request supersedes an older pending decode; failure leaves the previous view and announces an error. The actual `img` receives the wipe. In the pinned stage `picture` uses `display: contents` to avoid the observed browser paint issue while its image is positioned inside the screen. Normal-flow fallback restores natural image sizing. Preserve this interaction rather than treating the gallery as independent cards.

### Navigation, focus and data

The fixed header carries the database mark, version/license, two section links and language switch; GitHub is a footer link. It gains a translucent solid background after 80px of scrolling. Links have 44px minimum targets. The skip link becomes visible on focus. Global focus is a 2px foreground outline with a 4px offset. Language switching updates page copy, accessible labels, metadata and release/error text.

Comparison uses muted row labels, thin rules, blue EasyMySQL values and larger measured figures. Native details expose method and raw-data links. Preserve exact tested versions and the differing readiness-signal explanation from PRODUCT.md and `public/benchmarks.json`. The page has no input, chip or card primitive.

## Do's and Don'ts

### Do:
- **Do** preserve the actual template structure, first-viewport installation and six-state gallery.
- **Do** decode a requested screenshot before changing its selected tab and caption.
- **Do** retain bilingual copy, keyboard navigation, visible focus and reduced-motion behavior.
- **Do** keep runtime units together and expose exact tested versions and methodology.

### Don't:
- **Don't** add rounded cards, pill containers or repeated installation areas.
- **Don't** replace real app imagery with an unrelated decorative background.
- **Don't** turn the scroll gallery into an automatic slideshow.
- **Don't** extend the template color-field exception into arbitrary gradients or glows.

Not canonized: no new craft-floor defect identified in the sampled components. Template-specific Inter Tight and radial fields are user-directed exceptions; small gallery annotation text remains local. This documentation pass repairs no UI.
