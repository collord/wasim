import type { ModelDoc, FlatElement } from '../../model/schema'
import { addElement, updateElement, deleteElement, renameId, findElement, uniqueId, recomputeInputs } from '../../model/edits'
import type { ToolDef } from '../providers/tools'

/**
 * The `apply_edit` tool (WASIM_AUTHORING_ENVIRONMENT_SPEC.md §17.3): an incremental edit to the
 * current model, so Refine makes surgical changes instead of regenerating the whole doc. The model
 * calls it with an operation (add | modify | delete | rename); we apply it via the pure `edits.ts`
 * transforms and hand the result back to the loop to validate through the engine.
 *
 * The executor NEVER throws — bad input becomes `{doc, error}` so the loop feeds the error back as a
 * tool result the model can correct, rather than aborting the conversation.
 */

/** The tool's input schema (JSON Schema, accepted by both Anthropic `input_schema` and OpenAI
 *  `function.parameters`). Flat with an `op` enum — more robustly honored by weaker models than a
 *  nested oneOf; the executor validates the per-op fields. */
export const APPLY_EDIT_TOOL: ToolDef = {
  name: 'apply_edit',
  description:
    'Apply one incremental edit to the current WASiM model. Use this for each change (add a new ' +
    'element, modify fields of an existing element, delete an element, or rename an element id) ' +
    'instead of re-emitting the whole model. After each edit the engine validates the result and ' +
    'reports back so you can fix any issues.',
  inputSchema: {
    type: 'object',
    properties: {
      op: { type: 'string', enum: ['add', 'modify', 'delete', 'rename'], description: 'The edit operation.' },
      element: {
        type: 'object',
        description: 'For op=add: the complete new element (a flat elements[] entry, incl. its id, name, primitive, etc.).',
        additionalProperties: true,
      },
      position: {
        type: 'object',
        description: 'For op=add: optional canvas position.',
        properties: { x: { type: 'number' }, y: { type: 'number' } },
      },
      id: { type: 'string', description: 'For op=modify/delete/rename: the target element id.' },
      patch: {
        type: 'object',
        description: 'For op=modify: fields to shallow-merge onto the element.',
        additionalProperties: true,
      },
      newId: { type: 'string', description: 'For op=rename: the new element id.' },
    },
    required: ['op'],
  },
}

export interface ApplyEditResult {
  doc: ModelDoc
  error?: string
}

/** Apply one edit patch to a doc via the pure edits.ts transforms. Returns the (possibly unchanged)
 *  doc and an optional error string. Never throws. */
export function applyEditPatch(doc: ModelDoc, patch: unknown): ApplyEditResult {
  if (!patch || typeof patch !== 'object') return { doc, error: 'apply_edit input must be an object.' }
  const p = patch as Record<string, unknown>
  const op = p.op

  try {
    switch (op) {
      case 'add': {
        const element = p.element as FlatElement | undefined
        if (!element || typeof element !== 'object') return { doc, error: 'add requires an "element" object.' }
        if (typeof element.id !== 'string' || !element.id) return { doc, error: 'the added element needs a string "id".' }
        if (findElement(doc, element.id)) {
          return { doc, error: `an element with id "${element.id}" already exists — choose a different id.` }
        }
        recomputeInputs(element) // keep `inputs` honest with any expression/flow the element carries
        const pos = isPosition(p.position) ? p.position : undefined
        return { doc: addElement(doc, element, pos) }
      }
      case 'modify': {
        const id = p.id
        if (typeof id !== 'string') return { doc, error: 'modify requires a string "id".' }
        if (!findElement(doc, id)) return { doc, error: `no element with id "${id}".` }
        const patchFields = p.patch as Partial<FlatElement> | undefined
        if (!patchFields || typeof patchFields !== 'object') return { doc, error: 'modify requires a "patch" object.' }
        const next = updateElement(doc, id, patchFields)
        // A modify that touched an expression/rate/trigger may change refs — re-derive inputs.
        const el = findElement(next, id)
        if (el) recomputeInputs(el)
        return { doc: next }
      }
      case 'delete': {
        const id = p.id
        if (typeof id !== 'string') return { doc, error: 'delete requires a string "id".' }
        if (!findElement(doc, id)) return { doc, error: `no element with id "${id}".` }
        return { doc: deleteElement(doc, id) } // scrubs dangling id-list/scalar/effect/view refs
      }
      case 'rename': {
        const id = p.id
        const newId = p.newId
        if (typeof id !== 'string' || typeof newId !== 'string') return { doc, error: 'rename requires string "id" and "newId".' }
        if (!findElement(doc, id)) return { doc, error: `no element with id "${id}".` }
        if (uniqueId(doc, newId) !== newId) return { doc, error: `id "${newId}" is already taken.` }
        return { doc: renameId(doc, id, newId) }
      }
      default:
        return { doc, error: `unknown op "${String(op)}" — use add | modify | delete | rename.` }
    }
  } catch (e) {
    return { doc, error: `apply_edit failed: ${(e as Error).message}` }
  }
}

function isPosition(v: unknown): v is { x: number; y: number } {
  return !!v && typeof v === 'object' && typeof (v as { x?: unknown }).x === 'number' && typeof (v as { y?: unknown }).y === 'number'
}
