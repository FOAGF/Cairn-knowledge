// A page body starts with its `# title` line (the convention of every page body, replies.rs). Where a
// screen has its own title field, the editor shows the body without that line and the line is written
// back from the title field, so the title is edited in one place and a rename keeps the line in step.
// Only a first line that is exactly `# <title>` counts: any other first heading is the user's content.

/** The body without its `# title` line; `had` says whether the body started with it. */
export function splitTitleLine(body: string, title: string): { rest: string; had: boolean } {
  const m = /^# ([^\n]*)(?:\n\n?|$)/.exec(body)
  if (!m || m[1].trim() !== title.trim()) return { rest: body, had: false }
  return { rest: body.slice(m[0].length), had: true }
}

/** The body with its `# title` line in front: the inverse of `splitTitleLine`. */
export function joinTitleLine(title: string, rest: string): string {
  return rest ? `# ${title}\n\n${rest}` : `# ${title}`
}
