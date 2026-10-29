// Tasks: queries and mutations (spec §4). Rust emits DataChanged { tasks, activity } after every
// change, which refetches every query under ['tasks'] and ['activity'], so mutations here don't
// invalidate anything themselves. Errors from mutations are shown as toasts (client.ts).
//
// Query keys: ['tasks', 'list', filter] · ['tasks', 'get', idOrKey] · ['tasks', 'byKeys', keys]
// · ['tasks', 'labels'] · ['tasks', 'comments' | 'links' | 'versions' | 'summary', taskId] · ['activity', limit]
import { useMutation, useQuery } from '@tanstack/react-query'
import {
  type ActivityEvent,
  commands,
  type NewTask,
  type Task,
  type TaskComment,
  type TaskFilter,
  type TaskLink,
  type TaskLinkType,
  type TaskPatch,
  type TaskSeed,
  type TaskVersion,
} from './bindings'
import { queryClient } from './client'

export type {
  ActivityEvent,
  CompletionSummary,
  DueFilter,
  NewTask,
  Priority,
  Task,
  TaskComment,
  TaskFilter,
  TaskLink,
  TaskLinkType,
  TaskPatch,
  TaskSnapshot,
  TaskSortField,
  TaskStatus,
  TaskType,
  TaskVersion,
} from './bindings'

const EMPTY_TASKS: Task[] = []
const EMPTY_COMMENTS: TaskComment[] = []
const EMPTY_LINKS: TaskLink[] = []
const EMPTY_VERSIONS: TaskVersion[] = []
const EMPTY_LABELS: string[] = []
const EMPTY_ACTIVITY: ActivityEvent[] = []

/** Every task, closed ones included. For screens that count or look up tasks across categories. */
export const ALL_TASKS: TaskFilter = { includeClosed: true }

// ---------------------------------------------------------------- queries

/** Tasks matching `filter` (filtering and sorting happen in Rust). Keep `filter` stable (useMemo or a constant). */
export function useTasks(filter: TaskFilter = ALL_TASKS) {
  return useQuery({ queryKey: ['tasks', 'list', filter], queryFn: () => commands.listTasks(filter) })
}

/** Like useTasks, but just the array: empty until loaded, never a new array per render. */
export function useTaskList(filter: TaskFilter = ALL_TASKS): Task[] {
  return useTasks(filter).data ?? EMPTY_TASKS
}

/** One task by id, current key or old key (FR-TASK-UI-6). Compare `data.key` with the URL to redirect from an alias. */
export function useTask(idOrKey: string | undefined) {
  return useQuery({
    queryKey: ['tasks', 'get', idOrKey],
    queryFn: () => commands.getTask(idOrKey ?? ''),
    enabled: !!idOrKey,
  })
}

/** Tasks for `#KEY` mentions. Pass a sorted, de-duplicated key list for a stable query key. */
export function useTasksByKeys(keys: string[]) {
  return useQuery({
    queryKey: ['tasks', 'byKeys', keys],
    queryFn: () => commands.getTasksByKeys(keys),
    enabled: keys.length > 0,
  })
}

export function useTaskLabels(): string[] {
  return useQuery({ queryKey: ['tasks', 'labels'], queryFn: () => commands.listTaskLabels() }).data ?? EMPTY_LABELS
}

export function useTaskComments(taskId: string | undefined): TaskComment[] {
  return (
    useQuery({
      queryKey: ['tasks', 'comments', taskId],
      queryFn: () => commands.listTaskComments(taskId ?? ''),
      enabled: !!taskId,
    }).data ?? EMPTY_COMMENTS
  )
}

/** Links where the task is on either side. For `parent`, `fromTaskId` is the parent. */
export function useTaskLinks(taskId: string | undefined): TaskLink[] {
  return (
    useQuery({
      queryKey: ['tasks', 'links', taskId],
      queryFn: () => commands.listTaskLinks(taskId ?? ''),
      enabled: !!taskId,
    }).data ?? EMPTY_LINKS
  )
}

/** Daily versions, oldest first (FR-TASK-HI-3). */
export function useTaskVersions(taskId: string | undefined): TaskVersion[] {
  return (
    useQuery({
      queryKey: ['tasks', 'versions', taskId],
      queryFn: () => commands.listTaskVersions(taskId ?? ''),
      enabled: !!taskId,
    }).data ?? EMPTY_VERSIONS
  )
}

