# document-render 写文档指南（给 AI agent）

读完这一份就能写出能通过 `check` 的文档 JSON。字段名、取值、默认值都取自源码（`src/components.rs`、`src/style.rs`、`src/doc.rs`、`src/cascade.rs`、`src/custom.rs`）。拿不准时以 `list_components` / `document-render catalog --json` 的输出为准。

## 1. 是什么，怎么调用

document-render 把一份 JSON 文档排成 docx、pdf、html 或 Markdown（md）。JSON 里只写组件（标题、正文、表格……）和样式，不写排版细节。也能把 docx 解析回同样格式的 JSON。

推荐流程：写 JSON → `check` → 有错按错误信息改 → `render`。

### 命令行

| 命令 | 作用 |
|---|---|
| `document-render render doc.json -o out.docx` | 按扩展名选格式：`.docx` `.pdf` `.html`/`.htm` `.md`/`.markdown`；`-f/--format` 优先 |
| `document-render render doc.json -o - -f html` | 输出写 `-` 时必须给 `--format` |
| `document-render render doc.json -o out.html --paged` | html 内嵌 Paged.js，按页显示 |
| `document-render check doc.json` | 只校验，成功打印 `ok：展开后 N 个元素`；公式解析不了打警告 |
| `document-render catalog [--json] [--catalog 目录]` | 列组件；`--json` 带 props 的 JSON Schema、`container`、`parts` |
| `document-render parse in.docx -o out.json [--assets 目录]` | docx 解析成 JSON，图片默认导到 `out.assets/` |
| `document-render guide` | 打印本文 |
| `document-render mcp` | 以 MCP server 运行（stdio） |

`render`、`check` 都认 `--catalog <文件或目录>`（自定义组件，可写多次）、`--style-sets <文件或目录>`（样式集，可写多次）、`--style-set <名字>`（选样式集，优先于文档里的 `styleSet`）。输入文件写 `-` 表示读标准输入。文档目录下的 `catalog/`、`styles/` 会自动读。

### MCP 工具

| 工具 | 参数 | 返回 |
|---|---|---|
| `render` | `document`（必填，对象或 JSON 字符串）、`output`、`format`（html/docx/pdf/md）、`paged`、`style_set`、`style_sets`、`catalog`、`base_dir` | `path`、`format`、`bytes`、`warnings`；不写 `output` 时只能出 html/md，返回 `content` |
| `check` | `document`（必填）、`catalog`、`style_sets`、`style_set`、`base_dir` | `ok`、`errors`、`warnings`、`elements` |
| `list_components` | `catalog` | 组件名 → `{ description, container, parts, props, custom }` |
| `parse` | `input`（必填，docx 路径）、`output`、`assets` | 写了 `output` 返回 `path`；没写返回 `document` 和 `base_dir`；都带 `assets`、`warnings` |
| `guide` | 无 | 本文全文（`text`） |

- 本文也是 MCP 资源 `document-render://agent-guide`（`text/markdown`），用 `resources/read` 读。
- `catalog`、`style_sets` 写一个路径或路径数组。路径一律写绝对路径。
- `base_dir` = 文档所在目录：相对路径的图片按它找，它下面的 `catalog/`、`styles/` 自动读。
- `check` 发现文档有错返回 `ok: false`，不算工具出错；参数本身不对才返回 `isError`。

## 2. 文档结构

顶层字段（多写别的字段会报错）：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| `version` | 字符串 | 否 | 写 `"0.1"` |
| `meta` | 对象 | 否 | `title`、`author`、`lang`（如 `"zh-CN"`）、`page`（见第 4.8 节） |
| `styles` | 对象 | 否 | 键是组件名（`Heading.1`、`Table.header`）时是组件样式，否则是命名样式 |
| `styleSet` | 字符串 | 否 | 用哪个样式集 |
| `styleSets` | 对象 | 否 | 名字 → 样式集定义（第 4.10 节） |
| `components` | 数组 | 否 | 内联的自定义组件定义（第 5 节） |
| `root` | 字符串 | 是 | 根元素 id，一般指向 `Document` |
| `elements` | 对象 | 是 | id → 元素，全部平铺 |

元素字段（多写会报错）：

| 字段 | 说明 |
|---|---|
| `type` | 组件名，必填。内置组件或自定义组件 |
| `props` | 组件参数。多写、少写必填、类型不对都报错。没有 props 的组件可以不写 |
| `style` | 样式名 `"muted"`、样式名数组 `["muted", "center"]`（后面覆盖前面，数组里也能放对象）、样式对象 `{ "fontSize": 11 }` |
| `parts` | 部位样式，键是部位名，值同 `style` |
| `children` | 子元素 id 数组，只有容器组件（`Document`、`Section`、`Slide`、容器型自定义组件）能写 |

引用规则：

- 每个元素只能被引用一次（children 或页眉页脚），引用两次或成环报错。
- children 里的 id 必须在 `elements` 里，否则报「元素 `x` 不存在」。
- 没被引用的元素不报错，只打警告，也不渲染。别留孤儿元素。
- 页眉页脚元素写在 `elements` 里，由 `meta.page.header` 或 `Section` 的 `header` 引用，不能再放进 children。
- 元素 id 随便起，别带 `.`（展开自定义组件时 `.` 有特殊用途）。

最小可用示例：

<!-- check -->
```json
{
  "version": "0.1",
  "meta": { "title": "季度报告", "lang": "zh-CN", "page": { "size": "A4", "margin": "20mm" } },
  "styles": { "muted": { "color": "#666666", "fontSize": 10 } },
  "root": "doc",
  "elements": {
    "doc": { "type": "Document", "children": ["title", "intro"] },
    "title": { "type": "Heading", "props": { "level": 1, "text": "季度报告" } },
    "intro": { "type": "Text", "props": { "text": "一份 JSON，出三种格式。" }, "style": "muted" }
  }
}
```

## 3. 组件

总表（`容器` = 能写 children）：

| 组件 | 容器 | parts | 必填 props |
|---|---|---|---|
| Document | 是 | 无 | 无 |
| Heading | 否 | 无（用 `Heading.1`…`Heading.6` 区分级别） | `level`、`text` |
| Text | 否 | 无 | `text` |
| Code | 否 | 无 | `code` |
| Math | 否 | 无 | `latex` |
| List | 否 | `level1` `level2` `level3` `level4` `level5` `level6` `level7` `level8` `level9` `marker` `item` | `items` |
| Table | 否 | `table` `cell` `header` `firstRow` `lastRow` `firstColumn` `lastColumn` `oddRow` `evenRow` `oddColumn` `evenColumn` `topLeftCell` `topRightCell` `bottomLeftCell` `bottomRightCell` | `rows` |
| Image | 否 | `caption` | `src` |
| PageBreak | 否 | 无 | 无 |
| ColumnBreak | 否 | 无 | 无 |
| Divider | 否 | 无 | 无 |
| Quote | 否 | 无 | `text` |
| Callout | 否 | 无 | `text` |
| TableOfContents | 否 | `title`、`entry` | 无 |
| Section | 是 | 无 | 无 |
| Slide | 是 | `title`、`body` | 无 |

