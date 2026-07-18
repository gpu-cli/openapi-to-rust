import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://openapi-to-rust.dev",
  output: "static",
  // compressHTML strips the newline between a text line and a following inline
  // <a>/<code>, jamming words together ("…review theschema notes"). Gzip makes
  // the size difference negligible; correctness wins.
  compressHTML: false,
  trailingSlash: "never",
  integrations: [sitemap()],
  build: {
    format: "directory",
  },
  vite: {
    build: {
      cssMinify: "lightningcss",
    },
  },
});
