import { describe, it, expect, beforeEach } from 'vitest'
import { useStore } from './store'

/**
 * Per-container lens override semantics (the "explicit pick must stick" fix). An explicit pick is
 * recorded unconditionally — even when it matches the current hint — so a later content change can't
 * silently re-detect *over* the user's choice. Returning to automatic is the separate, deliberate
 * `clearContainerLens`. `setContainerLens` and `clearContainerLens` are plain state ops (no worker),
 * so they're drivable directly without loading a model.
 */

const reset = () => useStore.setState({ activeContainerId: null, lensOverrides: {} })

describe('setContainerLens — an explicit pick always records', () => {
  beforeEach(reset)

  it('records a pick for the drilled container', () => {
    useStore.setState({ activeContainerId: 'sub' })
    useStore.getState().setContainerLens('reliability')
    expect(useStore.getState().lensOverrides).toEqual({ sub: 'reliability' })
  })

  it('records the pick even when it equals the current hint (the core fix)', () => {
    // Simulate: `sub` currently auto-detects to control-systems, and the user explicitly picks it.
    // The old tidy-map logic would drop the override, letting later re-detection override the pick.
    useStore.setState({ activeContainerId: 'sub' })
    useStore.getState().setContainerLens('control-systems')
    expect(useStore.getState().lensOverrides.sub).toBe('control-systems')
  })

  it('a pick per container is independent', () => {
    useStore.setState({ activeContainerId: 'a' })
    useStore.getState().setContainerLens('metapop')
    useStore.setState({ activeContainerId: 'b' })
    useStore.getState().setContainerLens('decision')
    expect(useStore.getState().lensOverrides).toEqual({ a: 'metapop', b: 'decision' })
  })

  it('at root, a pick routes to the whole-doc view.lens, not the override map', () => {
    useStore.setState({ activeContainerId: null, doc: { view: {}, elements: [] } as never })
    useStore.getState().setContainerLens('reliability')
    expect(useStore.getState().lensOverrides).toEqual({})
    expect((useStore.getState().doc as { view?: { lens?: string } } | null)?.view?.lens).toBe('reliability')
  })
})

describe('clearContainerLens — the only way back to automatic', () => {
  beforeEach(reset)

  it('drops the drilled container override, returning it to auto-detection', () => {
    useStore.setState({ activeContainerId: 'sub', lensOverrides: { sub: 'reliability', other: 'metapop' } })
    useStore.getState().clearContainerLens()
    expect(useStore.getState().lensOverrides).toEqual({ other: 'metapop' })
  })

  it('is a no-op at root', () => {
    useStore.setState({ activeContainerId: null, lensOverrides: { sub: 'reliability' } })
    useStore.getState().clearContainerLens()
    expect(useStore.getState().lensOverrides).toEqual({ sub: 'reliability' })
  })
})
