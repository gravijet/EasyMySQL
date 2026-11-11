# Design System: EasyMySQL Website

A dark, cinematic page: drifting color clouds behind very large off-white type, and one 3D object that carries the page. It is short (about six screens) and nothing is pinned: the user asked on 2026-11-11 not to be forced to scroll for long, not to repeat the effects of their own sites one to one, for more real 3D, and for less text and fewer buttons.

## Colors

| Token | Value | Use |
|---|---|---|
| night | `#04060d` | ground behind the clouds |
| fg | `#f4f1ea` | type, copy button, "yes" dots of the other products |
| mute | `#b4bacb` | secondary text |
| dim | `#ffffff3d` | words that are not selected, "no" dashes, "not available" |
| blue | `#9dbcff` | EasyMySQL: headline ending, its numbers and dots, links |

Cloud colors change per section (two tones each, `FARBEN` in `app.js`): blue/violet at the top, magenta/blue for the comparison, one pair per app window, near black for the table, green/blue at the end.

## Type

Inter Tight for everything large, Instrument Sans for reading text, Martian Mono for the command.

## The 3D object (`gl.js`)

One WebGL fragment shader draws the clouds (layered noise, a few stars, grain) and three rounded discs by ray marching, lit with a highlight and a colored rim. `app.js` sets `window.easyScene` (place, size, and per disc x, y and half height); the shader eases toward it.

- **Top:** the discs are stacked as the database icon beside the headline, lean with the mouse, and jump apart for a moment when clicked.
- **Scrolling one screen:** the same three discs slide apart, the two right ones turn grey, and all three grow into the columns of the comparison. Name and value stand under each column; the labels are placed with the same projection as the shader.
- **Choosing a measurement** (eight plain words on the left; they also advance by themselves until the user picks one) lets the columns grow or shrink and the numbers count over. Where a product cannot do the task its column is a flat disc and grey words stand below.
- **End:** the stacked icon returns beside the closing sentence.

Without WebGL a static gradient is shown; with reduced motion only single frames are drawn.

## Other sections

- **Hero:** headline words tip up out of blur. Below: OS switch, the install command in a glass pill with the copy button, one small line with file links.
- **App windows:** six real screenshots lie as a slanted stack in space (CSS 3D). The chosen one folds out to the front; choose by pointing at or clicking one of the six large words, or by clicking a sheet. They advance by themselves until the user chooses.
- **Table:** plain rows, a dot for yes and a dash for no, large totals.

## Rules

- No header links, no install button in the header, no captions, no cards, icons or check circles. Every control on the page changes something that is shown.
- Nothing says "free" and nothing explains how the numbers were measured. Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded.
- Terminal installation is the main action; file downloads are a small line.
- CSP forbids inline styles: positions and custom properties are set from JavaScript.
- Both languages via `data-en`; word lists work with keyboard; visible focus and 44px targets stay intact.
