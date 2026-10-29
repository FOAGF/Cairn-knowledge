import type { Messages } from '../en'

/** A task's label and a page's tag are both 标签. */
const namePicker: Messages['shared']['namePicker']['label'] = {
  placeholder: '新标签或已有标签',
  toAdd: '要添加的标签',
  existing: '已有标签',
  toRemove: '选择要移除的标签',
  list: '标签',
  isNew: (name) => `“${name}”是新标签。`,
}

export const shared: Messages['shared'] = {
  bulk: {
    toolbar: '选择',
    selected: (n) => `已选 ${n} 项`,
    clear: '清除选择',
    skippedTitle: (n) => `${n} 项保持原样`,
    skippedDescription: (done) => `${done ? `${done}。` : '没有任何更改。'}以下项目无法更改：`,
    gone: '已不存在的项目',
    items: '要更改的项目',
    working: '处理中…',
    countdown: (label, seconds) => `${label}（${seconds}）`,
  },
  namePicker: { label: namePicker, tag: namePicker },
  colorPicker: {
    custom: '自定义颜色',
    customHex: '自定义颜色（十六进制）',
    hint: '选择一种颜色，或用色轮或十六进制值选择任意颜色。',
    colors: { blue: '蓝色', green: '绿色', purple: '紫色', amber: '琥珀色', rose: '玫红色', slate: '灰色' },
  },
  fold: {
    showMore: (n, noun) => `再显示 ${n} 个${noun}`,
  },
  glossaryTerm: {
    global: '全局',
    unknownCategory: '未知分类',
    also: (names) => `又称：${names.join('、')}`,
    open: '在术语表中打开',
  },
  markdown: {
    taskNotFound: '找不到任务',
    pageNotFound: '找不到页面',
    page: '页面',
    loadingImage: '正在加载图片',
    image: (name) => `[图片：${name}]`,
    inFolder: (path) => `在知识文件夹中：${path}`,
  },
  meaning: {
    partial: (ready, items) => `语义搜索已覆盖 ${ready}/${items} 项（正在建立索引…）`,
    unavailable: '语义搜索不可用，仅显示关键词结果',
  },
  placeholder: '此页面正在开发中。',
  savedAs: {
    rules: '页面是文件，所以文件名遵循 Windows 的规则：< > : " / \\ | ? * 会变成 -，开头和结尾的点和空格会被去掉。',
    label: (name) => <>保存为：{name}</>,
  },
  taskStatus: {
    saveKnowledge: '要从此任务保存知识吗？',
    createKnowledge: '创建知识',
    change: (status) => `状态：${status}。更改状态`,
  },
  tint: {
    remove: (tag) => `移除 ${tag}`,
    more: (names) => `另外 ${names.length} 个：${names.join('、')}`,
    priority: (priority) => `优先级：${priority}`,
  },
  ui: {
    commandTitle: '命令面板',
    commandDescription: '搜索要运行的命令…',
    notifications: '通知',
    closeToast: '关闭通知',
  },
}
