import { readdir, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const dist = path.resolve("dist");
const canonicalOrigin = "https://openapi-to-rust.dev";
const errors = [];
const titles = new Map();
const descriptions = new Map();

async function findHtml(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const nested = await Promise.all(entries.map((entry) => {
    const absolute = path.join(directory, entry.name);
    return entry.isDirectory() ? findHtml(absolute) : absolute.endsWith(".html") ? [absolute] : [];
  }));
  return nested.flat();
}

function capture(html, pattern) {
  return html.match(pattern)?.[1]?.trim();
}

function routeFor(file) {
  const relative = path.relative(dist, file);
  if (relative === "index.html") return "/";
  if (relative.endsWith("/index.html")) return `/${relative.slice(0, -"/index.html".length)}`;
  return `/${relative.replace(/\.html$/, "")}`;
}

function addDuplicate(map, value, route, kind) {
  if (!value) return;
  if (map.has(value)) errors.push(`${route}: duplicate ${kind} also used by ${map.get(value)}`);
  map.set(value, route);
}

function resolvesInternal(href) {
  if (href === "/") return existsSync(path.join(dist, "index.html"));
  const clean = href.replace(/\/$/, "").replace(/^\//, "");
  return existsSync(path.join(dist, clean, "index.html")) || existsSync(path.join(dist, `${clean}.html`)) || existsSync(path.join(dist, clean));
}

if (!existsSync(dist)) {
  console.error("dist/ is missing; run npm run build first");
  process.exit(1);
}

const pages = await findHtml(dist);
for (const file of pages) {
  const html = await readFile(file, "utf8");
  const route = routeFor(file);
  const noindex = /<meta name="robots" content="noindex, nofollow"/.test(html);
  const title = capture(html, /<title>([^<]+)<\/title>/);
  const description = capture(html, /<meta name="description" content="([^"]+)"/);
  const canonical = capture(html, /<link rel="canonical" href="([^"]+)"/);
  const h1Count = (html.match(/<h1\b/g) ?? []).length;

  if (!title) errors.push(`${route}: missing title`);
  if (!description) errors.push(`${route}: missing meta description`);
  if (!canonical?.startsWith(canonicalOrigin)) errors.push(`${route}: canonical must use ${canonicalOrigin}`);
  if (h1Count !== 1) errors.push(`${route}: expected 1 h1, found ${h1Count}`);
  if (!/<meta name="viewport"/.test(html)) errors.push(`${route}: missing viewport meta`);
  if (!/<meta property="og:image" content="https:\/\/openapi-to-rust\.dev\/og-card\.png"/.test(html)) errors.push(`${route}: missing canonical OG image`);

  if (!noindex) {
    if (title && (title.length < 30 || title.length > 70)) errors.push(`${route}: title length ${title.length} is outside 30–70`);
    if (description && (description.length < 120 || description.length > 170)) errors.push(`${route}: description length ${description.length} is outside 120–170`);
    addDuplicate(titles, title, route, "title");
    addDuplicate(descriptions, description, route, "description");
  }

  for (const match of html.matchAll(/<script[^>]+type="application\/ld\+json"[^>]*>([\s\S]*?)<\/script>/g)) {
    try {
      JSON.parse(match[1]);
    } catch (error) {
      errors.push(`${route}: invalid JSON-LD (${error.message})`);
    }
  }

  for (const match of html.matchAll(/<a\b[^>]*href="(\/[^"#?]*)"/g)) {
    if (!resolvesInternal(match[1])) errors.push(`${route}: broken internal link ${match[1]}`);
  }
}

const sitemapIndex = path.join(dist, "sitemap-index.xml");
if (!existsSync(sitemapIndex)) errors.push("missing sitemap-index.xml");
if (!existsSync(path.join(dist, "robots.txt"))) errors.push("missing robots.txt");
if (!existsSync(path.join(dist, "og-card.png"))) errors.push("missing og-card.png");

if (errors.length) {
  console.error(`SEO audit failed with ${errors.length} issue(s):`);
  for (const error of errors) console.error(`- ${error}`);
  process.exit(1);
}

console.log(`SEO audit passed for ${pages.length} rendered pages.`);
