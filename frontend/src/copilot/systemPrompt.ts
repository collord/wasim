import authoringGuide from './authoring-guide.generated.md?raw'
import { BATHTUB_EXEMPLAR } from './exemplar'

/**
 * System-prompt builders for the copilot (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.2). Two turn
 * types share the same schema context (the generated authoring guide) but differ in task framing:
 *
 * - `coldStartPrompt` (Phase 1): draft a complete model from a description, with a known-good
 *   exemplar as the shape reference.
 * - `refinePrompt` (Phase 2): edit the user's *current* model. The current model itself is the shape
 *   reference (so no exemplar is needed — keeping the prompt smaller), and the instruction stresses
 *   returning the COMPLETE updated model while preserving everything the change doesn't touch.
 *
 * Both are pure functions of committed data (guide + exemplar are build-time constants; the current
 * model is passed in), so they're unit-testable.
 */

/** Draft a complete model from scratch. */
export function coldStartPrompt(): string {
  return (
    authoringGuide +
    '\n\nReturn ONE complete WASiM v2 model as a single JSON object. If you are given validation ' +
    'diagnostics, fix exactly those issues and return the corrected full model. A fenced ```json block ' +
    'is fine. Do not include commentary that is not part of the JSON.\n\n' +
    'A complete, engine-valid example to follow (note the exact shapes — `ref` uses `element_id`, ' +
    'stocks list flow ids in `inflows`/`outflows`):\n```json\n' +
    BATHTUB_EXEMPLAR +
    '\n```'
  )
}

/** Edit the user's current model. `currentModelJson` is the active doc, serialized. */
export function refinePrompt(currentModelJson: string): string {
  return (
    authoringGuide +
    "\n\nHere is the user's current WASiM v2 model:\n```json\n" +
    currentModelJson +
    '\n```\n\n' +
    'Apply the requested change and return the COMPLETE updated model as a single JSON object — not ' +
    'a diff, not only the changed elements. Preserve everything the change does not touch: element ' +
    'ids, view positions, unrelated elements, and simulation settings. Use the current model above as ' +
    'the shape reference (it is engine-valid). If you are given validation diagnostics, fix exactly ' +
    'those and return the full model again. A fenced ```json block is fine; no commentary outside it.'
  )
}

/**
 * Refine via the `apply_edit` tool (§17.3): the model makes surgical edits to the current model
 * rather than re-emitting the whole thing. Used by the tool-dispatch loop (`toolLoop.ts`).
 */
export function refineToolPrompt(currentModelJson: string): string {
  return (
    authoringGuide +
    "\n\nHere is the user's current WASiM v2 model:\n```json\n" +
    currentModelJson +
    '\n```\n\n' +
    'Apply the requested change by calling the `apply_edit` tool once per change — add a new element, ' +
    'modify fields of an existing element (by id), delete an element (by id), or rename an element ' +
    'id. Do NOT re-emit the whole model; make only the edits the request calls for and leave ' +
    'everything else untouched. New elements must follow the exact shapes in the guide above (e.g. an ' +
    'expression `ref` uses `element_id`; stocks list flow ids in `inflows`/`outflows`). After each ' +
    'edit the engine validates the result and reports back — if it reports an issue, call `apply_edit` ' +
    'again to fix it. When the change is complete and valid, stop calling tools and briefly confirm.'
  )
}
