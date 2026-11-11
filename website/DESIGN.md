# Design System: EasyMySQL Website

Light, round and colorful, written for pupils, students and teachers. Little text, large type, real screenshots, lots of smooth motion.

## Colors

| Token | Value | Use |
|---|---|---|
| bg | `#f5f6fc` | page ground |
| card | `#fff` | cards, table, tab bar |
| ink | `#0e1530` | text, dark AI panel, command row |
| mute | `#5b6480` | secondary text |
| blue / violet / pink | `#2f5bff` `#7a4dff` `#e6468c` | brand gradient: main button, headline ending, tab glider, progress bar |
| green / red | `#12a150` `#e5484d` | check and cross in the comparison |
| cyan / orange | `#0fa9cf` `#f07c12` | extra tile colors |

Each feature tile and each statistic has its own color (`.c-…` sets `--c`). In the comparison, EasyMySQL bars use that color and the others are grey.

## Type

Inter Tight for headings and figures, Instrument Sans for reading text, Martian Mono only for the terminal command.

## Shapes

Cards 28px radius, buttons and tabs fully round, screenshots in a rounded window with a colored glow.

## Icons

Lucide (ISC license), inlined as one SVG sprite at the top of `index.html` and used with `<use href="#i-name">`. Yes/no marks are filled circles built from the Lucide check and x paths.

## Motion (all off with reduced motion)

- Headline words rise in one after another; pill, lead, button and screenshot follow.
- Soft color clouds drift behind the hero and the AI panel.
- The hero screenshot starts tilted, straightens while scrolling and follows the mouse slightly.
- Sections, tiles and statistics fade, rise and sharpen when they scroll into view; siblings are staggered.
- Statistic numbers count up and bars grow; table marks pop in row by row.
- The tab highlight glides with a spring; screenshots swap with a blur-scale.
- Tiles and the AI panel have a light spot following the mouse; the main button is slightly magnetic and has a moving shine.
- A gradient bar at the top shows scroll progress; the floating header narrows after scrolling.
- Formats run as a ticker.

## Rules

- No technical clutter on the page: no measurement method, script or raw-data links, no GitHub or author links. Imprint and privacy stay in the footer.
- The comparison table lists what EasyMySQL can do and also rows the others win, so the count stays honest.
- CSP forbids inline styles: widths and positions are set from JavaScript (CSSOM), never with `style` attributes.
- Both languages via `data-en`; keyboard tabs, visible focus and 44px targets stay intact.