`InlineCode`、`InlineMath` 不是元素，只能写在 runs 里（见 3.17），但可以当组件样式的键。

### 3.1 文字类 props（inline）

Heading/Text/Quote/Callout 的 `text`、List 的每一项、Table 的每一格都是「文字」，三种写法：

```json
"text": "纯字符串"
"text": ["字符串", { "text": "加粗", "bold": true }, "再接字符串"]
"text": [{ "text": "整段都是 run 对象" }]
```

run 对象的字段见 3.17。

### 3.2 Document

根容器，按顺序排子元素。props 为空对象，可以不写。它的 `style` 会被所有后代继承（文字、段落、列表字段）。

```json
"doc": { "type": "Document", "children": ["title", "p1"] }
```

### 3.3 Heading

| prop | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `level` | 整数 1–6 | 是 | — | 超出范围报错 |
| `text` | inline | 是 | — | 标题文字 |

内置样式：粗体、`keepWithNext`；字号 1 级 20、2 级 16、3 级 14、4 级 12、5/6 级 11（pt）。docx 挂 Word「标题 N」样式，目录能收。

```json
"h1": { "type": "Heading", "props": { "level": 2, "text": "一、概况" } }
```

### 3.4 Text

| prop | 类型 | 必填 | 说明 |
|---|---|---|---|
| `text` | inline | 是 | 一段正文。文字里的 `\n` 是段内换行 |

```json
"p1": { "type": "Text", "props": { "text": ["运行 ", { "type": "InlineCode", "text": "cargo test" }, "。"] } }
```

### 3.5 Code

| prop | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `code` | 字符串 | 是 | — | 代码原文，保留空格和换行 |
| `language` | 字符串 | 否 | 无 | 只做标记（html 写进 `class="language-xx"`），不做高亮 |
| `lineNumbers` | 布尔 | 否 | `false` | |

```json
"code": { "type": "Code", "props": { "language": "rust", "code": "fn main() {\n    println!(\"hi\");\n}\n" } }
```

### 3.6 Math

| prop | 类型 | 必填 | 说明 |
|---|---|---|---|
| `latex` | 字符串 | 是 | LaTeX 源码，不写 `$$`。JSON 里反斜杠写两个：`"\\frac{a}{b}"` |
| `number` | 整数或字符串 | 否 | 右侧编号，`1` 显示成「(1)」 |

默认居中，段前段后 6pt。LaTeX 解析不了不报错：`check` 给警告，渲染时显示原文。支持上下标、`\frac`、`\sqrt[3]{}`、`\sum`、`\int`、`\lim`、希腊字母、`\mathbf`、`\mathbb`、`\text`、`\left( \right)`、`pmatrix`/`bmatrix`、`cases`、`\binom`、`\hat`/`\vec`/`\bar`、`\overline`、`\underbrace`。

```json
"eq": { "type": "Math", "props": { "latex": "x = \\frac{-b \\pm \\sqrt{b^2-4ac}}{2a}", "number": 1 } }
```

### 3.7 List

| prop | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `ordered` | 布尔 | 否 | `false` | 有序带编号 |
| `items` | 数组 | 是 | — | 每项是 inline，或 `{ "text": inline, "items": [子项], "ordered": 布尔 }` |

子项对象只认 `text`、`items`、`ordered`；`ordered` 不写跟上一级一样。

每一级的样式按「元素 `style` + `parts.levelN`」算，规则是：

- 元素 `style` 上的大多数字段管所有级别，比如 `listMarkerColor`、`listIndent`、文字字段。
- 决定编号长什么样的五个字段只管第一级：`listStyleType`、`listMarkerFormat`、`listMarkerImage`、`listMarkerFont`、`listStart`。写在元素 `style` 上，就等于写在 `parts.level1` 上。
- 第二级起，这五个字段只看自己的 `parts.levelN`；没写就用这一级的默认：编号类型有序依次 `decimal` `lower-alpha` `lower-roman`，无序依次 `disc` `circle` `square`，三级一轮；模板是 `"%N."`；从 1 开始。

所以元素 `style` 上写 `"listStyleType": "decimal"`，只有第一级是数字，下级照常是 a. b. 或圆点。想改下级，就写 `parts.level2`、`parts.level3`。

`listMarkerFormat` 里 `%k` 是第 k 级的编号，和 Word 一样：二级写 `"%2)"` 或 `"%1.%2"`，写 `"%1"` 会显示一级的编号。指向更深级别的 `%k`（一级里写 `%2`）没有编号可填，会被去掉。符号级别（`disc`、`"▸"` 这类）不用 `listMarkerFormat`，写了也不管，只显示符号。

```json
"steps": {
  "type": "List",
  "props": { "ordered": true, "items": ["准备数据", { "text": "选样式集", "items": ["report", "slides"], "ordered": false }, "渲染"] },
  "style": { "listMarkerColor": "#1A73E8" },
  "parts": { "level1": { "listStyleType": "cjk-decimal", "listMarkerFormat": "%1、" }, "level2": { "listStyleType": "circle" } }
}
```

### 3.8 Table

| prop | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `columns` | 格子数组 | 否 | `[]` | 表头一行；有表头时它就是 `firstRow` |
| `rows` | 格子二维数组 | 是 | — | 数据行 |
| `widths` | 数组 | 否 | 等分 | 每列一个：数字=比例；带单位字符串=固定宽（`"20mm"`，不带单位当 pt）；`"auto"`=按内容 |
| `look` | 对象 | 否 | 见下 | 哪些部位生效 |
| `rowHeights` | 数组 | 否 | 无 | 每行一个（有表头时表头是第一项）：长度=最小高；`{ "height": "20pt", "rule": "exact" }`=固定高（`rule` 只能 `atLeast`/`exact`）；`null`=不管 |

`look` 字段和默认值：`firstRow: true`、`lastRow: false`、`firstColumn: true`、`lastColumn: false`、`bandedRows: true`、`bandedColumns: false`。部位样式只在对应开关打开时生效：用 `lastRow`、`lastColumn` 部位要写 `look.lastRow`/`look.lastColumn: true`，用 `oddColumn`/`evenColumn` 要开 `bandedColumns`。

格子写法：字符串、run 数组，或对象：

| 格子对象字段 | 说明 |
|---|---|
| `text`（别名 `runs`） | inline |
| `rowSpan` | 往下合并几行，含自己，默认 1 |
| `colSpan` | 往右合并几列，含自己，默认 1 |
| `style` | 只给这一格的样式（名字、数组或对象），叠在部位样式之后 |

