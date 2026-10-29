/** English plurals: `plural(n, '# task', '# tasks')`, where `#` is the number. */
export function plural(n: number, one: string, other: string): string {
  return (n === 1 ? one : other).replace('#', String(n))
}
