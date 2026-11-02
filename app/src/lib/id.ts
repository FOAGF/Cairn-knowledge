/** Short random id with a readable prefix, e.g. `t-k3j9x2`. */
export function newId(prefix: string): string {
  return `${prefix}-${Math.random().toString(36).slice(2, 8)}`
}