合并规则：被合并掉的位置在后面的行里不写（同 HTML），所以那一行会少几格。只要有一列是比例，表格撑满版心；全是固定列和 `auto` 列时宽度等于各列之和。

表头默认粗体、`#F3F3F3` 底。去掉灰底：`"parts": { "header": { "background": "none" } }`。部位叠加顺序：`table` → `cell` → 奇偶列 → 奇偶行 → 首末列 → 首末行 → 四角。

```json
"t": {
  "type": "Table",
  "props": {
    "columns": ["地区", "季度", "收入"],
    "rows": [
      [{ "text": "华东", "rowSpan": 2 }, "Q1", "120"],
      ["Q2", "135"],
      [{ "text": "合计", "colSpan": 2, "style": { "bold": true } }, "255"]
    ],
    "widths": [2, 1, 1],
    "look": { "lastRow": true }
  },
  "parts": { "evenRow": { "background": "#F5F7FA" }, "lastRow": { "borderTop": "1pt solid #333333" } }
}
```

### 3.9 Image

| prop | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `src` | 字符串 | 是 | — | 本地路径（相对文档目录 / `base_dir`）或网址 |
| `alt` | 字符串 | 否 | `""` | 替代文字；放不进去时显示 `[alt]` |
| `width` | 数字 | 否 | 原尺寸 | pt，只能写数字，不能写 `"100pt"` |
| `height` | 数字 | 否 | 按比例 | pt，只能写数字 |
| `caption` | 字符串 | 否 | 无 | 图下说明，样式走 `caption` 部位（默认 9pt 灰色） |

位置、绕排用样式字段（4.5 节）。docx 只收 png/jpeg/gif/webp；SVG、远程图片在 docx 里变成 alt 文字加警告，pdf 支持 SVG 不支持远程图。

```json
"fig": { "type": "Image", "props": { "src": "images/chart.png", "alt": "收入走势", "width": 300, "caption": "图 1 收入走势" }, "style": { "align": "center" } }
```

### 3.10 PageBreak / ColumnBreak / Divider

都没有 props，写 `{ "type": "PageBreak" }` 即可。

- `PageBreak`：分页。
- `ColumnBreak`：分栏的节里跳到下一栏；不分栏时 docx、pdf 当分页，html 忽略，md 丢掉。
- `Divider`：一条横线，外观由 `borderBottom` 决定（默认 `0.5pt solid #D0D7DE`）。

页眉页脚里不能放 PageBreak、ColumnBreak。

### 3.11 Quote

| prop | 类型 | 必填 | 说明 |
|---|---|---|---|
| `text` | inline | 是 | 引用文字，默认左边 3pt 竖线、灰字 |

```json
"q": { "type": "Quote", "props": { "text": "导出 Word 和 PDF 终于一模一样了。" } }
```

### 3.12 Callout

| prop | 类型 | 必填 | 说明 |
|---|---|---|---|
| `text` | inline | 是 | 提示文字 |
| `icon` | 字符串 | 否 | 放在最前面的图标（emoji 或字符） |

默认浅灰底、细边框、8pt 内边距。md 里写成 `> [!NOTE]`。

```json
"tip": { "type": "Callout", "props": { "icon": "💡", "text": "收入按到账日期统计。" }, "style": { "background": "#FFF8E1" } }
```

### 3.13 TableOfContents

| prop | 类型 | 必填 | 默认 | 说明 |
|---|---|---|---|---|
| `title` | 字符串 | 否 | 不显示 | 目录上方的标题，样式走 `title` 部位 |
| `levels` | 整数 1–6 | 否 | `3` | 收几级标题 |

每一条的样式走 `entry` 部位。docx 写成 TOC 域，打开后更新域才有页码；pdf 自己算好页码。

```json
"toc": { "type": "TableOfContents", "props": { "title": "目录", "levels": 2 } }
```

### 3.14 Section

一节。只能直接放在 root 下面。每节从新的一页开始。props 都可选，没写的沿用上一节（`pageNumberStart` 除外：没写就接着数）。

| prop | 类型 | 说明 |
|---|---|---|
| `size` | `"A3"` `"A4"` `"A5"` `"B5"` `"Letter"` `"Legal"`，或 `{ "width", "height" }` | 纸张 |
| `landscape` | 布尔 | 横向 |
| `margin` | 长度 | 四边页边距 |
| `marginTop` `marginRight` `marginBottom` `marginLeft` | 长度 | 单边，覆盖 `margin` |
| `gutter` | 长度 | 装订线 |
| `columns` | 数字，或 `{ "count", "gap" }` | 分栏，默认栏距 21pt |
| `headerDistance` `footerDistance` | 长度 | 页眉/页脚离纸边，默认 0.5in |
| `header` `footer` | 元素 id，或 `{ "default", "first", "even" }`，或 `null` | 见下 |
| `pageNumberStart` | 整数 | 本节页码从几开始 |
| `pageNumberFormat` | `decimal` `lowerRoman` `upperRoman` `lowerLetter` `upperLetter` `chineseCounting` | 页码数字格式 |

页眉页脚规则：

- 写一个 id = 只设 `default`（普通页）。
- 对象里 `first` 是本节首页，`even` 是偶数页；某一项写 `null` 表示留空，不写表示沿用上一节。
- 设了 `first`（含 `null`）就开「首页不同」；任何一节设了 `even` 就整份文档开「奇偶页不同」。
- 在 Section 上写 `"header": null` 清掉从上一节接过来的全部页眉。
- `first` 也会沿用：封面一节写了 `"first": null`，下一节首页照样空白。下一节要显示，就重写 `first`：`{ "default": "hdr", "first": "hdr" }`。同一个元素可以同时用在 `default` 和 `first`。

root 下面的排法：全不用 Section（整份一节，设置在 `meta.page`）；或者全是 Section；或者开头一段不在 Section 里（算第一节，用 `meta.page`），后面全是 Section。Section 后面不能再跟散的元素。

```json
"body": { "type": "Section", "props": { "pageNumberStart": 1, "pageNumberFormat": "decimal", "footer": "ftr" }, "children": ["h2", "p2"] }
```

### 3.15 Slide

| prop | 类型 | 必填 | 说明 |
|---|---|---|---|
| `title` | 字符串 | 否 | 幻灯片标题，样式走 `title` 部位 |

16:9 一页，子元素排在里面（`body` 部位）。html/pdf 一页一张；docx 变成一个二级标题加正文，后面分页。

```json
"s1": { "type": "Slide", "props": { "title": "封面" }, "children": ["s1-text"] }
```

### 3.16 自定义组件

`list_components` 里带 `custom: true` 的是自定义组件。用法和内置组件一样：`type` 写组件名，`props` 按它的 schema 写；容器型的才能写 `children`。定义方法见第 5 节。

