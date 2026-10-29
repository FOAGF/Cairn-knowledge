import type { Messages } from '../en'

export const editor: Messages['editor'] = {
  mode: {
    label: '编辑器模式',
    preview: '预览',
  },
  hints: {
    button: '提示',
    title: 'Cairn 中的 Markdown',
    pageLink: '链接页面：输入 [[ 并选择一个页面。链接显示为页面的当前标题，所以重命名页面不会让链接失效。',
    taskMention: '用编号提及任务：输入 # 并选择一个任务。任务移到其他分类后，提及仍然有效。',
    images: '图片',
    imagesText: '粘贴或拖入截图即可添加。源文本中显示为 ![](attachment:…)，预览中显示图片。',
    folderImagesText: '粘贴或拖入截图即可添加。图片保存在分类文件夹的 attachments 文件夹中；源文本中显示为 ![](folder:…)，预览中显示图片。',
    glossaryTerms: '术语',
    glossaryText: '照常书写即可。已知的工作术语带有虚线下划线，悬停时显示释义。',
    latex: 'LaTeX 公式：行内用 $…$，居中的公式把 $$ 各放在单独一行；也可以用 \\(…\\) 和 \\[…\\]。预览中会显示公式。$5 这样的金额仍是普通文本。',
    rest: (
      <>
        其余都是标准 Markdown：<code># 标题</code>（# 后有空格）、<code>**粗体**</code>、<code>*斜体*</code>、列表、<code>- [ ]</code> 清单、表格、
        <code>`代码`</code> 和 <code>&gt; 引用</code>。
      </>
    ),
  },
  previewEmpty: '还没有可预览的内容。',
  restoreDraft: (ago) => `恢复 ${ago}未保存的更改？`,
  image: {
    saving: (n) => `[正在保存图片… #${n}]`,
    saveFailed: (message) => `无法保存图片：${message}`,
    dropOnlyImages: '只能把 PNG 或 JPEG 图片拖入文本。',
  },
  glossary: {
    global: '全局',
    otherNames: (names) => `又称：${names.join('、')}`,
  },
  lint: {
    addToDictionary: '添加到词典',
    remove: '（删除）',
    spelling: '拼写',
    grammar: '语法',
  },
  pageChip: '页面',
}
