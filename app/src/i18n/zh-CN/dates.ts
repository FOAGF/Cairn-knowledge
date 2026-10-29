import type { Messages } from '../en'

export const dates: Messages['dates'] = {
  today: '今天',
  tomorrow: '明天',
  overdue: (days) => `逾期 ${days} 天`,
  pattern: {
    shortDay: 'M月d日 EEE',
    day: 'yyyy年M月d日 EEE',
    date: 'yyyy年M月d日',
    dateTime: 'yyyy年M月d日 HH:mm',
    weekdayDayMonth: 'M月d日 EEEE',
    longDate: 'yyyy年M月d日 EEEE',
  },
}
