import type { FlatElement, ModelDoc } from '../../model/schema'

/**
 * Container scoping for the lens hint (consensus `WASIM_LENS_MODELING_TYPES.md` §1.2). A lens is
 * hinted for the container the user has drilled into, so "the semantics within a container" is the
 * set of elements whose `container` is that id. `null` is the model root (elements with no
 * container). This is direct membership — nested submodels are hinted at their own drill level.
 */
export function interiorOf(containerId: string | null, doc: ModelDoc): FlatElement[] {
  const cid = containerId ?? null
  return doc.elements.filter((e) => (e.container ?? null) === cid)
}
