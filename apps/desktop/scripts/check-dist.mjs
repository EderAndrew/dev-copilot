// Fails when a built HTML file carries inline styles (specs/002 T018, research R1, F-1).
//
// The production CSP has `style-src 'self'` without 'unsafe-inline': a `<style>` element or a
// `style=` attribute in the shipped HTML would be blocked at runtime. Run after `vite build`:
//
//   node scripts/check-dist.mjs [dist-dir]
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const RULES = [
  { name: "<style> element", pattern: /<style[\s>/]/gi },
  { name: "style= attribute", pattern: /\sstyle\s*=/gi },
];

function htmlFiles(dir) {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return htmlFiles(path);
    return /\.html?$/i.test(entry) ? [path] : [];
  });
}

function lineOf(text, index) {
  return text.slice(0, index).split("\n").length;
}

/** Every inline style in the HTML files under `dir`, as `file:line: rule` strings. */
function findInlineStyles(dir) {
  const found = [];
  for (const file of htmlFiles(dir)) {
    const text = readFileSync(file, "utf8");
    for (const { name, pattern } of RULES) {
      for (const match of text.matchAll(pattern)) {
        found.push(`${relative(dir, file)}:${lineOf(text, match.index)}: ${name}`);
      }
    }
  }
  return found;
}

function main() {
  const here = fileURLToPath(new URL(".", import.meta.url));
  const dist = process.argv[2] ?? join(here, "..", "dist");
  let files;
  try {
    files = htmlFiles(dist);
  } catch {
    console.error(`check-dist: ${dist} not found; run the build first.`);
    return 1;
  }
  if (files.length === 0) {
    console.error(`check-dist: no HTML files in ${dist}.`);
    return 1;
  }
  const found = findInlineStyles(dist);
  if (found.length > 0) {
    console.error("check-dist: inline styles are blocked by the production CSP:");
    for (const line of found) console.error(`  ${line}`);
    return 1;
  }
  console.log(`check-dist: ${files.length} HTML file(s), no inline styles.`);
  return 0;
}

process.exitCode = main();
