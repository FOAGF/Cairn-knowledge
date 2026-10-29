import type { Messages } from '../en'

export const layout: Messages['layout'] = {
  topNav: {
    search: '搜索…',
    lightTheme: '浅色外观',
    darkTheme: '深色外观',
    me: '我',
  },
  askBar: {
    placeholder: '随便问点什么：搜索你的知识、查找信息，或获取任务方面的帮助。',
    ask: '提问',
  },
  palette: {
    description: '搜索任务和知识',
    placeholder: '搜索任务、页面，或输入命令…',
    noResults: '没有结果。',
    actions: '操作',
    goTo: '转到',
    newPage: '新建知识页面',
    askAssistant: '询问助手',
    byMeaning: '语义匹配',
    searchingByMeaning: '正在按语义搜索…',
  },
  shell: {
    browserPreview: (
      <>
        浏览器预览：这里无法获取 Rust 核心的数据。请用 <code>cargo tauri dev</code> 运行应用。
      </>
    ),
  },
}
