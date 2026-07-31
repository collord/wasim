import { describe, it, expect } from 'vitest'
import { coldStartPrompt, refinePrompt } from './systemPrompt'
import { BATHTUB_EXEMPLAR } from './exemplar'

describe('systemPrompt builders', () => {
  it('cold-start includes the authoring guide and the exemplar', () => {
    const p = coldStartPrompt()
    expect(p).toContain('WASiM v2 model authoring guide') // guide header
    expect(p).toContain(BATHTUB_EXEMPLAR)
    expect(p).toMatch(/ONE complete WASiM v2 model/)
  })

  it('refine embeds the current model and the preserve-untouched instruction', () => {
    const current = '{"wasim_version":"0.1.0","elements":[{"id":"tap"}]}'
    const p = refinePrompt(current)
    expect(p).toContain('WASiM v2 model authoring guide')
    expect(p).toContain(current) // the current model is in the prompt
    expect(p).toMatch(/COMPLETE updated model/)
    expect(p).toMatch(/Preserve everything the change does not touch/)
  })

  it('refine does NOT include the exemplar (the current model is the shape reference)', () => {
    const p = refinePrompt('{"wasim_version":"0.1.0","elements":[]}')
    expect(p).not.toContain(BATHTUB_EXEMPLAR)
  })
})
