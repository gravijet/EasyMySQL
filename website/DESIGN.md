# Design System: EasyMySQL Website

Light, squared, left-aligned. The measured comparison is the one loud element; everything else stays quiet.

## Colors

| Token | Value | Use |
|---|---|---|
| paper | `#f3f5fa` | page ground |
| sheet | `#fff` | install panel, comparison band |
| ink | `#131a2b` | text, command rows, strong rules |
| mute | `#566176` | secondary text |
| line | `#d3d9e5` | fine rules |
| blue | `#2450e0` | EasyMySQL bars and figures, links, primary download |
| rival | `#8f99ad` | hatched bars of the other products |
| ok | `#107a44` | check marks, safety facts |

Blue always means EasyMySQL or an action. Other products are hatched grey, so the charts do not rely on color alone.

## Type

Inter Tight for headings and figures, Instrument Sans for reading text, Martian Mono only for commands. Body 17px; nothing below 0.85rem except the mono version label.

## Layout

1240px wrap, sticky 60px header. Hero: headline and three key figures left, install panel right. Then the feature list (vertical tabs with their description) beside one real screenshot, the comparison band (three bar charts, feature table, collapsed method), the AI section and systems/formats. Below 900px everything stacks; the feature tabs become a horizontal scroller.

## Rules

- No radius, no pills, no cards. Structure comes from rules and the grid.
- Bars are inline SVG `rect` widths in percent of the largest value per chart (CSP forbids inline styles). They grow once when the charts scroll into view; reduced motion shows them directly.
- Screenshots are real app views and open full size.
- Both languages via `data-en`; keyboard tabs, visible focus and 44px targets stay intact.
