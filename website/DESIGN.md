# Design System: EasyMySQL Website

The page is one stage: a dark, calm liquid behind everything. Only at the top the word EasyMySQL rises out of it as an embossed relief. Everything else is normal type at moderate sizes, real screenshots and one comparison table. About nine screens long, free scrolling, nothing has to be clicked.

Direction from the user (2026-11-11, last rounds): unique background, no rounded 3D discs, nothing that must be clicked, no scroll bars, no standard underlined links, no mention of GitHub; a longer page, not over the top; clean, smooth, clearly readable; then: measurements belong in one table together with the feature comparison, real check marks that look the same everywhere, no giant standalone text sections (the "0,47 s" and "Claude Code & Codex" reliefs were removed, the AI brands need not be named), more information and content; then: no filler content (their example: mentioning Ctrl+Z), plain human wording instead of slogans, clearer numbers in the table, no version number in the footer; then: remove the standard landing-page elements too (their example: "in 3 Schritten"); then: sharper hero wordmark, a bigger download area without arrows and dots, text selection must not flip the page to white, always high contrast, a language switch that shows both DE and EN with a smooth animated change, and a section on why to switch.

## The stage (`gl.js`)

One WebGL fragment shader. Height = slow liquid swells + blurred text (drawn to a 2D canvas with Inter Tight and uploaded as a texture) + ripples from the pointer. `window.easyScene` (set by `app.js`) carries the text (`text`, counter `n`), the relief strength (`hoch`), its scroll offset (`versatz`), the scrolled distance (`lauf`) and two reflection colors (`A`, `B`). The relief is attached to the hero and scrolls away with it; the liquid drifts along more slowly. Full highlights exist only at the relief: away from it the brightness is capped at about 20%, so light text always has high contrast. The canvas renders at screen resolution (up to about 2.4 million pixels) and steps down by itself if the device drops below about 22 frames per second. The hero's top padding keeps its text below the relief. Without WebGL: a static gradient and the headline as normal type. With reduced motion: single frames, no ripples.

## Sections

| Section | Content |
|---|---|
| Top | relief EasyMySQL (about 70% of the width), one sentence, OS words, the install command, file words |
| Intro | the question "What is EasyMySQL?" and one paragraph |
| Six features | title, two or three plain sentences, the real screenshot (alternating sides); liquid color changes per feature |
| Why switch? | heading left, five reasons as plain sentences right, separated by hairlines |
| Comparison | one left-aligned table: speed, size, then features by group; EasyMySQL column tinted; sticky header |
| Switching from XAMPP | heading left, one paragraph right (same layout as the intro): moving databases, connection settings, PHP |
| Footer | a link back up to the installation, imprint, privacy |

## Custom parts

- **Switches (language, OS):** both options are always written out (DE EN; Windows macOS Linux). A light block glides under the chosen word; its leading edge moves faster than the trailing one, so it stretches briefly (`gleite` in `app.js`, `--l`/`--r`).
- **Language change:** where the browser supports view transitions, the old page dissolves softly into the new one while the switch itself glides.
- **Command:** a large dark panel; a click anywhere copies. The light "Kopieren" block on the right shows a check that draws itself and "Kopiert". No arrows, dots or other glyph decorations anywhere.
- **Links:** normal words with a fine line underneath that brightens on hover.
- **Screenshots:** arrive slightly turned from below and straighten while scrolling in.
- **Table marks:** one green check and one red cross (inline SVG symbols `#ok`, `#no`), identical in every column; they draw themselves when their row appears. Numbers are large with a small unit, count up, and a bar across the cell grows under each.
- **Scrolling:** native scroll bars are hidden.

Removed as too much: the custom pointer, the slat images, automatic cycling, section snapping, the huge number blocks, the relief sections for a number and for the AI tools, bright liquid highlights behind text and the fixed header. Headings stay below about 3rem.

## Type and color

Inter Tight for the relief and the few larger lines, Martian Mono for all small labels. Off-white `#f4f1ea` text, `#a6c2ff` for EasyMySQL, ground `#03040a`.

## Rules

- No template sections: no step lists, feature grids, FAQ blocks or closing call-to-action. Information that matters goes into the prose blocks or the table.
- Copy: plain sentences as you would explain it to a classmate. No slogans, no three-word staccato, no keyboard shortcuts or minor details. Every entry must matter to someone deciding whether to install.

- Nothing says "free", nothing explains the measurements, no GitHub. Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded.
- Terminal installation is the main action; file downloads are small words.
- Relief text also exists in the document (class `ghost`) for screen readers and search.
- CSP forbids inline styles: custom properties and background images are set from JavaScript.
- Both languages via `data-en`; visible focus and 44px targets stay intact.
