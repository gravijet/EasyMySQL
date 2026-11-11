# Design System: EasyMySQL Website

Flat colors from the app icon, round shapes, soft motion in space. Written for people who are not experts: little text, the terminal command first.

## Colors

| Token | Value | Use |
|---|---|---|
| paper | `#f2f5fa` | page ground |
| card | `#fff` | cards, table, header |
| navy | `#101f4a` | text, terminal, the dark stage behind the app screenshots |
| navy-2 | `#1b2d66` | tiles and tab bar on the stage |
| blue | `#2b62c9` | EasyMySQL: headline ending, bars, figures, links |
| sky | `#a9c8f6` | secondary text on navy |
| sun | `#ffc531` | the one warm accent: copy button, selected tab, icons on navy, AI panel |
| green / red | `#17a058` `#e04848` | check and cross in the comparison |

No decorative gradients, glows, blur or hover lighting. The only gradient is the shading that makes the comparison bars look like round tubes.

## Type

Inter Tight for headings and figures, Instrument Sans for reading text, Martian Mono for the terminal.

## Structure

1. Headline, then the terminal with the install command (typed out, OS switch, copy button). File downloads are a small line below it.
2. Dark stage: feature tabs and the real app screenshots (dark UI on navy), then eight feature tiles.
3. Comparison: three statistics with rolling digits and round 3D bars, then the feature table with a count per product.
4. AI panel with a small chat example.

Nothing says "free": the app is open source and that is not advertised. No measurement method, GitHub or author links on the page; imprint and privacy stay in the footer.

## Motion (all off with reduced motion)

- Headline words tip up one after another; the terminal unfolds; the command is typed.
- The dark stage grows from a round inset card to full width while scrolling in (`clip-path`).
- The screenshot starts tilted and straightens with scroll, following the mouse slightly. All scroll and mouse values are eased frame by frame.
- Switching tabs: the yellow marker glides, screenshots are stacked and slide past each other in depth.
- Sections, tiles and chat bubbles rise from a slight backward tilt.
- Statistics: every digit is a drum with ten faces that rolls to its value; the tubes grow and the chart turns with scroll and mouse, rows at different depths.
- Check and cross marks flip in row by row.

## Rules

- Screenshots load eagerly; the first one is preloaded.
- CSP forbids inline styles: positions and custom properties are set from JavaScript, never with `style` attributes.
- Icons are Lucide (ISC), inlined as one SVG sprite.
- Both languages via `data-en`; keyboard tabs, visible focus and 44px targets stay intact.
