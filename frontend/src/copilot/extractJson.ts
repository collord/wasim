/**
 * Extract a single JSON object from an LLM response. The model may wrap the model.json in prose or
 * ```json fences; this pulls out the first balanced top-level `{…}`. String-aware brace counting so
 * braces inside JSON string values (or escaped quotes) don't throw off the depth. Pure and
 * dependency-free — the copilot loop treats a "no JSON found" as a validation-style failure and
 * feeds it back, so this throws a clear message rather than returning something ambiguous.
 */
export function extractJson(text: string): string {
  // Prefer a fenced block if present — ```json … ``` or ``` … ```.
  const fence = text.match(/```(?:json)?\s*([\s\S]*?)```/i)
  const haystack = fence ? fence[1] : text

  const start = haystack.indexOf('{')
  if (start === -1) throw new Error('Response contained no JSON object.')

  let depth = 0
  let inString = false
  let escaped = false
  for (let i = start; i < haystack.length; i++) {
    const ch = haystack[i]
    if (inString) {
      if (escaped) escaped = false
      else if (ch === '\\') escaped = true
      else if (ch === '"') inString = false
      continue
    }
    if (ch === '"') inString = true
    else if (ch === '{') depth++
    else if (ch === '}') {
      depth--
      if (depth === 0) return haystack.slice(start, i + 1)
    }
  }
  throw new Error('Response contained an unbalanced JSON object (no closing brace).')
}
