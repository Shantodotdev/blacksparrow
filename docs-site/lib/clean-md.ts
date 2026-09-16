/**
 * Clean Markdown Normalizer for AI Agents & LLMs.
 * Converts Docora's proprietary MDC component directives (hero, card-group, tabs,
 * callouts, CTA buttons) into pure, semantic GitHub Flavored Markdown (GFM).
 *
 * Removes all framework-specific token noise and provides clean, token-efficient
 * text optimized for agentic context windows.
 */

export function splitFrontmatter(raw: string): {
  body: string
  title?: string
  description?: string
} {
  const match = raw.match(/^---\r?\n([\s\S]*?)\r?\n---\r?\n?([\s\S]*)$/)
  if (!match) return { body: raw }

  const yaml = match[1]
  const body = match[2]

  const titleMatch = yaml.match(/^title:\s*(.+)$/m)
  const descMatch = yaml.match(/^description:\s*(.+)$/m)

  const title = titleMatch ? titleMatch[1].trim().replace(/^['"]|['"]$/g, '') : undefined
  const description = descMatch ? descMatch[1].trim().replace(/^['"]|['"]$/g, '') : undefined

  return { body, title, description }
}

export function cleanMdc(content: string): string {
  // 1. Temporarily protect fenced code blocks so regex transformations never touch code
  const codeBlocks: string[] = []
  let text = content.replace(/```[\s\S]*?```/g, match => {
    codeBlocks.push(match)
    return `__CODE_BLOCK_${codeBlocks.length - 1}__`
  })

  // 2. Convert hero directives
  text = text.replace(
    /::hero\{[^}]*title="([^"]+)"[^}]*description="([^"]+)"[^}]*\}/g,
    '# $1\n\n> $2\n',
  )
  text = text.replace(/::hero\{[^}]*\}/g, '')

  // 3. Convert callouts (note, tip, warning, info, caution) into standard GFM blockquotes
  text = text.replace(
    /::(note|tip|warning|info|caution)(?:\{[^}]*\})?\s*\n([\s\S]*?)\n::/g,
    (_, type, body) => {
      const alertType = type.toUpperCase()
      const quoted = body
        .trim()
        .split('\n')
        .map((line: string) => (line.length ? `> ${line}` : '>'))
        .join('\n')
      return `> [!${alertType}]\n${quoted}\n`
    },
  )

  // 4. Convert cards into clean markdown links
  text = text.replace(
    /:::card\{title="([^"]+)"[^}]*to="([^"]+)"[^}]*\}\s*([\s\S]*?):::/g,
    (_, title, to, body) => {
      const cleanBody = body.trim().replace(/\n+/g, ' ')
      return cleanBody ? `- [**${title}**](${to}): ${cleanBody}` : `- [**${title}**](${to})`
    },
  )

  // 5. Convert CTA buttons into markdown links
  text = text.replace(/::::?cta\{label="([^"]+)"\s+to="([^"]+)"[^}]*\}/g, '- [$1]($2)')

  // 6. Convert tabs-item to clean subheadings
  text = text.replace(/::::?tabs-item\{label="([^"]+)"[^}]*\}/g, '\n#### $1\n')

  // 7. Strip wrapper container tags
  text = text.replace(/:::(hero-actions|hero-preview|card-group|tabs|steps|accordion)[^\n]*/g, '')
  text = text.replace(/::(hero-actions|hero-preview|card-group|tabs|steps|accordion)[^\n]*/g, '')

  // 8. Strip standalone closing colons
  text = text.replace(/^:{2,5}\s*$/gm, '')

  // 9. Restore code blocks
  text = text.replace(/__CODE_BLOCK_(\d+)__/g, (_, index) => codeBlocks[Number(index)] ?? '')

  // 10. Clean up multiple blank lines
  text = text.replace(/\n{3,}/g, '\n\n')

  return text.trim()
}
