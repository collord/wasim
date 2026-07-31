// Small formatters shared by the reliability and decision behavior readouts.

export const clamp01 = (x: number) => Math.max(0, Math.min(1, x))
export const pct = (x: number) => `${(x * 100).toFixed(1)}%`
export const fmtNum = (x: number) => (Number.isInteger(x) ? String(x) : x.toFixed(1))
