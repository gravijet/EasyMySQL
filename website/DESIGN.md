# Design System: EasyMySQL Website

The page is one stage: a dark, glossy liquid. The large words and numbers are not set in HTML type; they rise out of the liquid as an embossed relief, lit from the upper left, and melt into each other when the content changes. Everything on top of it is small, plain and custom. About five screens long; nothing has to be clicked.

Direction from the user (2026-11-11, last round): the background must not look like their other sites, no rounded 3D discs or cylinders, nothing that must be clicked to see content, no scroll bars, no standard underlined links, no mention of GitHub, and no element that looks like stock HTML and CSS.

## The stage (`gl.js`)

One WebGL fragment shader. Height = slow liquid swells + blurred text (drawn to a 2D canvas with Inter Tight and uploaded as a texture) + ripples from the pointer. Normals from that height give colored reflections on edges facing the light, dark edges facing away, a thin-film tint and a light streak that drifts over the letters and follows the mouse. On a text change the old texture melts into the new one through a noise mask. `window.easyScene` (set by `app.js`) carries the text (`text`, counter `n`), the relief strength (`hoch`) and two reflection colors (`A`, `B`). Without WebGL: a static gradient and the headline as normal type. With reduced motion: single frames, no ripples.

## What stands in the relief

| Section | Relief | On top (HTML) |
|---|---|---|
| Top | EasyMySQL | one sentence, OS words, the install command, file words |
| Comparison | EasyMySQL's value for the current measurement | measurement name, three thin bars with XAMPP and Workbench values |
| App windows | name of the current feature | the screenshot |
| Table | nothing, the liquid goes calm | the feature table |
| End | Claude Code & Codex | "sind eingebaut." and the footer |

Comparison and app windows advance by themselves every 3.6 s while they are on screen. Reflection colors change per section and per feature (`FARBEN`, `FUNKTIONEN` in `app.js`).

## Custom parts

- **Pointer:** a ring that follows the mouse with a delay; over the command it grows into a white pill that says "kopieren". Fine pointers only.
- **Command:** the whole line is the button; a click copies it. A blue line sweeps along its top edge on hover. No boxed button.
- **OS choice:** three mono words, a small dot marks the chosen one.
- **Links:** mono words without underlines; file links carry an arrow that drops on hover.
- **Screenshot:** built from 14 vertical slats that turn in 3D one after another to reveal the next image; the whole board leans toward the mouse.
- **Table marks:** a diamond that spins in for yes, a faint small ring for no; totals as large numbers.
- **Scrolling:** native scroll bars are hidden; sections snap softly.

## Type and color

Inter Tight for the relief and the few larger lines, Martian Mono for all small labels. Off-white `#f4f1ea` text, `#a6c2ff` for EasyMySQL, ground `#03040a`.

## Rules

- Nothing says "free", nothing explains the measurements, no GitHub. Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded.
- Terminal installation is the main action; file downloads are small words.
- Relief text also exists in the document (class `ghost`) for screen readers and search.
- CSP forbids inline styles: custom properties and background images are set from JavaScript.
- Both languages via `data-en`; visible focus and 44px targets stay intact.