### 3.17 runs 和行内元素

run 对象字段（多写会报错）：

| 字段 | 类型 | 说明 |
|---|---|---|
| `text` | 字符串 | 文字；写了 `field` 或 `image` 时可省 |
| `type` | `"InlineCode"` / `"InlineMath"` | 行内代码 / 行内公式（`text` 写 LaTeX，不写 `$`） |
| `href` | 字符串 | 链接；默认蓝色 `#0563C1` 加下划线 |
| `field` | `"page"` `"pages"` `"sectionPages"` | 当前页码 / 总页数 / 本节页数；`text` 可写占位值 |
| `image` | 字符串 | 行内图片路径；写了就不显示 `text`（`text` 当替代文字） |
| `width` `height` | 数字 | 行内图片尺寸，pt；只写一个按比例算另一个 |
| `bold` `italic` | 布尔 | 简写 |
| `underline` | 布尔，或 `single` `double` `thick` `dotted` `dashed` `wavy` `none` | 简写 |
| `color` | `#RRGGBB` | 简写 |
| `style` | 样式名、数组或对象 | 只能写文字字段（4.1 节那张表）外加 `background` |

删除线、上下标、字号等没有简写，写在 `style` 里：`{ "text": "H", "style": { "verticalAlign": "sub" } }`。

```json
"p": {
  "type": "Text",
  "props": {
    "text": [
      "见",
      { "text": "官网", "href": "https://example.com" },
      "，命令 ",
      { "type": "InlineCode", "text": "document-render check" },
      "，公式 ",
      { "type": "InlineMath", "text": "E=mc^2" },
      "，图标 ",
      { "image": "images/icon.png", "height": 12, "text": "图标" },
      "，",
      { "text": "旧价", "style": { "strike": true, "color": "#999999" } },
      "。"
    ]
  }
}
```

页脚页码：`"text": ["第 ", { "field": "page" }, " 页，共 ", { "field": "pages" }, " 页"]`。只有 `page` 跟 `pageNumberFormat` 走，`pages`、`sectionPages` 一直是阿拉伯数字。

## 4. 样式

字段平铺在样式对象里，camelCase，拼错报错。某个格式表达不了的字段只给警告，不报错。

### 4.0 取值写法

| 类型 | 写法 |
|---|---|
| 长度 | 数字 = pt；字符串带单位：`pt` `mm` `cm` `in` `px`（96dpi）、`em`（当前字号倍数，`"2em"` 首行缩进两字）、`line`（行数，只用于 `spaceBefore`/`spaceAfter`）、`%`（用于 `width`/`height`） |
| 颜色 | `"#RGB"` `"#RRGGBB"` `"#RRGGBBAA"`，必须带 `#`，不认 `"red"` |
| 布尔 | `true`/`false` |
| 边框 | `"1pt solid #CCCCCC"`、`"none"`，或 `{ "width": 1, "style": "solid", "color": "#CCCCCC" }`；简写里缺的部分默认 1pt、solid、黑色 |
| 背景 | `"#F5F7FA"`；`"none"`（去掉继承来的或内置底色）；底纹 `{ "pattern": "pct20", "color": "#000000", "fill": "#FFFFFF" }`；渐变 `{ "gradient": "linear", "angle": 90, "stops": ["#FFFFFF", "#E8F0FE"] }`（至少两色） |
| 字体 | `"Times New Roman"`，或 `{ "latin": "...", "eastAsia": "...", "complex": "..." }`；每项可写逗号分隔的回退列表 |

边框线型 `style`：`none` `solid` `thick` `dashed` `dotted` `dashDot` `dashDotDot` `dashSmallGap` `dashDotStroked` `double` `triple` `thinThickSmallGap` `thickThinSmallGap` `thinThickThinSmallGap` `thinThickMediumGap` `thickThinMediumGap` `thinThickThinMediumGap` `thinThickLargeGap` `thickThinLargeGap` `thinThickThinLargeGap` `wave` `doubleWave` `threeDEmboss` `threeDEngrave` `outset` `inset`。

底纹 `pattern`：`pct5` `pct10` `pct20` `pct25` `pct30` `pct40` `pct50` `pct60` `pct70` `pct75` `pct80` `pct90` `horzStripe` `vertStripe` `diagStripe` `reverseDiagStripe` `horzCross` `diagCross` `thinHorzStripe` `thinVertStripe` `thinDiagStripe` `thinReverseDiagStripe` `thinHorzCross` `thinDiagCross`。

### 4.1 文字（runs 的 `style` 只能写这些，外加 `background`）

| 字段 | 取值 | 说明 |
|---|---|---|
| `fontFamily` | 字体 | 写英文族名：`SimSun`、`Microsoft YaHei`、`PingFang SC`、`Noto Sans CJK SC` |
| `fontSize` | 长度 | 默认 10.5pt |
| `color` | 颜色 | |
| `bold` `italic` | 布尔 | 中文字体没有斜体（pdf） |
| `underline` | 布尔或 `single` `double` `thick` `dotted` `dashed` `wavy` `none` | |
| `underlineColor` | 颜色 | |
| `strike` | 布尔或 `"double"` | |
| `verticalAlign` | `super` `sub` | 上下标；盒子/图片上另有 `top` `middle` `bottom` |
| `caps` | `all` `small` | 全大写 / 小型大写 |
| `letterSpacing` | 长度 | 字间距 |
| `highlight` | 颜色 | 高亮 |
| `emphasisMark` | `dot` `comma` `circle` `underDot` | 着重号 |
| `scale` | 数字（百分比，如 `80`） | 字符缩放 |
| `kerning` | 布尔 | |
| `lang` | BCP 47 字符串 | 如 `zh-CN` |
| `hidden` | 布尔 | 隐藏文字 |

### 4.2 段落

| 字段 | 取值 | 说明 |
|---|---|---|
| `align` | `left` `center` `right` `justify` `distribute` | |
| `spaceBefore` `spaceAfter` | 长度或 `"0.5line"` | 段前段后，不继承 |
| `lineHeight` | 见 4.3 | 行距 |
| `marginLeft` `marginRight` | 长度 | 整段缩进，不继承 |
| `textIndent` | 长度 | 首行缩进，常用 `"2em"` |
| `hangingIndent` | 长度 | 悬挂缩进，和 `textIndent` 只写一个 |
| `keepWithNext` | 布尔 | 与下段同页 |
| `keepTogether` | 布尔 | 段中不分页 |
| `pageBreakBefore` | 布尔 | 段前分页 |
| `widowControl` | 布尔 | 孤行控制，默认开 |
| `contextualSpacing` | 布尔 | 同样式段落间不加段距 |
| `autoSpace` | 布尔 | 中英文之间自动加空隙 |
| `kinsoku` | 布尔 | 中文避头尾 |
| `direction` | `ltr` `rtl` | |
| `wrap` | 布尔 | 段落是否自动换行（图片上是绕排方式，见 4.5） |
| `outlineLevel` | 整数 1–9 | 大纲级别 |

