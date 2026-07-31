/**
 * The lens registry public surface. As of the manifest refactor
 * (`WASIM_LENS_MANIFEST_SPEC.md` §3), lenses are assembled by the **loader** from declarative
 * JSON manifests (`lenses/*.json`) over the element registry (`element-registry.json`) plus code
 * behavior plugins (`behaviors/`) — no longer hand-written here. This module stays as the stable
 * import surface (`store.ts`, `Toolbar.tsx`, …) so consumers don't move.
 */
export { resolveLens, listLenses, DEFAULT_LENS, LENSES } from './loader'
