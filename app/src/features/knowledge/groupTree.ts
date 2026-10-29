// Groups as a tree (FR-KN-EXT-2): in external mode a group is a folder and can hold groups, without a
// depth limit. In internal mode every group is at the top (one level, FR-KN-GR-1), so these helpers
// give the same names and order as before there.
import { commands, type Group } from '@/api/bindings'
import { readSettings } from '@/api/settings'
import { getT } from '@/i18n'

/** A group's parent in `groups`, or null when it is at the top (or its parent isn't in the list). */
function parentOf(group: Group, groups: Group[]): string | null {
  return group.parentId && groups.some((g) => g.id === group.parentId) ? group.parentId : null
}

/** The groups directly inside `parentId` (null: directly in the category), in the order given. */
export function childGroups(groups: Group[], parentId: string | null): Group[] {
  return groups.filter((g) => parentOf(g, groups) === parentId)
}

/** "Parent / Child": the group's name after the names of the groups it is in. */
export function groupPath(group: Group, groups: Group[]): string {
  const names = [group.name]
  const seen = new Set([group.id])
  let up = parentOf(group, groups)
  while (up && !seen.has(up)) {
    seen.add(up)
    const g = groups.find((x) => x.id === up)
    if (!g) break
    names.unshift(g.name)
    up = parentOf(g, groups)
  }
  return names.join(' / ')
}

/** The ids of a group and of every group inside it, at any depth. */
export function groupAndInside(group: Group, groups: Group[]): Set<string> {
  const ids = new Set([group.id])
  for (let grew = true; grew; ) {
    grew = false
    for (const g of groups)
      if (g.parentId && ids.has(g.parentId) && !ids.has(g.id)) {
        ids.add(g.id)
        grew = true
      }
  }
  return ids
}

/** Groups for a picker: each group followed by the groups inside it, labelled with its path. */
export function groupOptions(groups: Group[]): { group: Group; label: string }[] {
  const out: { group: Group; label: string }[] = []
  const seen = new Set<string>()
  const visit = (parentId: string | null) => {
    for (const g of childGroups(groups, parentId)) {
      if (seen.has(g.id)) continue
      seen.add(g.id)
      out.push({ group: g, label: groupPath(g, groups) })
      visit(g.id)
    }
  }
  visit(null)
  return out
}

/** External mode: a group name with " / " in it, as *Suggest groups* gives nested groups (FR-KN-EXT-13). */
export function isGroupPath(name: string): boolean {
  return readSettings().knowledgeMode === 'external' && name.split(' / ').filter((n) => n.trim()).length > 1
}

/**
 * Makes the groups of "A / B / C" that are missing, each inside the one before, and returns the id of
 * the last. `description` goes to the last one when it is new.
 */
export async function ensureGroupPath(categoryId: string, path: string, description?: string | null): Promise<string> {
  const names = path
    .split(' / ')
    .map((n) => n.trim())
    .filter(Boolean)
  const groups = await commands.listGroups(categoryId)
  let parent: string | null = null
  for (const [i, name] of names.entries()) {
    const have = groups.find((g) => (g.parentId ?? null) === parent && g.name.toLowerCase() === name.toLowerCase())
    if (have) {
      parent = have.id
      continue
    }
    const made: Group = await commands.createGroup({
      categoryId,
      name,
      description: i === names.length - 1 ? description || null : null,
      ...(parent ? { parentId: parent } : {}),
    })
    groups.push(made)
    parent = made.id
  }
  if (!parent) throw new Error(getT().knowledge.groupDialog.needsName)
  return parent
}
