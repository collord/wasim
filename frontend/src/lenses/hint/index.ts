/**
 * The lens-hint module (`EMITTER_LENS_TARGETING.md`): a pure, ephemeral classifier that reads the
 * emitted/authored JSON and returns a container's dominant modeling paradigm + overlays, plus the
 * computed-role bridge that lets the existing `lens_role`-reading behaviors work on untagged
 * models. Nothing here is persisted into the model.
 */
export { lensHint, activeLensId, type LensHint, type OverlayId } from './lensHint'
export { computeRolesForLens, withComputedRoles, type RoleMap } from './roles'
export { SIGNATURES, HINT_FLOOR, type ScoreFn } from './signatures'
export { interiorOf } from './scope'
