import { readFile } from 'node:fs/promises'
import docsConfig from '../../docs.config'
import { cleanMdc, splitFrontmatter } from '../../lib/clean-md'
import { source } from '../../lib/source'

export const dynamic = 'force-static'

function absolute(path: string): string {
  const baseUrl = docsConfig.site.url ?? 'https://blacksparrow.dev'
  return new URL(path, baseUrl).toString()
}

interface SectionGroup {
  name: string
  prefix: string
}

const SECTIONS: SectionGroup[] = [
  { name: 'Getting Started', prefix: '/docs/getting-started' },
  { name: 'CLI Reference', prefix: '/docs/cli-reference' },
  { name: '120 Technical SEO Rules Catalog', prefix: '/docs/seo-rules' },
  { name: 'Architecture & Engine', prefix: '/docs/architecture' },
  { name: 'AI & Model Context Protocol (MCP)', prefix: '/docs/ai-and-mcp' },
  { name: 'Developer Guide', prefix: '/docs/developer-guide' },
]

export async function GET() {
  const allPages = await source.getPages()

  // Separate root landing page from documentation guides
  const landingPage = allPages.find(p => p.path === '/')
  const docPages = allPages.filter(p => p.path !== '/')

  // Sort pages into sections
  const categorized: { section: string; pages: typeof docPages }[] = []

  for (const sec of SECTIONS) {
    const secPages = docPages.filter(p => p.path.startsWith(sec.prefix))
    if (secPages.length > 0) {
      categorized.push({ section: sec.name, pages: secPages })
    }
  }

  // Any remaining pages that did not match standard prefixes
  const categorizedPaths = new Set(categorized.flatMap(c => c.pages.map(p => p.path)))
  const miscPages = docPages.filter(p => !categorizedPaths.has(p.path))
  if (miscPages.length > 0) {
    categorized.push({ section: 'Additional Guides', pages: miscPages })
  }

  // Build Executive Header & Table of Contents
  const tocLines: string[] = [
    `# ${docsConfig.site.name} — Complete Documentation (\`llms-full.txt\`)`,
    '',
    `> ${docsConfig.site.description}`,
    `> Canonical URL: ${docsConfig.site.url}`,
    `> Repository: https://github.com/Shantodotdev/blacksparrow`,
    '',
    '## Table of Contents',
    '',
  ]

  for (const cat of categorized) {
    tocLines.push(`### ${cat.section}`)
    for (const page of cat.pages) {
      const desc = page.frontmatter.description ? `: ${page.frontmatter.description}` : ''
      tocLines.push(`- [${page.title}](${absolute(page.path)})${desc}`)
    }
    tocLines.push('')
  }

  // Process all page contents with MDC cleaning
  const docSections: string[] = []

  // Add landing page overview first if present
  if (landingPage) {
    const raw = await readFile(landingPage.filePath, 'utf8')
    const { body, description } = splitFrontmatter(raw)
    const cleanBody = cleanMdc(body)

    docSections.push(
      [
        '================================================================================',
        `DOCUMENT: Overview & Feature Summary`,
        `URL: ${absolute('/')}`,
        ...(description ? [`Description: ${description}`] : []),
        '================================================================================',
        '',
        cleanBody,
      ].join('\n'),
    )
  }

  // Add categorized document sections
  for (const cat of categorized) {
    for (const page of cat.pages) {
      const raw = await readFile(page.filePath, 'utf8')
      const { body, title, description } = splitFrontmatter(raw)
      const cleanBody = cleanMdc(body)

      const docTitle = title ?? page.title
      const docDesc = description ?? page.frontmatter.description

      const headerBlock = [
        '================================================================================',
        `SECTION: ${cat.section}`,
        `DOCUMENT: ${docTitle}`,
        `URL: ${absolute(page.path)}`,
        ...(docDesc ? [`Description: ${docDesc}`] : []),
        '================================================================================',
        '',
      ]

      if (!cleanBody.startsWith('# ')) {
        headerBlock.push(`# ${docTitle}`, '')
      }
      if (docDesc && !cleanBody.startsWith('> ')) {
        headerBlock.push(`> ${docDesc}`, '')
      }

      headerBlock.push(cleanBody)
      docSections.push(headerBlock.join('\n'))
    }
  }

  const fullContent = [...tocLines, ...docSections].join('\n\n')

  return new Response(fullContent, {
    headers: {
      'content-type': 'text/plain; charset=utf-8',
      'cache-control': 'public, max-age=3600, stale-while-revalidate=86400',
    },
  })
}
