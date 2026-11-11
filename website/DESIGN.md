# Design System: EasyMySQL Website

The page is one stage: a dark, calm liquid behind everything. Only at the top the word EasyMySQL rises out of it as an embossed relief. Everything else is normal type at moderate sizes, real screenshots and one comparison table. About twelve screens long, free scrolling, nothing has to be clicked.

Direction from the user (2026-11-11, last rounds): unique background, no rounded 3D discs, nothing that must be clicked, no scroll bars, no standard underlined links, no mention of GitHub; a longer page, not over the top; clean, smooth, clearly readable; then: measurements belong in one table together with the feature comparison, real check marks that look the same everywhere, no giant standalone text sections (the "0,47 s" and "Claude Code & Codex" reliefs were removed, the AI brands need not be named), more information and content.

## The stage (`gl.js`)

One WebGL fragment shader. Height = slow liquid swells + blurred text (drawn to a 2D canvas with Inter Tight and uploaded as a texture) + ripples from the pointer. `window.easyScene` (set by `app.js`) carries the text (`text`, counter `n`), the relief strength (`hoch`), its scroll offset (`versatz`), the scrolled distance (`lauf`) and two reflection colors (`A`, `B`). The relief is attached to the hero and scrolls away with it; the liquid drifts along more slowly. Full highlights exist only at the relief: the open liquid stays dim so text on it is readable. Without WebGL: a static gradient and the headline as normal type. With reduced motion: single frames, no ripples.

## Sections

| Section | Content |
|---|---|
| Top | relief EasyMySQL (about 70% of the width), one sentence, OS words, the install command, file words |
| Intro | one heading, one paragraph, four facts |
| Six features | title, one line, three points, the real screenshot (alternating sides); liquid color changes per feature |
| Also included | twelve short entries in three columns |
| Comparison | one table: speed, size, then features by group; EasyMySQL column tinted; sticky header |
| Three steps | numbered because it is a real sequence |
| Good to know | six questions with answers, all open |
| End | the install command again and the footer |

## Custom parts

- **Command:** the whole line is the button; a click copies it. A blue line sweeps along its top edge on hover. It appears at the top and at the end (`data-cmd`, `data-prompt`).
- **OS choice:** three mono words, a small dot marks the chosen one.
- **Links:** mono words without underlines; file links carry an arrow that moves on hover.
- **Screenshots:** arrive slightly turned from below and straighten while scrolling in.
- **Table marks:** one green check and one red cross (inline SVG symbols `#ok`, `#no`), identical in every column; they draw themselves when their row appears. Numbers count up and thin bars grow.
- **Scrolling:** native scroll bars are hidden.

Removed as too much: the custom pointer, the slat images, automatic cycling, section snapping, the huge number blocks, the relief sections for a number and for the AI tools, bright liquid highlights behind text and the fixed header. Headings stay below about 3rem.

## Type and color

Inter Tight for the relief and the few larger lines, Martian Mono for all small labels. Off-white `#f4f1ea` text, `#a6c2ff` for EasyMySQL, ground `#03040a`.

## Rules

- Nothing says "free", nothing explains the measurements, no GitHub. Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded.
- Terminal installation is the main action; file downloads are small words.
- Relief text also exists in the document (class `ghost`) for screen readers and search.
- CSP forbids inline styles: custom properties and background images are set from JavaScript.
- Both languages via `data-en`; visible focus and 44px targets stay intact.
