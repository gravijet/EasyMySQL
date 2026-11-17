// Builds public/en/index.html from public/index.html, so English has its own address for search
// engines. Every data-en text and data-en-* attribute is swapped in; the German original is kept in
// data-de / data-de-* so app.js can switch back. Run before deploying: node scripts/en.mjs
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";

const dir = new URL("../public/", import.meta.url);
let html = readFileSync(new URL("index.html", dir), "utf8");
let texts = 0;

// <tag ... data-en="EN" ...>DE</tag>  (app.js swaps textContent, so these hold plain text only)
html = html.replace(/<([a-z0-9]+)\b([^>]*?\sdata-en="([^"]*)"[^>]*)>([^<]*)<\/\1>/g, (_, tag, attrs, en, de) => {
  texts++;
  return `<${tag}${attrs} data-de="${de.replace(/"/g, "&quot;")}">${en}</${tag}>`;
});
const expected = (readFileSync(new URL("index.html", dir), "utf8").match(/\sdata-en="/g) || []).length;
if (texts !== expected) throw new Error(`en.mjs: swapped ${texts} of ${expected} data-en texts (nested markup?)`);

// data-en-alt / -label / -content / -href: the attribute itself (label means aria-label)
html = html.replace(/<[a-z0-9]+\b[^>]*\sdata-en-[a-z]+="[^>]*>/g, (tag) => {
  for (const [, kind, en] of [...tag.matchAll(/\sdata-en-([a-z]+)="([^"]*)"/g)]) {
    const name = kind === "label" ? "aria-label" : kind;
    const re = new RegExp(`(\\s${name}=")([^"]*)"`);
    const de = (tag.match(re) || [])[2];
    if (de === undefined) throw new Error(`en.mjs: ${name} missing in ${tag}`);
    tag = tag.replace(re, `$1${en}" data-de-${kind}="${de}"`);
  }
  return tag;
});

html = html.replace('<html lang="de">', '<html lang="en">');
mkdirSync(new URL("en/", dir), { recursive: true });
writeFileSync(new URL("en/index.html", dir), html);
console.log(`en/index.html: ${texts} texts`);
