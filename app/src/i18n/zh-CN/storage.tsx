import type { Messages } from '../en'

export const storage: Messages['storage'] = {
  pickFolder: {
    knowledge: '选择存放知识页面的文件夹',
    anotherKnowledge: '为知识页面选择另一个文件夹',
    onlyInApp: '只能在 Cairn 应用中选择文件夹。',
  },
  notWatched: {
    title: '无法实时获取其他程序所做的更改',
    toast: (error) => `${error} 在“设置 › 常规”中重新扫描即可读取这些更改。`,
  },
  // `step` is usually English (from Rust), so the colon stays half-width.
  progress: (step, done, total) => `${step}: ${done} / ${total}`,
  starting: '正在开始',
  scan: {
    reading: '正在读取知识文件夹',
    failed: (error) => `无法读取知识文件夹：${error}`,
    skipped: (n) => `知识文件夹中有 ${n} 个文件未编入索引`,
    skippedDescription: (list, more) => `${list}${more > 0 ? ` …另有 ${more} 个` : ''}。可在“设置 › 常规”中查看全部。`,
  },
  modeChooser: {
    welcome: '欢迎使用 Cairn',
    intro: '首先，选择知识页面的存放位置。无论哪种方式，任务的用法都一样。',
    aria: '知识页面的存放位置',
    internal: '内部模式',
    internalBody: '页面保存在 Cairn 自己的数据库中，位于它的数据文件夹里。',
    internalTogether: '所有内容集中在一起：备份和导出包含每个页面。',
    internalOtherPrograms: '其他程序无法直接打开这些页面。你可以随时把它们导出为 Markdown 文件。',
    external: '外部模式',
    externalBody: '页面是你选择的文件夹中的 .md 文件。Cairn 会为它建立索引，用于搜索、链接和助手。',
    externalFolders: '其中的每个文件夹是一个分类，再下一层的文件夹是分组。',
    externalTools: '其他工具（文本编辑器、Obsidian、git、OneDrive）可以读取和修改同样的文件，Cairn 会获取它们所做的更改。',
    externalBackups: 'Cairn 的备份不包含这个文件夹：请用你自己的工具妥善保管。',
    noFolder: '未选择文件夹',
    change: '更改…',
    chooseFolder: '选择文件夹…',
    sample: '使用示例文件夹（开发）',
    laterTitle: '以后可以更改',
    laterBody: '无论哪种方式，任务、分类、术语表和对话都保存在 Cairn 的数据库中。可以在“设置 › 常规”中在两者之间切换；每次切换前都会先备份知识页面。',
    useFolder: '使用此文件夹',
    chooseFirst: '请先选择文件夹',
    keepInCairn: '把页面保存在 Cairn 中',
    chooseOne: '选择一种方式以继续',
  },
  switchDialog: {
    toExternal: '正在切换到外部模式',
    toFolder: '正在切换到另一个文件夹',
    toInternal: '正在切换到内部模式',
    switching: '正在切换知识的存放位置',
    failed: '切换未完成',
    nowInternal: '现在使用内部模式',
    nowFolder: '现在使用新文件夹',
    nowExternal: '现在使用外部模式',
    keepOpen: '请保持 Cairn 打开，直到完成。任务和其他内容都不会改变。',
    backupKept: (file, size) => (
      <>
        切换前的知识页面备份保存在“切换备份”中：{file}（{size}）。
      </>
    ),
    internalResult: (restored) => `${restored > 0 ? `已从切换备份载入 ${restored} 个页面。` : '从空白开始，没有页面。'}文件夹保持原样。`,
    pagesInFolder: (n) => `文件夹中有 ${n} 个页面。`,
    detailsBack: (n) => `已从较早的切换备份中为 ${n} 个页面恢复 Cairn 自己的信息（检查、任务链接、历史记录）。`,
    skipped: (n) => `${n} 个文件未编入索引；“设置 › 常规”中列出了这些文件。`,
    previousLeft: '之前的文件夹保持原样。以后切换回去时，Cairn 会恢复其中页面的信息。',
    searchRebuilt: '语义搜索会在后台重建。关键词搜索现在就能用。',
  },
  backupWarning: {
    title: (size) => `切换备份占用了 ${size}`,
    body: (warnSize) =>
      `Cairn 会在每次切换时备份知识页面：每个文件夹保留最新的一个，内部模式的则全部保留。它们的总大小现已超过 ${warnSize}。删除不再需要的备份即可释放空间。`,
    skip: '暂时跳过',
    openFolder: '打开文件夹',
    openList: '打开切换备份',
  },
}
