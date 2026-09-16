import { readFile } from 'node:fs/promises'
import { cleanMdc, splitFrontmatter } from '../../../lib/clean-md'
import { source } from '../../../lib/source'

export const dynamic = 'force-static'

function rawSlug(page: { slug: string[] }): string[] {
  if (page.slug.length === 0) return ['index.md']
  return [...page.slug.slice(0, -1), `${page.slug.at(-1)}.md`]
}

export async function generateStaticParams() {
  const pages = await source.getPages()
  return pages.map(page => ({ slug: rawSlug(page) }))
}

export async function GET(
  _request: Request,
  context: { params: Promise<{ slug: string[] }> },
) {
  const { slug } = await context.params
  const last = slug.at(-1)

  if (!last?.endsWith('.md')) {
    return new Response('Not found', { status: 404 })
  }

  const name = last.slice(0, -3)
  const lookup = name === 'index' && slug.length === 1 ? [] : [...slug.slice(0, -1), name]

  const page = await source.getPage(lookup)
  if (!page) {
    return new Response('Not found', { status: 404 })
  }

  const raw = await readFile(page.filePath, 'utf8')
  const { body, title, description } = splitFrontmatter(raw)
  const cleanBody = cleanMdc(body)

  const docTitle = title ?? page.title
  const docDesc = description ?? page.frontmatter.description

  const lines: string[] = []
  if (docTitle && !cleanBody.startsWith('# ')) {
    lines.push(`# ${docTitle}`, '')
  }
  if (docDesc && !cleanBody.startsWith('> ')) {
    lines.push(`> ${docDesc}`, '')
  }
  lines.push(cleanBody)

  return new Response(lines.join('\n'), {
    headers: {
      'content-type': 'text/markdown; charset=utf-8',
      'cache-control': 'public, max-age=3600, stale-while-revalidate=86400',
    },
  })
}
