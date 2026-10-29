import type { Messages } from '../en'

export const labels: Messages['labels'] = {
  status: {
    todo: '待办',
    in_progress: '进行中',
    blocked: '受阻',
    done: '已完成',
    wont_do: '不处理',
  },
  priority: { high: '高', medium: '中', low: '低' },
  type: { bug: 'Bug', feature: '功能', general: '常规' },
  origin: {
    manual: '手动编写',
    file: '从文件创建',
    task: '从任务创建',
    folder: '在文件夹中找到',
  },
}
