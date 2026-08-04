import { describe, it, expect } from 'vitest'
import { nodesInLasso } from './EditableCanvas'

/**
 * The lasso stroke's enclosure geometry (EMITTER_LENS_TARGETING.md §8 — the deferred "loop" stroke).
 * The mouse wiring around it needs a live canvas, but *which nodes a rect encloses* is pure and is
 * the part worth locking down: a node is selected iff its center lies within the (orientation-free)
 * rubber-band rect.
 */

const nodes = [
  { id: 'a', pos: { x: 10, y: 10 } },
  { id: 'b', pos: { x: 50, y: 50 } },
  { id: 'c', pos: { x: 200, y: 200 } },
  { id: 'edge', pos: { x: 100, y: 100 } }, // exactly on the boundary below
]

describe('nodesInLasso', () => {
  it('selects nodes whose center is inside the rect', () => {
    expect(nodesInLasso(nodes, { x0: 0, y0: 0, x1: 100, y1: 100 })).toEqual(['a', 'b', 'edge'])
  })

  it('is orientation-free — a rect dragged up-left encloses the same nodes', () => {
    expect(nodesInLasso(nodes, { x0: 100, y0: 100, x1: 0, y1: 0 })).toEqual(['a', 'b', 'edge'])
  })

  it('includes a node exactly on the boundary (inclusive)', () => {
    expect(nodesInLasso([{ id: 'edge', pos: { x: 100, y: 100 } }], { x0: 0, y0: 0, x1: 100, y1: 100 })).toEqual(['edge'])
  })

  it('excludes nodes outside the rect', () => {
    expect(nodesInLasso(nodes, { x0: 0, y0: 0, x1: 60, y1: 60 })).toEqual(['a', 'b'])
  })

  it('returns an empty array when nothing is enclosed', () => {
    expect(nodesInLasso(nodes, { x0: 300, y0: 300, x1: 400, y1: 400 })).toEqual([])
  })
})