### 4.3 行距 lineHeight

| 写法 | 含义 |
|---|---|
| `1.5` | 至少 字号 × 1.5，字更高时撑开。推荐，三种格式一致 |
| `"1.5em"` / `"150%"` | 同 `1.5` |
| `"20pt"`（或 `"7mm"` 等绝对长度） | 固定 20pt，字大了会叠住 |
| `{ "atLeast": "18pt" }` | 至少 18pt |
| `{ "multiple": 1.5 }` | 字体自己的单倍行高 × 1.5，等于 Word 的「多倍行距」；`parse` 读出来的就是这种 |

注意：数字永远是倍数。要 20pt 固定行距写 `"20pt"`，写 `20` 是 20 倍。默认全局 `lineHeight: 1.5`，标题 1.25。

### 4.4 盒子、边框、背景（不继承）

| 字段 | 取值 | 说明 |
|---|---|---|
| `border` | 边框 | 四边 |
| `borderTop` `borderRight` `borderBottom` `borderLeft` | 边框 | 单边，覆盖 `border` |
| `borderBetween` | 边框 | 相邻同样式段落之间的线 |
| `borderInsideH` `borderInsideV` | 边框 | 表格内部横线、竖线 |
| `borderDiagonal` | 边框 | 单元格斜线 |
| `background` | 背景 | |
| `padding` | 长度 | 四边内边距；docx 段落上只在有 `border` 时生效 |
| `paddingTop` `paddingRight` `paddingBottom` `paddingLeft` | 长度 | 单边 |
| `width` `height` | 长度或百分比 | 盒子宽高 |
| `verticalAlign` | `top` `middle` `bottom` | 格子内竖直对齐 |
| `textRotation` | `0` `90` `270` | 文字方向（格子） |
| `radius` | 长度 | 圆角 |

### 4.5 图片位置（写在 Image 的 `style` 上）

| 字段 | 取值 | 说明 |
|---|---|---|
| `position` | `inline`（默认）`float` `absolute` | |
| `float` | `left` `right` | 浮动方向 |
| `wrap` | `square` `tight` `topAndBottom` `none` `behind` `inFront` | 绕排 |
| `wrapDistance` | 长度，或 `{ "top", "right", "bottom", "left" }` | 图文间距，默认 6pt；四边写法里没写的边也是 6pt |
| `anchorTo` | `paragraph` `margin` `page` `slide` | 定位参照 |
| `x` `y` | 长度 | 相对参照的偏移 |
| `horizontalAlign` | `left` `center` `right` | |
| `verticalAlign` | `top` `middle` `bottom` | |
| `objectFit` | `contain` `cover` `fill` | 宽高都给时怎么缩放 |
| `rotation` | 数字（度） | |
| `opacity` | 0–1 | |

### 4.6 列表（写在 List 的 `style` 或 `parts.levelN` 上）

`listStyleType`、`listMarkerFormat`、`listMarkerImage`、`listMarkerFont`、`listStart` 写在元素 `style` 上只管第一级，其余列表字段管所有级别（见 3.7）。

| 字段 | 取值 | 说明 |
|---|---|---|
| `listStyleType` | 无序 `disc` `circle` `square` `dash` `none`；有序 `decimal` `decimal-leading-zero` `lower-alpha` `upper-alpha` `lower-roman` `upper-roman` `cjk-decimal`（一二三） `cjk-ideographic`（甲乙丙） `circled-decimal`（①②③）；或 1–3 个字符当符号，如 `"▸"` | |
| `listMarkerFormat` | 字符串 | 编号模板，`%k` 是第 k 级编号：一级 `"%1."` `"(%1)"` `"第%1条"`，二级 `"%2)"` `"%1.%2"`；符号级别不用 |
| `listMarkerImage` | 路径 | 图片当符号 |
| `listMarkerColor` | 颜色 | |
| `listMarkerFont` | 字体名 | 如 `Wingdings` |
| `listMarkerSize` | 数字（百分比） | 相对正文 |
| `listMarkerAlign` | `left` `center` `right` | 编号在编号区里的对齐 |
| `listMarkerGap` | 长度，或 `tab` `space` `none` | 编号和正文之间 |
| `listIndent` | 长度 | 每级缩进，逐级累加，默认 `2em` |
| `listHanging` | 长度 | 编号区宽度，默认 `2em` |
| `listStart` | 整数 | 起始编号，不继承；写在元素 `style` 上只管第一级 |

注意：不在名单里、又不超过 3 个字符的字符串会被当成项目符号（写 `"abc"` 就显示 abc）。

### 4.7 表格（写在 Table 的 `style` 上）

| 字段 | 取值 | 说明 |
|---|---|---|
| `tableLayout` | `fixed` `auto` | 有 `auto` 列且没写时按 `auto` |
| `cellPadding` | 长度 | 默认 4pt |
| `repeatHeader` | 布尔 | 跨页重复表头，默认开 |
| `rowBreak` | 布尔 | 行内能不能跨页 |
| `bandSize` | 整数 | 隔几行算一条带，默认 1 |

Table 内置样式：`border: 0.5pt solid #999999`、`cellPadding: 4`、`spaceAfter: 8`。部位上可以写 `padding*` 只改那部分格子。

### 4.8 页面（`meta.page`，也是第一节的设置）

| 字段 | 取值 | 默认 |
|---|---|---|
| `size` | `A3` `A4` `A5` `B5` `Letter` `Legal`（不分大小写），或 `{ "width": 长度, "height": 长度 }`；不认识的纸张给警告并按 A4 | `A4` |
| `landscape` | 布尔 | `false` |
| `margin` | 长度 | `20mm` |
| `marginTop` `marginRight` `marginBottom` `marginLeft` | 长度 | 用 `margin` |
| `gutter` | 长度 | 无 |
| `columns` | 数字或 `{ "count", "gap" }` | 不分栏 |
| `header` `footer` | 元素 id 或 `{ "default", "first", "even" }` | 无 |
| `headerDistance` `footerDistance` | 长度 | `0.5in` |
| `pageNumberStart` | 整数 | 1 |
| `pageNumberFormat` | 同 Section | `decimal` |

### 4.9 层叠和继承

会继承给子元素的：文字字段、`align` `lineHeight` `textIndent` `hangingIndent` `widowControl` `contextualSpacing` `autoSpace` `kinsoku` `direction`、列表字段（`listStart` 除外）。其余（段前段后、`marginLeft/Right`、`keep*`、`pageBreakBefore`、`wrap`、`outlineLevel`、边框、背景、盒子、图片位置、表格字段、`verticalAlign`）不继承。

一个元素的样式按下面顺序叠，后面覆盖前面：

