# Design System: EasyMySQL Website

Light and airy: white round cards on a pale ground, one strong blue, colorful icons. Depth comes from real 3D scenes instead of glows and color gradients. Written for people who are not experts: little text, the terminal command first.

## Colors

| Token | Value | Use |
|---|---|---|
| bg | `#f5f6fb` | page ground |
| card | `#fff` | cards, table, header, pill bars |
| ink | `#0f1730` | text, terminal, AI panel |
| mute | `#5a637c` | secondary text |
| blue | `#2a5ee0` | EasyMySQL: headline ending, selected pill, copy button, its puck and cylinder |
| green / red | `#12a150` `#e5484d` | check and cross, stopped pucks |
| orange / amber / teal | `#f07c12` `#d99a00` `#0d9db8` | icon tiles and statistic cards |

No decorative gradients, light clouds, hover lighting or shine. The only gradient is the shading that makes the cylinders round. Other products are always grey.

## Type

Inter Tight for headings and figures, Instrument Sans for reading text, Martian Mono for the terminal.

## Structure

1. Headline, then the terminal with the install command (typed out, OS switch, copy button with a pressable edge). File downloads are one small line below.
2. Features: pill tabs and a 3D carousel of the six real app screenshots, then eight icon tiles.
3. Comparison: a race on a tilted board (five measurements to choose), three cards with rolling digits and cylinders for memory, disk and download size, then the feature table with a count per product.
4. AI panel.

Nothing says "free" and nothing explains how the numbers were measured. Imprint and privacy stay in the footer.

## Motion (all off with reduced motion)

- Headline words tip up one after another; the terminal unfolds; the command is typed.
- Carousel: the chosen window is in front, the others stand turned to the sides in depth; clicking a tab or a side window turns it. The whole carousel starts tilted, straightens with scroll and follows the mouse.
- Race: three lanes, each puck is a stack of layers standing on the board. Pucks travel in proportion to the measured times while the seconds count up; where a product cannot do the task, its puck stops red.
- Statistics: every digit is a drum with ten faces that rolls to its value; cylinders grow.
- Sections and tiles rise from a slight backward tilt; tiles lean toward the mouse and lift their icon.
- Check and cross marks flip in row by row; pill markers glide with a spring; the header narrows after scrolling.

## Rules

- Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded for reading.
- Screenshots load eagerly; the first one is preloaded.
- CSP forbids inline styles: positions and custom properties are set from JavaScript, never with `style` attributes.
- Icons are Lucide (ISC), inlined as one SVG sprite.
- Both languages via `data-en`; keyboard tabs, visible focus and 44px targets stay intact.
