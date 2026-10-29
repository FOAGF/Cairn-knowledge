// The names of statuses, priorities, task types and page origins (lib/labels.ts reads them).
export const labels = {
  status: {
    todo: 'To do',
    in_progress: 'In progress',
    blocked: 'Blocked',
    done: 'Done',
    wont_do: "Won't do",
  },
  priority: { high: 'High', medium: 'Medium', low: 'Low' },
  type: { bug: 'Bug', feature: 'Feature', general: 'General' },
  origin: {
    manual: 'Written manually',
    file: 'Created from a file',
    task: 'Created from a task',
    folder: 'Found in the folder',
  },
}
