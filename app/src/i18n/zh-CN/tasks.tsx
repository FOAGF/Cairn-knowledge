import type { Messages } from '../en'

export const tasks: Messages['tasks'] = {
  markdownHint: '支持 Markdown。输入 # 提及任务，输入 [[ 链接页面。',
  page: {
    count: (n, filtered) => (filtered ? `${n} 个任务符合筛选条件` : `${n} 个任务`),
    view: '视图',
    listView: '列表视图',
    boardView: '看板视图',
    list: '列表',
    board: '看板',
    search: '搜索任务',
    all: (filter) => `${filter}：全部`,
    active: '活跃（进行中 + 受阻）',
    label: '标签',
    due: '截止日期',
    dueOptions: { overdue: '已逾期', today: '今天截止', week: '本周截止', none: '无截止日期' },
    clear: '清除',
    sortBy: '排序方式',
    sort: '排序：',
    sortOptions: { due: '截止日期', priority: '优先级', created: '创建时间', updated: '更新时间', key: '编号' },
    showClosed: '显示已关闭',
    noMatch: '没有匹配的任务',
    emptyHint: (filtered, showClosed) =>
      filtered
        ? showClosed
          ? '试试移除某个筛选条件。'
          : '试试移除某个筛选条件，或打开“显示已关闭”查看已关闭的任务。'
        : showClosed
          ? '这里还没有任务。'
          : '这里还没有任务。打开“显示已关闭”可查看已关闭的任务。',
  },
  list: {
    selectAll: '全选显示的任务',
    selectedOf: (ticked, shown) => `已选 ${ticked} / ${shown} 个`,
    selectAllShown: (shown) => `全选显示的任务（${shown}）`,
    select: (key) => `选择 ${key}`,
  },
  board: {
    expand: (status) => `展开${status}`,
    collapse: (status) => `收起${status}`,
    dropHere: '把任务拖到这里',
    dropTo: (status) => `拖至${status}`,
  },
  bulk: {
    addLabel: '添加标签',
    removeLabel: '移除标签',
    same: '每个任务的历史记录和动态都会记下这次更改，与单独更改一个任务时一样。',
    statusTitle: (n, status) => `将 ${n} 个任务设为“${status}”？`,
    statusDescription: (pill, same) => (
      <>
        每个任务都将变为 {pill}。{same}它们当前的状态：
      </>
    ),
    setStatus: (status) => `设为${status}`,
    statusDone: (n, status) => `已将 ${n} 个任务设为“${status}”`,
    priorityTitle: (n, priority) => `将 ${n} 个任务的优先级设为“${priority}”？`,
    priorityDescription: (icon, same) => (
      <>
        每个任务都将变为 {icon}。{same}它们当前的优先级：
      </>
    ),
    setPriority: (priority) => `设为${priority}优先级`,
    priorityDone: (n, priority) => `已将 ${n} 个任务的优先级设为“${priority}”`,
    moveTitle: (n, category) => `将 ${n} 个任务移到“${category}”？`,
    moveDescription: (category) => `每个任务都会在“${category}”中获得新编号；旧编号在提及和链接中仍然有效。`,
    moveDone: (n, category) => `已将 ${n} 个任务移到“${category}”`,
    addLabelTitle: (n) => `为 ${n} 个任务添加标签`,
    addLabelDescription: '输入一个标签，或从已有标签中选择。已有该标签的任务不会重复添加。',
    addLabelDone: (label, n) => `已为 ${n} 个任务添加标签“${label}”`,
    removeLabelTitle: (n) => `从 ${n} 个任务中移除标签`,
    removeLabelDescription: '没有该标签的任务保持不变。',
    removeLabelDone: (label, n) => `已从 ${n} 个任务中移除标签“${label}”`,
    deleteTitle: (n) => `删除 ${n} 个任务？`,
    deleteDescription: '这些任务将连同评论、历史记录和链接被永久删除。此操作无法撤销，只能从备份中找回。它们的编号不会再被使用。',
    deleteConfirm: (n) => `删除 ${n} 个任务`,
    deleteDone: (n) => `已删除 ${n} 个任务`,
  },
  detail: {
    titleLabel: '任务标题',
    noDescription: '暂无描述。',
    details: '详情',
    created: (date) => `创建于 ${date}`,
    completed: (date) => `完成于 ${date}`,
    notFound: (key) => `找不到任务 ${key}`,
    loadFailed: '无法加载该任务',
    maybeDeleted: '它可能已被删除。',
    backToAll: '返回所有任务',
    createKnowledge: '从任务创建知识',
    deleteTask: '删除任务',
    updates: (n) => `进展（${n}）`,
    history: (n) => `历史记录（${n}）`,
    deleteTitle: (key) => `删除 ${key}？`,
    deleteDescription: (title) => `“${title}”及其评论、链接和历史记录将被删除。此操作无法撤销。`,
    deleted: (key) => `已删除 ${key}`,
  },
  links: {
    remove: (key) => `移除与 ${key} 的链接`,
    add: '添加链接',
    addDescription: (key) => `将 ${key} 链接到任意任务，包括其他分类中的任务。`,
    relatesTo: '关联',
    parentOf: '是父任务（所选任务成为子任务）',
    subtaskOf: '是子任务（所选任务成为父任务）',
    search: '按编号或标题搜索',
    noTasks: '没有匹配的任务。',
    pageTitle: '链接知识页面',
    pageDescription: (key) => `选择与 ${key} 相关的页面。`,
    searchPages: '搜索页面',
    noPages: '没有匹配的页面。',
    taskLinks: '任务链接',
    noLinkedTasks: '没有链接的任务。',
    parent: '父任务',
    subtasks: '子任务',
    knowledgePages: '知识页面',
    linkPage: '链接页面',
    noLinkedPages: '没有链接的页面。',
    unlink: (title) => `取消链接 ${title}`,
    mentionedIn: '被提及于',
    comment: '评论',
  },
  newTask: {
    description: '编号会根据分类自动分配。',
    titlePlaceholder: '需要做什么？',
    create: '创建任务',
    created: (key) => `已创建 ${key}`,
  },
  history: {
    none: '无',
    empty: '暂无历史记录。',
    created: (title) => `任务已创建，标题为“${title}”。`,
    noChanges: '没有字段变化。',
    edited: '已编辑',
  },
  comments: {
    edited: '· 已编辑',
    edit: '编辑评论',
    delete: '删除评论',
    created: '已创建',
    completed: '已完成',
    statusTo: (status) => (
      <>
        状态 →{' '}
        {status}
      </>
    ),
    showStatus: '显示状态变化',
    addTitle: '添加进展',
    placeholder: '有什么进展？这里可以使用 Markdown 和 #KEY 提及。',
    add: '添加',
  },
  completion: {
    title: '完成总结',
    spent: () => '工作日',
    blocked: () => '受阻天数',
    note: '按日期计算，包括第一天和最后一天，只算周一至周五，不考虑公共假期。',
  },
  labels: {
    clearDate: '清除日期',
    remove: (label) => `移除标签 ${label}`,
    placeholder: '添加标签后按 Enter',
    suggestions: '建议：',
  },
}
