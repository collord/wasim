import type { ModelDoc } from '../../model/schema'
import type { ToolDef } from '../providers/tools'
import { APPLY_EDIT_TOOL, applyEditPatch, type ApplyEditResult } from './applyEdit'

/**
 * The copilot's tool registry (§17.3). Today only `apply_edit`; the dispatcher shape lets more tools
 * (validate, run) slot in later. `executeTool` runs a tool against the working doc in the browser
 * (the edit transforms are pure); the loop validates the result through the engine.
 */

export const TOOLS: ToolDef[] = [APPLY_EDIT_TOOL]

/** Execute a tool call by name against the current working doc. Unknown tools return an error
 *  (never throw) so the loop can feed it back to the model. */
export function executeTool(name: string, input: Record<string, unknown>, doc: ModelDoc): ApplyEditResult {
  if (name === 'apply_edit') return applyEditPatch(doc, input)
  return { doc, error: `unknown tool "${name}".` }
}

export { APPLY_EDIT_TOOL, applyEditPatch }
export type { ApplyEditResult }
