# Design System: EasyMySQL Website

The page is one stage: a dark, glossy liquid behind everything. At three moments large words rise out of it as an embossed relief, lit from the upper left. Between them the liquid stays calm and normal large type and real screenshots carry the page. About twelve screens long, free scrolling, nothing has to be clicked.

Direction from the user (2026-11-11, last rounds): unique background, no rounded 3D discs, nothing that must be clicked, no scroll bars, no standard underlined links, no mention of GitHub; then: a longer page, not over the top, still worth looking at; then: too overloaded, make it clean, smooth and clearly readable.

## The stage (`gl.js`)

One WebGL fragment shader. Height = slow liquid swells + blurred text (drawn to a 2D canvas with Inter Tight and uploaded as a texture) + ripples from the pointer. Normals from that height give colored reflections on edges facing the light, dark edges facing away, a thin-film tint and a light streak that drifts over the letters and follows the mouse. On a text change the old texture melts into the new one through a noise mask. `window.easyScene` (set by `app.js`) carries the text (`text`, counter `n`), the relief strength (`hoch`) and two reflection colors (`A`, `B`). Without WebGL: a static gradient and the headline as normal type. With reduced motion: single frames, no ripples.

## Sections

| Section | Relief in the liquid | On top (HTML) |
|---|---|---|
| Top | EasyMySQL | one sentence, OS words, the install command, file words |
| Six features | none, colors change per feature | large title, one line, the real screenshot (alternating sides) |
| One number | 0,47 s | one sentence with the other two values |
| Measurements | none | eight rows with three columns (EasyMySQL, XAMPP, Workbench), a thin bar under each value |
| Table | none | the feature table |
| End | Claude Code & Codex | one sentence and the footer |

The relief is attached to its section and scrolls with it (`easyScene.versatz`), so page text never lies on top of it; the liquid drifts along more slowly (`easyScene.lauf`). When the section leaves the middle of the screen the relief sinks back. Full highlights exist only at the relief: the open liquid stays dim so text on it is readable.

## Custom parts

- **Command:** the whole line is the button; a click copies it. A blue line sweeps along its top edge on hover.
- **OS choice:** three mono words, a small dot marks the chosen one.
- **Links:** mono words without underlines; file links carry an arrow that drops on hover.
- **Screenshots:** arrive slightly turned from below and straighten while scrolling in.
- **Numbers:** count up and their bars grow when they scroll into view.
- **Table marks:** a diamond that spins in for yes, a faint small ring for no; totals as large numbers.
- **Scrolling:** native scroll bars are hidden.

Removed as too much: the custom pointer, the slat images, automatic cycling, section snapping, the eight huge number blocks, bright liquid highlights behind text and the fixed header.

## Type and color

Inter Tight for the relief and the few larger lines, Martian Mono for all small labels. Off-white `#f4f1ea` text, `#a6c2ff` for EasyMySQL, ground `#03040a`.

## Rules

- Nothing says "free", nothing explains the measurements, no GitHub. Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded.
- Terminal installation is the main action; file downloads are small words.
- Relief text also exists in the document (class `ghost`) for screen readers and search.
- CSP forbids inline styles: custom properties and background images are set from JavaScript.
- Both languages via `data-en`; visible focus and 44px targets stay intact.