1. 父元素传下来的可继承字段（根元素的父 = 内置全局 `fontSize: 10.5, lineHeight: 1.5` + 样式集 `global`）。
2. 内置组件默认样式（先 `Heading`，再 `Heading.1`）。
3. 样式集的 `components`。
4. 顶层 `styles` 里的组件样式（如 `"Heading.2"`）。
5. 自定义组件模板里写的 `style`。
6. 指名自定义组件的键：`PriceTable`（模板根元素）、`PriceTable:t`（模板元素 `t`）、`PriceTable:t.header`（再接部位）。
7. 元素自己的 `style`。命名样式先找当前样式集的 `styles`，再找顶层 `styles`。
8. run：段落文字样式 → `InlineCode`/`InlineMath` 组件样式 → 链接默认色 → 简写字段 → run 的 `style`。

部位样式顺序：内置 → 样式集 → 顶层 `styles`（`Table.header`）→ 模板 `parts` → `组件名:元素id.部位` → 元素自己的 `parts`。

内置默认样式（不写也有）：

| 键 | 默认 |
|---|---|
| 全局 | `fontSize: 10.5`、`lineHeight: 1.5` |
| `Heading` | 粗体、`keepWithNext`、`spaceBefore: 12`、`spaceAfter: 6`、`lineHeight: 1.25`；`Heading.1` 20pt、段前 18 |
| `Text` | `spaceAfter: 6` |
| `Code` | 等宽字体、9pt、`lineHeight: 1.4`、底色 `#F6F8FA`、`padding: 8`、`keepTogether` |
| `InlineCode` | 等宽字体、底色 `#F0F1F3` |
| `List` | `spaceAfter: 6`、`listIndent: 2em`、`listHanging: 2em` |
| `Table` / `Table.header` | 见 3.8、4.7 |
| `Image.caption` | 9pt、`#666666` |
| `Divider` | `borderBottom: 0.5pt solid #D0D7DE` |
| `Quote` | `borderLeft: 3pt solid #D0D7DE`、`paddingLeft: 10`、`#57606A` |
| `Callout` | 底色 `#F6F8FA`、`border: 0.5pt solid #E1E4E8`、`padding: 8` |
| `Math` | 居中、段前段后 6 |
| `TableOfContents.title` | 14pt 粗体 |
| `Slide.title` | 20pt 粗体 |

### 4.10 命名样式、样式集、变量

组件样式的键：组件名（`Text`、`InlineCode`、`InlineMath`、自定义组件名）、`Heading.1`…`Heading.6`、`组件名.部位`（`Table.header`、`List.level2`、`Image.caption`、`TableOfContents.entry`、`Slide.title`）、`自定义组件名:模板元素id[.部位]`。其他键都是命名样式，元素用 `"style": "名字"` 引用。

样式集：

```json
"styleSet": "formal",
"styleSets": {
  "formal": {
    "global": { "$brand": "#1A73E8", "$rule": "1pt solid ${brand}", "fontFamily": { "latin": "Times New Roman", "eastAsia": "SimSun" }, "fontSize": 12 },
    "components": { "Heading": { "color": "${brand}" }, "Table.header": { "background": "#F2F2F2" } },
    "styles": { "muted": { "color": "#666666" }, "ruled": { "borderBottom": "${rule}" } }
  },
  "modern": { "extends": "formal", "global": { "fontFamily": { "latin": "Inter", "eastAsia": "Noto Sans CJK SC" } } }
}
```

| 字段 | 说明 |
|---|---|
| `global` | 只能写会继承的字段（文字、段落、列表）；写 `border`、`background` 这类会报错 |
| `components` | 组件样式 |
| `styles` | 命名样式，只在这个样式集选中时有效 |
| `extends` | 继承另一个样式集，单继承，成环报错 |
| `name` | 只在单独的样式集文件里写（必填） |

变量：

- 只能在样式集的 `global` 里定义，键以 `$` 开头：`"$brand": "#1A73E8"`。
- 引用写 `"${brand}"`。整个值就是引用时保留原类型（数字、对象都行）；夹在文字里时按文字拼，这时变量只能是文字或数字。
- 能用在样式集的 `global`/`components`/`styles`、顶层 `styles`、元素的 `style`/`parts`、格子 `style`、组件 props 里。
- 没选样式集时没有变量，引用会报「变量没定义」。字面的 `${` 写成 `$${`。

## 5. 自定义组件

定义写在文档顶层 `components` 数组里（MCP 一次传完最方便），或单独的 JSON 文件（放 `catalog/` 目录或用 `--catalog`）。

| 字段 | 必填 | 说明 |
|---|---|---|
| `name` | 是 | 大写字母开头，只用字母、数字、`_`，不能和内置组件重名 |
| `description` | 否 | 说明 |
| `props` | 否 | prop 名 → `{ "type", "required", "default", "description" }` |
| `container` | 否 | 默认 `false`；`true` 时模板里某个 `children` 要写一次 `"$children"` |
| `template` | 是 | `{ "root": 模板根元素 id, "elements": { ... } }`，写法同文档 elements |

prop `type`：`string`（默认）`number` `integer` `boolean` `array` `object` `inline`（字符串或 runs 数组）`any`。`default` 的类型要和 `type` 对上。

模板规则：

- 字符串正好是 `"{{title}}"` 时，整个换成 prop 的值（数组、对象也行）；夹在文字里（`"第 {{n}} 章"`）按文字替换，这时 prop 不能是数组或对象。
- 没给值的可选 prop：整个替换时变成 null，夹在文字里变成空串。
- 模板里引用没定义的 prop 报错；模板元素 id 不能带 `.`。
- 实例 id 留给模板根元素；其他模板元素 id 变成 `实例id.模板id`（如 `tip.head`），别和文档里已有 id 撞。
- 实例的 `style` 叠在模板根元素的 `style` 后面。
- 模板里可以用别的自定义组件，最多套 32 层。
- 实例多写没定义的 prop、少写必填 prop、类型不对都报错。

<!-- check -->
```json
{
  "version": "0.1",
  "components": [
    {
      "name": "Notice",
      "description": "带标题的提示框，子元素放在标题下面",
      "container": true,
      "props": {
        "title": { "type": "inline", "required": true },
        "tone": { "type": "string", "default": "#1A73E8" }
      },
      "template": {
        "root": "box",
        "elements": {
          "box": { "type": "Document", "children": ["head", "$children"] },
          "head": { "type": "Text", "props": { "text": "{{title}}" }, "style": { "bold": true, "color": "{{tone}}" } }
        }
      }
    },
    {
      "name": "Signature",
      "props": { "who": { "type": "string", "required": true }, "date": { "type": "string" } },
      "template": {
        "root": "s",
        "elements": { "s": { "type": "Text", "props": { "text": "{{who}}　{{date}}" }, "style": { "align": "right" } } }
      }
    }
  ],
  "root": "doc",
  "elements": {
    "doc": { "type": "Document", "children": ["tip", "sign"] },
    "tip": { "type": "Notice", "props": { "title": "提示" }, "children": ["tip-body"] },
    "tip-body": { "type": "Text", "props": { "text": "子元素会放到 $children 的位置。" } },
    "sign": { "type": "Signature", "props": { "who": "document-render", "date": "2026-11-20" } }
  }
}
```

