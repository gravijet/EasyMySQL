# Design System: EasyMySQL Website

A dark, cinematic page in the family of the user's own sites (benjaminberger.at, winmorph.benjaminberger.at, lifebudget.me): drifting color clouds behind very large off-white type, a real 3D object and scenes that stay pinned while scrolling transforms them. Its own ideas: the database icon as a 3D object that falls apart into its discs, and measurements that run like a stopwatch as you scroll.

## Colors

| Token | Value | Use |
|---|---|---|
| night | `#04060d` | ground behind the clouds |
| fg | `#f4f1ea` | type, rail fill, copy button, "yes" dots of the other products |
| mute | `#b4bacb` | secondary text |
| dim | `#ffffff3d` | unselected rail labels, "no" dashes, words not yet lit |
| blue | `#9dbcff` | EasyMySQL: headline ending, its numbers, bar and dots, links |

Cloud colors change per section and per feature (two tones each, set in `app.js` under `FARBEN`): blue/violet at the top, then blue, teal, amber, violet, green and slate for the six features, magenta/blue for the measurements, near black for the table, green/blue for the AI sentence.

## Type

Inter Tight for everything large (headline up to 9rem, scene titles up to 8rem, EasyMySQL numbers up to 10.5rem), Instrument Sans for reading text, Martian Mono for the command.

## Background and 3D object (`gl.js`)

One WebGL fragment shader draws the clouds (layered noise, a few stars, grain) and the database icon: three rounded discs, ray-marched, lit with a highlight and a colored rim. `window.easyScene` tells it where the object stands, how far the discs are apart and which colors the clouds have; the shader eases toward those values. Without WebGL a static gradient is used. With reduced motion it draws single frames only.

## Scenes

1. **Hero**: headline words tip up out of blur; the 3D icon floats beside it and follows the mouse. Below: OS switch, the install command in a glass pill with the copy button, file links.
2. **Features (pinned, 6 steps)**: scrolling away from the hero pulls the discs apart and fades them. The app window rises tilted from below and straightens. Each step swaps the huge title (tipping away upward), the caption and the screenshot (turning in from the side in depth) and changes the cloud colors. A rail at the bottom fills with scroll and can be clicked.
3. **Measurements (pinned, 8 steps)**: for each measurement the time runs with the scroll: all three bars start together and each stops at its own value, so EasyMySQL stops first. Where a product cannot do the task, grey words stand instead. Sizes grow together.
4. **Table**: plain rows, a dot for yes and a dash for no, large totals.
5. **AI**: one long sentence lights up word by word while the 3D icon returns and its discs close again.

## Rules

- No cards, icons, pills-as-tabs or check circles. Only the command pill, the header button and the screenshots are rounded.
- Nothing says "free" and nothing explains how the numbers were measured. Numbers come from `/home/benj/client-benchmark-2026-11` (2026-11-11), rounded.
- Terminal installation is the main action; file downloads are a small line.
- CSP forbids inline styles: positions and custom properties are set from JavaScript.
- Both languages via `data-en`; rails work with keyboard; visible focus and 44px targets stay intact.
