// Distribution roster + default parameter templates for the Sample-node editor (spec §5.2).
// Families here are the ones the engine samples; params map to the flat `distribution`
// object ({ family, parameters: { name: {value,unit} | number } }).

import type { Quantity } from '../../types'

export interface DistDef {
  family: string
  label: string
  group: string
  /** Ordered parameter names shown as fields. */
  params: string[]
  /** Default parameter values when switching to this family. */
  defaults: () => Record<string, Quantity | number>
}

const q = (value: number, unit = '1'): Quantity => ({ value, unit })

export const DISTRIBUTIONS: DistDef[] = [
  { family: 'uniform', label: 'Uniform', group: 'Continuous', params: ['min', 'max'], defaults: () => ({ min: q(0), max: q(1) }) },
  { family: 'normal', label: 'Normal', group: 'Continuous', params: ['mean', 'stddev'], defaults: () => ({ mean: q(0), stddev: q(1) }) },
  { family: 'lognormal', label: 'Lognormal', group: 'Continuous', params: ['mean', 'stddev'], defaults: () => ({ mean: q(0), stddev: q(1) }) },
  { family: 'lognormal_moments', label: 'Lognormal (moments)', group: 'Continuous', params: ['mean', 'stddev'], defaults: () => ({ mean: q(1), stddev: q(0.5) }) },
  { family: 'triangular', label: 'Triangular', group: 'Continuous', params: ['min', 'mode', 'max'], defaults: () => ({ min: q(0), mode: q(0.5), max: q(1) }) },
  { family: 'pert', label: 'PERT', group: 'Continuous', params: ['min', 'mode', 'max'], defaults: () => ({ min: q(0), mode: q(0.5), max: q(1) }) },
  { family: 'exponential', label: 'Exponential', group: 'Continuous', params: ['mean'], defaults: () => ({ mean: q(1) }) },
  { family: 'gamma', label: 'Gamma', group: 'Continuous', params: ['shape', 'scale'], defaults: () => ({ shape: q(2), scale: q(1) }) },
  { family: 'weibull', label: 'Weibull', group: 'Continuous', params: ['shape', 'scale'], defaults: () => ({ shape: q(1.5), scale: q(1) }) },
  { family: 'beta', label: 'Beta', group: 'Continuous', params: ['alpha', 'beta'], defaults: () => ({ alpha: q(2), beta: q(2) }) },
  { family: 'pearson_v', label: 'Pearson V', group: 'Continuous', params: ['shape', 'scale'], defaults: () => ({ shape: q(3), scale: q(1) }) },
  { family: 'pearson_iii', label: 'Pearson III', group: 'Continuous', params: ['mean', 'stddev', 'skewness'], defaults: () => ({ mean: q(0), stddev: q(1), skewness: q(0.5) }) },
  { family: 'discrete_uniform', label: 'Discrete Uniform', group: 'Discrete', params: ['min', 'max'], defaults: () => ({ min: 0, max: 10 }) },
  { family: 'bernoulli', label: 'Bernoulli', group: 'Discrete', params: ['prob'], defaults: () => ({ prob: q(0.5) }) },
]

export function distDef(family: string): DistDef | undefined {
  return DISTRIBUTIONS.find((d) => d.family === family)
}

export function paramValue(p: Quantity | number | undefined): number {
  if (p == null) return 0
  return typeof p === 'number' ? p : p.value
}
