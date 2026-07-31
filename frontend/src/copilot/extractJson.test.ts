import { describe, it, expect } from 'vitest'
import { extractJson } from './extractJson'

describe('extractJson', () => {
  it('returns a bare JSON object unchanged', () => {
    expect(extractJson('{"a":1}')).toBe('{"a":1}')
  })

  it('extracts JSON from a ```json fence', () => {
    const text = 'Here is the model:\n```json\n{"a":1,"b":[2,3]}\n```\nDone.'
    expect(JSON.parse(extractJson(text))).toEqual({ a: 1, b: [2, 3] })
  })

  it('extracts JSON from a bare ``` fence', () => {
    expect(JSON.parse(extractJson('```\n{"x":true}\n```'))).toEqual({ x: true })
  })

  it('extracts the first object from surrounding prose (no fence)', () => {
    const text = 'Sure! { "wasim_version": "0.1.0", "elements": [] } — that models it.'
    expect(JSON.parse(extractJson(text))).toEqual({ wasim_version: '0.1.0', elements: [] })
  })

  it('counts braces inside string values correctly', () => {
    const text = '{"note":"a } here and { there","n":1}'
    expect(JSON.parse(extractJson(text))).toEqual({ note: 'a } here and { there', n: 1 })
  })

  it('handles escaped quotes inside strings', () => {
    const text = String.raw`{"q":"say \"hi\" }","ok":true}`
    expect(JSON.parse(extractJson(text))).toEqual({ q: 'say "hi" }', ok: true })
  })

  it('extracts a nested object', () => {
    const text = 'prefix {"a":{"b":{"c":1}}} suffix'
    expect(JSON.parse(extractJson(text))).toEqual({ a: { b: { c: 1 } } })
  })

  it('throws when there is no JSON object', () => {
    expect(() => extractJson('no braces here')).toThrow(/no JSON/i)
  })

  it('throws on an unbalanced object', () => {
    expect(() => extractJson('{"a":1, "b":')).toThrow(/unbalanced/i)
  })
})
