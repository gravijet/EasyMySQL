# Design System: EasyMySQL Website

Plain and large. White ground, very big type, one blue, thin rules and thin bars. The user chose this direction on 2026-11-11 after rejecting cards, icon tiles, pill tabs, check-mark circles, gradients and 3D charts as generic.

## Colors

| Token | Value | Use |
|---|---|---|
| bg | `#fff` | page |
| ink | `#0c1226` | text, strong rules, command line, "yes" dots of the other products |
| mute | `#5d6579` | secondary text |
| soft | `#c9cedb` | unselected tabs, bars of the other products, "no" dashes, "not available" |
| line | `#e6e8ef` | fine rules |
| blue | `#2456e6` | EasyMySQL only: headline ending, its numbers, bars and dots, copy button, links |

## Type

Inter Tight for headings and numbers (headline up to 9.4rem, EasyMySQL numbers up to 7.6rem), Instrument Sans for reading text at 18px, Martian Mono for the command.

## Structure

Left-aligned, 1248px wide, large gaps between sections.

1. Plain header (name, three links, language).
2. Headline, OS switch as underlined words, the install command on one dark line with a copy button, file downloads as small links.
3. Features: word tabs with a sliding blue line, one real screenshot, eight short text entries under a rule.
4. Comparison: eight measurements in two columns. Each shows the EasyMySQL value very large in blue with a thicker bar, the other two smaller with thin grey bars, or grey words where a product cannot do it.
5. Feature table: text rows, a filled dot for yes and a thin dash for no, totals as large numbers.
6. AI: one heading and one sentence.

No cards, shadows on boxes, icons, pills or gradients. The only rounded things are the command line, the copy button and the screenshot.

## Motion (off with reduced motion)

Only fading in with a small rise, numbers counting up, bars growing, the tab line sliding, screenshots cross-fading, and the screenshot straightening slightly while it scrolls in.

## Rules

- Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded for reading. Nothing on the page says "free" or explains the measurement.
- Screenshots load eagerly; the first one is preloaded.
- CSP forbids inline styles: positions and custom properties are set from JavaScript.
- Both languages via `data-en`; keyboard tabs, visible focus and 44px targets stay intact.
