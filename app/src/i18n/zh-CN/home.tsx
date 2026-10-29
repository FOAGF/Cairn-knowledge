import type { Messages } from '../en'

export const home: Messages['home'] = {
  greeting: {
    morning: '早上好',
    afternoon: '下午好',
    evening: '晚上好',
  },
  greetingNamed: (greeting, name) => `${greeting}，${name}`,
  loading: '加载中…',
  counts: {
    active: (num) => <>活跃 {num}</>,
    blocked: (num) => <>受阻 {num}</>,
    dueThisWeek: (num) => <>本周到期 {num}</>,
    overdue: (num) => <>逾期 {num}</>,
    doneThisWeek: (num) => <>本周完成 {num}</>,
    pages: (num) => <>页面 {num}</>,
    categories: (num) => <>分类 {num}</>,
  },
  attention: {
    title: '需要注意',
    notChecked: (n) => `${n} 个页面未检查`,
    backupFailed: (error) => `上次备份失败：${error}`,
    checkFailed: '上次备份前的数据库检查失败',
    noModel: '未设置 AI 模型',
  },
  today: {
    title: '今天',
    description: (date, n) => `${date} · ${n === 0 ? '没有需要你处理的事' : `${n} 个任务需要你处理`}`,
    empty: '没有逾期、今天到期、进行中或受阻的任务。',
    allTasks: '全部任务',
    group: {
      overdue: '逾期',
      due_today: '今天到期',
      in_progress: '进行中',
      blocked: '受阻',
    },
    showMore: (n) => `再显示 ${n} 个`,
    allInTasks: (total) => `在任务中查看全部 ${total} 个`,
  },
  day: {
    due: (n) => `${n} 个到期`,
    back: '返回今天',
    empty: '那天没有到期的任务。',
  },
  calendar: {
    title: '日历',
    hint: '点击某一天，查看当天到期的任务。',
    tasksDue: (n) => `${n} 个任务到期`,
  },
  firstStart: {
    title: '欢迎使用 Cairn',
    description: '这里还没有任何数据。你想如何开始？',
    fresh: '全新开始',
    import: '从备份或导出文件导入…',
    keysTitle: '需要重新输入 API 密钥',
    keysText: '备份和导出文件从不包含 API 密钥。导入后，请在“设置 › AI 服务商”中重新输入密钥。',
  },
}