## 6. 完整示例

封面一节（不显示页眉页脚）+ 正文一节（页码从 1 起、奇偶页页眉不同），带目录、段落 runs、多级列表、合并格表格、图片、公式、代码、提示框、引用、样式集变量。图片路径相对文档所在目录（MCP 里设 `base_dir`）。

<!-- check -->
```json
{
  "version": "0.1",
  "meta": {
    "title": "2026 年第三季度运营报告",
    "author": "运营部",
    "lang": "zh-CN",
    "page": { "size": "A4", "margin": "25mm", "headerDistance": "12mm", "footerDistance": "12mm" }
  },
  "styleSet": "report",
  "styleSets": {
    "report": {
      "global": {
        "$brand": "#1A73E8",
        "$rule": "0.5pt solid #999999",
        "fontFamily": { "latin": "Times New Roman, Liberation Serif", "eastAsia": "SimSun, Songti SC, Noto Serif CJK SC" },
        "fontSize": 11,
        "lineHeight": 1.5
      },
      "components": {
        "Heading": { "fontFamily": { "eastAsia": "Microsoft YaHei, PingFang SC, Noto Sans CJK SC" }, "color": "${brand}" },
        "Heading.1": { "fontSize": 20, "align": "center" },
        "Text": { "textIndent": "2em" },
        "Table.header": { "background": "${brand}", "color": "#FFFFFF" },
        "Table.evenRow": { "background": "#F5F7FA" }
      },
      "styles": {
        "hf": { "fontSize": 9, "color": "#666666", "textIndent": 0 },
        "hfLine": { "borderBottom": "${rule}", "paddingBottom": 2 }
      }
    }
  },
  "styles": {
    "center": { "align": "center", "textIndent": 0 },
    "cover-title": { "fontSize": 28, "spaceBefore": "60mm" }
  },
  "root": "doc",
  "elements": {
    "doc": { "type": "Document", "children": ["cover", "body"] },

    "cover": {
      "type": "Section",
      "props": { "header": { "first": null }, "footer": { "first": null } },
      "children": ["c-title", "c-sub", "c-pb", "toc"]
    },
    "c-title": { "type": "Heading", "props": { "level": 1, "text": "2026 年第三季度运营报告" }, "style": "cover-title" },
    "c-sub": { "type": "Text", "props": { "text": "运营部 · 2026 年 10 月" }, "style": ["center", { "color": "#666666" }] },
    "c-pb": { "type": "PageBreak" },
    "toc": { "type": "TableOfContents", "props": { "title": "目录", "levels": 2 } },

    "body": {
      "type": "Section",
      "props": {
        "pageNumberStart": 1,
        "pageNumberFormat": "decimal",
        "header": { "default": "hdr-odd", "first": "hdr-odd", "even": "hdr-even" },
        "footer": { "default": "ftr", "first": "ftr" }
      },
      "children": ["h1", "p1", "h1-1", "steps", "h1-2", "eq", "p2", "h2", "tbl", "fig", "code", "tip", "quote", "line", "end"]
    },
    "h1": { "type": "Heading", "props": { "level": 2, "text": "一、概况" } },
    "p1": {
      "type": "Text",
      "props": {
        "text": [
          "本季度收入 ",
          { "text": "255 万元", "bold": true, "color": "#CF222E" },
          "，环比增长 12%。数据来自",
          { "text": "运营看板", "href": "https://example.com/dashboard" },
          "，导出命令是 ",
          { "type": "InlineCode", "text": "document-render render q3.json -o q3.pdf" },
          "。增长率按 ",
          { "type": "InlineMath", "text": "r = \\frac{x_t - x_{t-1}}{x_{t-1}}" },
          " 计算",
          { "text": "1", "style": { "verticalAlign": "super" } },
          "。"
        ]
      }
    },
    "h1-1": { "type": "Heading", "props": { "level": 3, "text": "1.1 本季度做了什么" } },
    "steps": {
      "type": "List",
      "props": {
        "ordered": true,
        "items": [
          "上线华东区新仓",
          { "text": "调整定价", "items": ["基础版不变", "专业版下调 10%"], "ordered": false },
          "完成年中审计"
        ]
      },
      "parts": {
        "level1": { "listStyleType": "cjk-decimal", "listMarkerFormat": "%1、" },
        "level2": { "listStyleType": "circle", "listMarkerColor": "#1A73E8" }
      }
    },
    "h1-2": { "type": "Heading", "props": { "level": 3, "text": "1.2 预测模型" } },
    "eq": { "type": "Math", "props": { "latex": "\\hat{y}_{t+1} = \\alpha y_t + (1-\\alpha)\\hat{y}_t", "number": 1 } },
    "p2": { "type": "Text", "props": { "text": ["其中 ", { "type": "InlineMath", "text": "\\alpha = 0.3" }, "，按过去八个季度拟合。"] } },

    "h2": { "type": "Heading", "props": { "level": 2, "text": "二、分区数据" } },
    "tbl": {
      "type": "Table",
      "props": {
        "columns": ["地区", "月份", "收入（万元）", "占比"],
        "rows": [
          [{ "text": "华东", "rowSpan": 3 }, "7 月", "48", "19%"],
          ["8 月", "52", "20%"],
          ["9 月", "55", "22%"],
          [{ "text": "华南", "rowSpan": 3 }, "7 月", "30", "12%"],
          ["8 月", "33", "13%"],
          ["9 月", "37", "14%"],
          [{ "text": "合计", "colSpan": 2, "style": { "align": "center" } }, "255", "100%"]
        ],
        "widths": [1.2, 1, 1.5, 1],
        "look": { "lastRow": true, "lastColumn": true },
        "rowHeights": ["20pt"]
      },
      "style": { "border": "${rule}" },
      "parts": {
        "lastRow": { "bold": true, "borderTop": "1pt solid #333333" },
        "lastColumn": { "align": "right" }
      }
    },
    "fig": {
      "type": "Image",
      "props": { "src": "logo.png", "alt": "公司标志", "width": 96, "caption": "图 1 公司标志" },
      "style": { "align": "center" },
      "parts": { "caption": { "align": "center", "textIndent": 0 } }
    },
    "code": { "type": "Code", "props": { "language": "sql", "code": "SELECT region, SUM(amount)\nFROM sales\nWHERE quarter = '2026Q3'\nGROUP BY region;" } },
    "tip": { "type": "Callout", "props": { "icon": "💡", "text": "9 月数据还没审计，可能小幅调整。" }, "style": { "background": "#FFF8E1", "border": "0.5pt solid #F0C36D" } },
    "quote": { "type": "Quote", "props": { "text": "新仓让华东的平均配送时间少了半天。" } },
    "line": { "type": "Divider" },
    "end": { "type": "Text", "props": { "text": "注 1：增长率按含税收入计算。" }, "style": ["hf", { "fontSize": 9 }] },

    "hdr-odd": { "type": "Text", "props": { "text": "2026 年第三季度运营报告" }, "style": ["hf", "hfLine", { "align": "right" }] },
    "hdr-even": { "type": "Text", "props": { "text": "运营部" }, "style": ["hf", "hfLine", { "align": "left" }] },
    "ftr": {
      "type": "Text",
      "props": { "text": ["第 ", { "field": "page" }, " 页，共 ", { "field": "pages" }, " 页"] },
      "style": ["hf", { "align": "center" }]
    }
  }
}
```