/** Working days and blocked days; `null` unless the task is done (FR-TASK-SUM-1). */
export function useCompletionSummary(taskId: string | undefined) {
  return useQuery({
    queryKey: ['tasks', 'summary', taskId],
    queryFn: () => commands.getCompletionSummary(taskId ?? ''),
    enabled: !!taskId,
  })
}

/** Recent task activity, newest first. (Page activity joins in M3.) */
export function useActivity(limit = 50): ActivityEvent[] {
  return useQuery({ queryKey: ['activity', limit], queryFn: () => commands.listActivity(limit) }).data ?? EMPTY_ACTIVITY
}

/** For code outside React (fake assistant, store actions): all tasks, from the cache when loaded. */
export function fetchAllTasks(): Promise<Task[]> {
  return queryClient.fetchQuery({ queryKey: ['tasks', 'list', ALL_TASKS], queryFn: () => commands.listTasks(ALL_TASKS) })
}

// ---------------------------------------------------------------- mutations

export function useCreateTask() {
  return useMutation({ mutationFn: (input: NewTask) => commands.createTask(input) })
}

/** Only the given fields change. `dueDate: ''` clears the due date. */
/**
 * Puts a task returned by a change into every cached single-task query that shows it (by id,
 * current key or an old key), so a screen never shows an outdated copy while it refetches —
 * e.g. after a move, when the page switches to the task's new (or restored) key.
 */
export function primeTask(t: Task) {
  for (const [key, data] of queryClient.getQueriesData<Task>({ queryKey: ['tasks', 'get'] })) {
    if (data?.id === t.id) queryClient.setQueryData(key, t)
  }
  queryClient.setQueryData(['tasks', 'get', t.id], t)
  queryClient.setQueryData(['tasks', 'get', t.key], t)
}

/**
 * Puts a saved task into every loaded task list right away, instead of waiting for the refetch
 * that `DataChanged` starts. The board uses it so a dropped card doesn't jump back for a moment.
 */
export function primeTaskInLists(t: Task) {
  queryClient.setQueriesData<Task[]>({ queryKey: ['tasks', 'list'] }, (list) =>
    list?.some((x) => x.id === t.id) ? list.map((x) => (x.id === t.id ? t : x)) : list,
  )
}

export function useUpdateTask() {
  return useMutation({
    mutationFn: ({ id, patch }: { id: string; patch: TaskPatch }) => commands.updateTask(id, patch),
    onSuccess: primeTask,
  })
}

/** New key in the target category (or the task's old key there); the key it leaves stays an alias (FR-TASK-UI-6). */
export function useMoveTask() {
  return useMutation({
    mutationFn: ({ id, categoryId }: { id: string; categoryId: string }) => commands.moveTask(id, categoryId),
    onSuccess: primeTask,
  })
}

/** Permanent: ask for confirmation first (FR-TASK-UI-3). */
export function useDeleteTask() {
  return useMutation({ mutationFn: (id: string) => commands.deleteTask(id) })
}

export function useAddTaskComment() {
  return useMutation({ mutationFn: ({ taskId, text }: { taskId: string; text: string }) => commands.addTaskComment(taskId, text) })
}

export function useUpdateTaskComment() {
  return useMutation({ mutationFn: ({ id, text }: { id: string; text: string }) => commands.updateTaskComment(id, text) })
}

export function useDeleteTaskComment() {
  return useMutation({ mutationFn: (id: string) => commands.deleteTaskComment(id) })
}

/** Rules (FR-TASK-LN-4) are checked in Rust; a refused link shows its reason as a toast. */
export function useAddTaskLink() {
  return useMutation({
    mutationFn: ({ type, fromTaskId, toTaskId }: { type: TaskLinkType; fromTaskId: string; toTaskId: string }) =>
      commands.addTaskLink(type, fromTaskId, toTaskId),
  })
}

export function useRemoveTaskLink() {
  return useMutation({ mutationFn: (id: string) => commands.removeTaskLink(id) })
}

/**
 * Development only: inserts the prototype's sample tasks (with comments, links, history and activity)
 * where their ids are missing. Call after the sample categories are seeded.
 */
export async function seedDevTasks(seed: TaskSeed) {
  await commands.devSeedTasks(seed)
}
