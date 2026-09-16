import { defineDocsConfig } from "docora";

export default defineDocsConfig({
  site: {
    name: "Black Sparrow",
    description:
      "High-performance, local-first website crawler, 120-rule technical SEO audit engine, and AI-native auditor in Rust.",
    url: "https://blacksparrowdev.vercel.app/",
    locale: "en",
  },

  header: {
    links: [
      { label: "Documentation", href: "/docs/getting-started/introduction" },
      { label: "CLI Reference", href: "/docs/cli-reference/overview" },
      { label: "SEO Rules (120)", href: "/docs/seo-rules/overview" },
      { label: "AI & MCP", href: "/docs/ai-and-mcp/mcp-setup" },
      { label: "Architecture", href: "/docs/architecture/crawler-and-aimd" },
    ],
  },

  socials: {
    github: "https://github.com/Shantodotdev/blacksparrow",
  },

  toc: {
    title: "On this page",
  },

  colorMode: {
    default: "dark",
  },

  loadingIndicator: {
    enabled: true,
    color: "#e11d48",
  },

  footer: {
    credits: "Built with Docora & Next.js for Black Sparrow",
    columns: [
      {
        title: "Guides",
        links: [
          { label: "Introduction", href: "/docs/getting-started/introduction" },
          { label: "Installation", href: "/docs/getting-started/installation" },
          { label: "Quickstart", href: "/docs/getting-started/quickstart" },
          {
            label: "Core Concepts",
            href: "/docs/getting-started/core-concepts",
          },
        ],
      },
      {
        title: "Tools & Rules",
        links: [
          { label: "CLI Commands", href: "/docs/cli-reference/overview" },
          { label: "Audit Engine", href: "/docs/cli-reference/audit" },
          { label: "120 Rules Catalog", href: "/docs/seo-rules/overview" },
          {
            label: "AI Search & GEO",
            href: "/docs/seo-rules/ai-and-geo-readiness",
          },
        ],
      },
      {
        title: "Developers & AI",
        links: [
          {
            label: "Model Context Protocol (MCP)",
            href: "/docs/ai-and-mcp/mcp-setup",
          },
          {
            label: "Building from Source",
            href: "/docs/developer-guide/building-from-source",
          },
          {
            label: "Contributing Heuristics",
            href: "/docs/developer-guide/adding-custom-rules",
          },
          { label: "llms.txt", href: "/llms.txt" },
        ],
      },
    ],
  },
});