要点：`hdr-odd`、`hdr-even`、`ftr` 不在任何 children 里，只由 Section 的 `header`/`footer` 引用。合并格那几行少写了被占掉的格子。

## 7. 各格式差异（要点）

- docx：图片只收 png/jpeg/gif/webp，SVG 和远程图片变成 `[alt]` 加警告。段落 `padding` 只在有 `border` 时生效。目录要在 Word 里更新域才有页码。每节末尾多一个空段落放节设置。字体只写回退列表里的第一个。
- pdf（Typst）：字体按英文族名在本机找，找不到退回默认字体并给警告。中文没有斜体。支持 SVG，不支持远程图片。颜色只认十六进制。
- html：默认一长条网页，分页、页眉页脚、表头重复只在打印时出现；`--paged`/`paged: true` 才按页显示并填页码。公式写成 MathML。
- md：只留结构；合并格的表格写成内嵌 HTML，Callout 写成 `> [!NOTE]`，公式写成 `$…$`/`$$…$$`；样式、分栏、ColumnBreak 都丢掉。
- 表达不了的样式一律警告不报错，所以「`check` 通过」不代表每个样式在每种格式里都生效。
- `Slide`：html/pdf 一页一张，docx 变成二级标题加正文再分页。

## 8. 常见错误

| 错误写法 | 后果 | 正确写法 |
|---|---|---|
| children 里写了不存在的 id | 报错「元素 `x` 不存在」 | 先在 `elements` 里定义 |
| 同一个 id 放进两个 children，或页眉元素也放进 children | 报错「被引用了不止一次」 | 每个元素只引用一次；页眉页脚只由 `header`/`footer` 引用 |
| 定义了元素却没引用 | 不报错，只警告，内容丢失 | 检查每个元素都挂到了树上 |
| `Text`、`Heading`、`Table` 写 `children` | 报错「不能有 children」 | 只有 Document/Section/Slide/容器型自定义组件能写 |
| `"Heading"` 的 `"props": { "text": "x" }` 少了 `level` | 报错 props 不对 | 必填 prop 都要写 |
| props 拼错：`"lineNumber"`、`"rowspan"`、`"colspan"` | 报错 unknown field | `lineNumbers`、`rowSpan`、`colSpan` |
| 样式字段拼错：`"font-size"`、`"fontWeight"` | 报错 | `fontSize`、`bold: true` |
| 颜色写 `"red"`、`"1A73E8"` | 报错 | `"#FF0000"`、`"#1A73E8"` |
| `fontFamily` 写中文名 `"宋体"`、`"微软雅黑"` | pdf 找不到字体退回默认并警告，行高也算不准 | 英文族名 `"SimSun"`、`"Microsoft YaHei"`，可写回退列表 |
| `lineHeight: 20` 想要 20pt | 变成 20 倍行距 | `"20pt"` |
| Image 的 `width: "100pt"` | 报错，只收数字 | `width: 100` |
| run 上写 `"strike": true`、`"fontSize": 12` | 报错，run 只有几个简写 | 写进 run 的 `style` |
| 报错信息是 `did not match any variant of untagged enum Inline`（或 `Cell`、`ListItem`） | — | 多半是 run、格子或列表项对象里写了不认识的字段，或类型不对（比如 `width` 写成字符串） |
| run 的 `style` 里写 `align`、`border` | 报错，只能写文字字段 | 段落字段写在元素的 `style` 上 |
| `InlineCode`/`InlineMath` 当元素 `type` | 报错 | 写在 runs 里；块级用 `Code`/`Math` |
| LaTeX 反斜杠只写一个 `"\frac"` | JSON 把 `\f` 当转义，公式错 | `"\\frac"` |
| 被 `rowSpan` 占掉的位置还写了格子 | 列数对不上，报错或警告 | 后面的行少写那几格 |
| `parts` 写了组件没有的部位（`Text` 的 `header`） | 报错并列出可选部位 | 按第 3 节总表写 |
| `"Heading.7"`、`"Text.header"` 当样式键 | 报错 | 级别只有 1–6；部位见总表 |
| 样式集 `global` 里写 `background`、`border` | 报错 | 放进 `components` 或命名样式 |
| `"style": "muted"` 但没定义 `muted` | 报错「样式没定义」 | 在 `styles` 或样式集 `styles` 里定义 |
| 用了 `${brand}` 但没选样式集 | 报错「变量没定义」 | 写 `styleSet`，变量定义在它的 `global` 里 |
| Section 放在 Document 以外的位置，或 Section 后面跟散元素 | 报错 | Section 只放在 root 下；后面的内容都进 Section |
| 页眉页脚里放 PageBreak/Section | 报错 | 页眉页脚只放文字类元素 |
| 封面节写 `"first": null`，正文节只写 `"header": "hdr"` | 正文首页没有页眉（`first` 沿用了） | 正文节写 `{ "default": "hdr", "first": "hdr" }` |
| 表格写了 `parts.lastRow`/`lastColumn` 但没开 `look` | 不报错，样式不生效 | `"look": { "lastRow": true }` |
| 想让各级都用同一种编号，只在 List 的 `style` 上写 `listStyleType` | 只有第一级变，下级还是默认的 a. b. 或圆点 | 每级写进 `parts.levelN` |
| 二级的 `listMarkerFormat` 写成 `"%1."` | 二级每项都显示一级的编号（`%1` 是第一级） | 写 `"%2."` 或 `"%1.%2"` |
| 长度写 `"2字符"`、`"10 pt"` 以外的怪单位 | 报错 | 单位只认 `pt mm cm in px em line %` |
| `listStyleType: "roman"` | 报错；而 `"abc"`（≤3 字符）会被当成符号 | `"lower-roman"` |
| 顶层多写 `title`、`page` | 报错 | 放进 `meta`、`meta.page` |
