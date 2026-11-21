# 样式模型（草案）

> 状态：已实现（2026-10-28），代码见 `src/style.rs`（字段）、`src/cascade.rs`（层叠）、`src/render/`（各格式）。各格式表达不了的字段，渲染时给一次警告并忽略。
>
> 已定（Rei 2026-10-28）：字段平铺在 `style` 下；字段名跟 CSS 走；不做主题色。

document-render 的样式以 OOXML 为准：先把 Word、PowerPoint、Excel 能配的属性列全，再归并成一套字段。HTML 和 PDF（Typst）按这套字段去对应，表达不了的就忽略并给警告。

依据：ECMA-376 第 5 版 Part 1（WordprocessingML 第 17 章、SpreadsheetML 第 18 章、DrawingML 第 20–21 章）。

## 一、写法约定

- 字段名用 camelCase，和现在一致。
- 按 OOXML 的层级分组：文字（rPr）、段落（pPr）、边框、背景、盒子（表格单元格、文本框）、表格、页面。分组只用来写文档；JSON 里字段平铺在 `style` 下，不嵌套。
- 长度：数字就是 pt；字符串可以带单位：`pt`、`mm`、`cm`、`in`、`px`（按 96 dpi 折算），还有两个排版单位：
  - `em`：当前字号的倍数，`"2em"` 就是首行缩进两个字。Word 里对应 `firstLineChars` 这类按字符计的属性。
  - `line`：行数，只用在段前段后，`"0.5line"` 对应 Word 的 `beforeLines`。
- 颜色：`#RRGGBB`，以后加 `#RRGGBBAA`（只有 PPT、HTML、PDF 认透明度）。不做主题色名。
- 同一个属性分「全部」和「单边」两种写法，单边覆盖全部：`border` 和 `borderTop`，`padding` 和 `paddingLeft`。

### 单位换算（实现时用）

| OOXML 单位 | 用在哪 | 1pt 等于 |
|---|---|---|
| twip（1/20 pt） | w:spacing、w:ind、页边距、表格宽 | 20 |
| 半磅 | w:sz（字号） | 2 |
| 1/8 pt | w:pBdr、w:tcBorders 的 sz | 8 |
| 1/100 pt | a:rPr sz、a:spcPts | 100 |
| EMU | DrawingML 的长度、线宽 | 12700 |
| 1/240 行 | w:spacing line（lineRule=auto） | — |
| 1/1000 % | a:spcPct、a:lnSpc | 100% = 100000 |

## 二、文字（对应 w:rPr、a:rPr、xlsx font）

| 字段 | 值 | Word w:rPr | PPT a:rPr | Excel font | HTML | PDF |
|---|---|---|---|---|---|---|
| `fontFamily` | 字符串，或按文种分开的对象（见下） | w:rFonts | a:latin / a:ea / a:cs | name（只有一个） | font-family | font |
| `fontSize` | 长度 | w:sz + w:szCs | sz | sz | font-size | size |
| `color` | 颜色 | w:color | a:solidFill | color | color | fill |
| `bold` | 布尔 | w:b + w:bCs | b | b | font-weight | weight |
| `italic` | 布尔 | w:i + w:iCs | i | i | font-style | style |
| `underline` | 布尔，或 `single` `double` `thick` `dotted` `dashed` `wavy` | w:u val | u | u（single、double，另有会计用） | text-decoration-style | underline（只有实线） |
| `underlineColor` | 颜色 | w:u color | a:uFill | — | text-decoration-color | stroke |
| `strike` | 布尔，或 `double` | w:strike / w:dstrike | strike（sngStrike / dblStrike） | strike（只有单线） | line-through | strike |
| `verticalAlign` | `super` `sub`（字号和偏移取字体自带的上标下标值，跟 Word 一样；有中文时看东亚字体） | w:vertAlign | baseline（±30000） | vertAlign | vertical-align + font-size（按字体算） | super / sub（按字体算） |
| `caps` | `all` `small` | w:caps / w:smallCaps | cap | — | text-transform / font-variant | smallcaps / upper |
| `letterSpacing` | 长度 | w:spacing（twip） | spc（1/100 pt） | — | letter-spacing | tracking |
| `highlight` | 颜色 | w:highlight（只有 16 种固定色）或 w:shd | a:highlight | — | background | highlight |
| `emphasisMark` | `dot` `comma` `circle` `underDot` | w:em（着重号） | — | — | text-emphasis | — |
| `scale` | 百分比数字，如 `80` | w:w | — | — | transform（不准） | — |
| `kerning` | 布尔 | w:kern | kern | — | font-kerning | kerning |
| `lang` | BCP 47，如 `zh-CN` | w:lang val / eastAsia | lang / altLang | — | lang | lang |
| `hidden` | 布尔 | w:vanish | — | — | display:none | — |

`fontFamily` 按文种分开写：

```json
{ "fontFamily": { "latin": "Inter", "eastAsia": "思源黑体", "complex": "Noto Naskh" } }
```

- `latin`：西文：w:rFonts ascii + hAnsi，a:latin
- `eastAsia`：中日韩：w:rFonts eastAsia，a:ea
- `complex`：阿拉伯文等：w:rFonts cs，a:cs

写成字符串时三项都用它。代码用什么字体不在这里定，写在 `InlineCode` 和 `Code` 组件的样式里（见第十三节）。Excel 一个单元格只能有一种字体：有中文内容取 `eastAsia`，否则取 `latin`。HTML、PDF 拼成回退列表：`latin, eastAsia`。

### 字体要装在本机

渲染 pdf 和 docx 之前，先把文档实际用到的字体都找一遍（Rei 2026-11-21 定：「最开始就检查字体，如果字体不存在就报错」）。找不到就报错，不出文件。

- 查哪些：样式集展开、层叠算完以后，每个元素、每个 run、每个部位、每个格子实际用的 `fontFamily`（三个文种都算），加上 `listMarkerFont`。标题、代码、列表符号、页眉页脚里的都算。
- 在哪找：和 pdf 排版同一套字体，系统字体加 Typst 自带字体（Libertinus Serif、New Computer Modern、DejaVu Sans Mono 等）。中文名（宋体、微软雅黑、思源黑体……）按内置对照表换成英文族名再找；pdf 排版时也用换过的名字。
- 自己写的字体：单个族名要找得到。逗号分开的回退列表有一个找得到就行，一个都没有才报错（Rei 2026-11-21 改，原来要求每一项都有）。列表里缺几个、但有能用的，不报错也不给警告，pdf 用第一个找得到的。
- 内置默认字体：西文 Helvetica Neue、Arial、Liberation Sans；中文 PingFang SC、Hiragino Sans GB、Microsoft YaHei、Noto Sans CJK SC、Source Han Sans SC；代码 Menlo、Consolas、DejaVu Sans Mono。这是按系统挑着用的备选，一组里一个都没有才报错。哪段文字用不到某一组（比如没有中文），这一组就不查。
- 报错是中文，每个缺的字体一行：族名（回退列表整条列出来，后面注明「列表里一个都没有」）、用到它的元素 id（多了只列前五个，写「等 12 处」），还有写在哪个样式键里，比如 `styles.正文`、`styleSets.formal.global`、`元素 t1 的 style`。
- 放行：命令行 `--allow-missing-fonts`，MCP 的 `render`、`check` 写 `allow_missing_fonts: true`。缺的字体变成警告，照常出文件：pdf 里换成默认字体，docx 里照写字体名。
- docx 也报错：本机没有这个字体，就没法确认打开文件的那台电脑有没有。确定对方装了，就用上面的参数放行。
- html、Markdown 不查，浏览器自己回退。`check` 不分格式，也把缺的字体算成错误，加参数时降为警告。
- parse 读字体按文种比，只写和上面传下来不一样的那个文种，不会每段都抄一遍东亚字体。
- 读回的字体照 docx 原样，不跳过、不替换。document-render 写 docx 时，没写字体就在 docDefaults 里写 Arial（西文、复杂文种）和 PingFang SC（东亚），parse 读回来就是 `styles.Document` 里的 Arial、PingFang SC；代码写的是 Menlo，就读成 Menlo，不换成等宽回退列表。本机没装这些字体，再出 pdf、docx 会报缺字体，要出就放行。跨系统字体兼容由写文档的人负责，document-render 不替他兜底（Rei 2026-11-21 定）。
- docx 每个文种只能写一个字体名，回退列表（包括 `listMarkerFont`）只写第一个。

## 三、段落（对应 w:pPr、a:pPr、xlsx alignment）

| 字段 | 值 | Word w:pPr | PPT a:pPr | Excel alignment | HTML | PDF |
|---|---|---|---|---|---|---|
| `align` | `left` `center` `right` `justify` `distribute` | w:jc（start、center、end、both、distribute） | algn（l、ctr、r、just、dist） | horizontal | text-align | align / justify |
| `spaceBefore` | 长度或 `"Nline"` | w:spacing before / beforeLines | a:spcBef | — | margin-top | block above |
| `spaceAfter` | 同上 | w:spacing after / afterLines | a:spcAft | — | margin-bottom | block below |
| `lineHeight` | 数字是倍数，至少字号 × 倍数；长度是固定值；`{ atLeast: 长度 }` 是最小值；`{ multiple: 倍数 }` 是 Word 的多倍行距（见下面「行距」） | w:spacing line + lineRule（数字写成 atLeast 字号 × 倍数；固定值 exact；最小值 atLeast；multiple 写 auto，line = 倍数 × 240） | a:lnSpc（spcPct / spcPts） | — | line-height（按 Word 的规则换算） | top-edge / bottom-edge（按 Word 的规则换算） |
| `marginLeft` | 长度，整段左缩进 | w:ind start（`em` 时加 leftChars） | marL | indent（按字符数，只有整数） | margin-left | pad |
| `marginRight` | 长度，整段右缩进 | w:ind end（`em` 时加 rightChars） | — | — | margin-right | pad |
| `textIndent` | 长度，首行缩进 | w:ind firstLine（`em` 时用 firstLineChars） | indent（正数） | — | text-indent | first-line-indent |
| `hangingIndent` | 长度，悬挂缩进：除首行外其他行缩进，和 `textIndent` 只能写一个 | w:ind hanging / hangingChars | indent（负数）+ marL | — | padding-left + 负 text-indent | hanging-indent |
| `keepWithNext` | 布尔 | w:keepNext | — | — | break-after: avoid | 用 block(breakable) 近似 |
| `keepTogether` | 布尔 | w:keepLines | — | — | break-inside: avoid | block(breakable: false) |
| `pageBreakBefore` | 布尔 | w:pageBreakBefore | — | — | break-before | pagebreak |
| `widowControl` | 布尔，默认开 | w:widowControl | — | — | orphans / widows | — |
| `contextualSpacing` | 布尔：同样式段落之间不加段距 | w:contextualSpacing | — | — | — | — |
| `autoSpace` | 布尔：中英文、中文和数字之间自动加空隙 | w:autoSpaceDE + w:autoSpaceDN | — | — | text-autospace（新） | — |
| `kinsoku` | 布尔：中文避头尾 | w:kinsoku | — | — | line-break: strict | 默认就有 |
| `direction` | `ltr` `rtl` | w:bidi | rtl | readingOrder | direction | dir |
| `wrap` | 布尔：自动换行 | — | a:bodyPr wrap | wrapText | white-space | — |
| `outlineLevel` | 1–9 | w:outlineLvl | — | — | — | — |

### 行距

`lineHeight` 有四种写法，数字和 `{ multiple }` 都叫「倍数」，但乘的东西不一样：

| 写法 | 一行多高 | 什么时候用 |
|---|---|---|
| `1.5` | 至少字号 × 1.5，字比它高时撑开 | 默认用这个，三种格式行高一致 |
| `"20pt"` | 固定 20pt | 要精确对齐时 |
| `{ "atLeast": "18pt" }` | 至少 18pt，字大了会撑高 | 混排大字、图片时 |
| `{ "multiple": 1.5 }` | 字体自己的单倍行高 × 1.5，跟 Word 的「多倍行距」一样 | 要和 Word 做的文档对上时，`document-render parse` 读出来的就是这种 |

pdf 和 html 的行高照 Windows 版 Word 排 docx 的规则算（2026-11-18 在 Word 2024 上实测），三种格式出来一样高。

**一种字体的单倍行高**不是字号，要看字体文件：

- 西文字体：fsSelection 的 bit7（USE_TYPO_METRICS）关着时，取 win（usWinAscent + usWinDescent）和 hhea（含 lineGap）里大的那个，多出 win 的部分加在基线上面；bit7 开着时只看 typo（含 typoLineGap）。比如 Calibri 1.221、Arial 1.150、Segoe UI 1.330。
- 东亚字体（OS/2 的 ulCodePageRange1 里有日文、简中、韩文、繁中任一位）：不看 hhea，在 win 全高的上下各加 0.15 倍（按字体单位四舍五入），所以约是 1.3 倍。微软雅黑 1.715、等线 1.354、宋体 1.297、MiSans 1.724。东亚字体排西文字母也是这么高。

**一行里有几种字体**（中文配西文、编号用 Symbol）：上沿取各字体最高的 ascent，下沿取最深的 descent。有中文时算上 eastAsia 字体，没有时只算西文字体；`listMarkerFont` 写了的话编号字体也算。

**四种写法怎么算**：

- `{ multiple: m }`：混排单倍 + (m − 1) × 最高的那个字体单倍，多出来的加在基线下面。
- 数字 `x`：docx 写成「至少 字号 × x」（不写 auto，因为 auto 乘的是字体的单倍，雅黑比宋体高三成多）。所以行高是 max(字号 × x, 混排单倍)，多出来的加在基线上面。
- `{ atLeast: v }`：max(v, 混排单倍)，多出来的加在基线上面。
- 固定值 `v`：就是 v，基线在 0.8 × v 处，跟字体无关。字比它高时会和上一行叠起来，Word 也这样。

各格式的做法：

- docx：数字写 `w:lineRule="atLeast"`，固定值写 exact，最小值写 atLeast，multiple 写 auto（line = 倍数 × 240），Word 自己算。fontTable 里给用到的字体写上 charset（简中字体 86、日文 80、韩文 81、繁中 88），Windows Word 只有 charset 和字体对得上时才乘 1.3，写成 00 就不乘。
- pdf：用本机找到的字体按上面的规则算出基线上下各多少，写进 Typst 的 top-edge、bottom-edge。表格横线的粗细也算进行高，跟 Word 一样：第一行加上框线，最后一行加下框线；行和行之间的线在 docx 里写在上面一格的下边，Word 先拿它去占下一行的上边距，线比上边距粗的部分才加进行高（边距 5pt、线 3pt 时不加）。
- html：同样算出行高，写成 `line-height`（字号的倍数，固定值写长度）。CSS 只能定行高，基线在行里的位置还是浏览器自己分；浏览器用的字体和生成时找到的不一样时，行高会有出入。
- markdown：不管行距。
- 字体都找不到时按 0.88 + 0.29 = 1.17 倍字号算。
- 字体按「列表里第一个本机找得到的」算，和 pdf 实际用的一样；docx 只写列表里第一个。本机没有、Word 那台有的字体（比如 Linux 上没有 PingFang SC），pdf 和 Word 会对不上。默认中文字体 PingFang SC 在 Windows 上没有，Word 换成微软雅黑，所以默认字体列表里雅黑排在 Noto、思源前面。
- document-render 写的 docx 不带文档网格（`w:docGrid`），所以不做网格对齐。parse 读到带网格的 Word 文档时给警告。

## 四、边框（对应 w:pBdr、w:tblBorders、w:tcBorders、a:ln、xlsx border）

一条边写成对象，也能用简写字符串 `"1pt solid #CCCCCC"`：

```json
{
  "border": { "width": 1, "style": "solid", "color": "#CCCCCC" },
  "borderBottom": { "width": 2, "style": "double", "color": "#1A73E8" },
  "borderBetween": "none"
}
```

- `borderBetween`：合成一个框的相邻段落之间的线，w:pBdr between（见下面「相邻段落合成一个框」）

| 字段 | 用在哪 | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|---|
| `border` / `borderTop` / `borderRight` / `borderBottom` / `borderLeft` | 段落、单元格、表格、文本框 | w:top … w:start、w:end | 形状只有一条 a:ln，四边不能分开 | left right top bottom | border-* | stroke |
| `borderBetween` | 合框段落之间 | w:pBdr between | — | — | 两段之间一个 div 的 border-top | 两段之间一条线 |
| `borderInsideH` / `borderInsideV` | 表格内部横线、竖线 | w:tblBorders insideH / insideV | a:tcPr 的 lnB 等逐格写 | 逐格写 | 逐格写 | table stroke |
| `borderDiagonal` | 单元格斜线 | w:tl2br / w:tr2bl | a:lnTlToBr | diagonal | — | — |

线型 `style` 收 Word 的全部 25 种，艺术边框不算。名字照抄 Word 的 `w:val`，只有三个不一样：`solid` 就是 Word 的 single，`dashDot` 是 dotDash，`dashDotDot` 是 dotDotDash。

常用的几种，各格式这样换：

| document-render | Word val | PPT prstDash / cmpd | Excel style | CSS |
|---|---|---|---|---|
| `none` | nil | a:noFill | 不写 | none |
| `solid` | single | solid | 按粗细：hair（≤0.25pt）、thin（≤1）、medium（≤2）、thick | solid |
| `dashed` | dashed | dash | dashed / mediumDashed | dashed |
| `dotted` | dotted | sysDot | dotted | dotted |
| `dashDot` | dotDash | dashDot | dashDot / mediumDashDot | SVG 图案 |
| `double` | double | cmpd="dbl" | double | double |
| `wave` | wave | — | — | SVG 图案 |

剩下的线型，PPT、Excel 先不管，Word 原样写：

| document-render | 长什么样 | HTML | PDF |
|---|---|---|---|
| `thick` | 粗实线 | solid | 实线 |
| `dashDotDot` | 一划两点 | SVG 图案 | SVG 图案 |
| `dashSmallGap` | 空隙很小的虚线 | SVG 图案 | SVG 图案 |
| `dashDotStroked` | 斜着的一划一点 | SVG 图案 | SVG 图案 |
| `triple` | 三条细线 | SVG 图案 | 几条实线 |
| `thinThickSmallGap` 等九种 | 细粗、粗细、细粗细三组，每组再分小、中、大三种空隙 | SVG 图案 | 几条实线 |
| `doubleWave` | 两条波浪线 | SVG 图案 | SVG 图案 |
| `threeDEmboss` / `threeDEngrave` | 凸起、凹下去的立体线 | ridge / groove | 一深一浅两条实线 |
| `outset` / `inset` | 整个框凸出来、凹进去 | outset / inset | 实线，两边深两边浅 |

几条线的线型，线宽按比例分给每条线和中间的空：`double`、`triple` 是线和空一样宽；`thinThickSmallGap` 是 1:1:3（细线、空、粗线），中空隙的空是 2，大空隙是 4。比如 `3pt thinThickSmallGap`：细线 0.6pt，空 0.6pt，粗线 1.8pt。

各格式怎么画：

- Word：直接写 `w:val`，Word 自己画。
- HTML：CSS 有的线型（solid、dashed、dotted、double、ridge、groove、inset、outset）直接用。CSS 没有的，先写一条同样宽的透明边占位，再用 `::before` 在上面盖一层 SVG 图案。
- PDF：实线、虚线、点线用 Typst 自带的 stroke。几条线的线型拆成几条实线来画：段落、文本框一层 block 套一层，每层画一条，拐角自己接得上；表格格子用硬边渐变当线的颜色，一条边画出几条线。这样线都是整条的，不会有图案拼接的缝。点划线、波浪线这些才用 SVG 图案铺，和 HTML 用的是同一份图案。
- Markdown：没有边框，线型直接忽略。

Word 的线宽只能在 0.25pt 到 12pt 之间（sz 2–96），Excel 只有四档，换算时就近取。

### 段落边框画在哪

段落（Text、Heading、Quote、Callout、Divider、Code、Math）的边框照 Word 画，文字不因为加了边框挪位置（2026-11-22 在 Windows Word 2024 上实测，线宽 0.5～3pt、内边距 0～10pt、缩进 0 和 36pt 都一样）：

- 文字留在左右缩进处。内边距（docx 的 w:space，整数 pt）和线都画在缩进外面，左右的线再往外让 1.5pt，所以框比版心宽，伸进页边距。1pt 线、4pt 内边距时，左线外沿在文字左边 6.5pt。
- 上下没有这 1.5pt：线整条在段落上面（下面），里沿到文字行盒是内边距。
- 没有线的那一边，Word 不留内边距，`padding` 写了也不算。
- Quote、Callout 在 docx 里把线宽和内边距加进了缩进，所以文字在缩进 + 内边距 + 线宽处，线的外沿在缩进外 1.5pt。
- pdf、html 照这样摆，docx 不用改，Word 自己就这么画。合成一个框的几段也一样。

没对上的：悬挂缩进时 Word 的线跟着悬挂后的左缩进走，pdf、html 按首行算；Word 的 `double` 这类几条线的线型，3pt 画出来总宽 9pt，我们总宽 3pt（线里沿对得上，外沿差）。

### 相邻段落合成一个框

Word 会把挨着的、边框设置相同的段落画成一个框（Rei 2026-11-21 定「按一个框来做」，以 Word 为准）。docx 每段照写 w:pBdr，Word 打开时自己合；pdf 和 html 按下面的规则分组，画成同样的一个框。Markdown 没有边框，不管。

2026-11-21 在 Windows Word 2024（16.0.17932）上实测，合不合只看两样：

- 四条边一样：线型、粗细、颜色、内边距（w:space）都相同。一边有一边没有也算不同。
- 左右缩进一样。Word 的左缩进含悬挂量；Quote、Callout 在 docx 里的缩进加了线宽和内边距，所以它们和写了同样边框的 Text 合不到一起。

这些不同都照样合：段前段后、底色、对齐、首行缩进、段落样式、字号、`borderBetween`。只写了 `borderBetween`、四边都没写的段落也会合。中间夹着表格、列表、没边框的段落（空段落也算）就断开；有边框的空段落不断开。参与合框的组件是 docx 里写成一个段落的那些：Text、Heading（开了编号的标题除外）、Quote、Callout、Divider、Code、Math。

合成以后怎么画（实测值，pdf、html 照着排）：

- 上边框只画在第一段上面，下边框只画在最后一段下面，左右边框连成一条。上下内边距只在框的顶上和底下各留一次，段与段之间没有。
- 第一段的段前、最后一段的段后在框外。
- 段与段之间空段前段后里大的那个（Word 不把两个加起来）。上一段的段后算上一段的，下一段段前比它多出来的部分算下一段的。
- 段与段之间画上面那段的 `borderBetween`，没写就不画。线画在上一段的段后下面，也就是两块的分界上。（Word 的 between 线自己也有 w:space，docx 里写的是 0，所以这里不用管。）
- 底色各段铺各段的那一块，左右铺到边框里沿，上下按上面的分法铺满，所以底色连成一片，中间不露白。
- 两条挨着的 Divider 只剩一条线（只画最后一条的下边），Word 里也是这样。

没做的：只有左边框、没有上下边框也没有底色时，Word 的竖线会把第一段的段前、最后一段的段后也包进去（底色在的时候又不包），pdf、html 的竖线只到框的上下沿。单独一段也是这样，所以默认的 Quote（段后 8pt）Word 的竖线比 pdf 长 8pt。跨页时 pdf 每页各画一圈完整的框，没按 Word 的样子处理。列表项里的段落、列表项 children 里的段落不参与分组。

## 五、背景（对应 w:shd、a:solidFill / a:gradFill / a:pattFill、xlsx fill）

```json
{ "background": "#F5F7FA" }
{ "background": { "pattern": "pct20", "color": "#000000", "fill": "#FFFFFF" } }
{ "background": { "gradient": "linear", "angle": 90, "stops": ["#FFFFFF", "#E8F0FE"] } }
```

三行分别是纯色、底纹、渐变。另外 `"background": "none"` 表示不画底色，用来去掉继承来的或内置的底色，比如表头默认的灰底。

| 写法 | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|
| 纯色 | w:shd val=clear fill | a:solidFill | patternFill solid | background-color | fill |
| 底纹 `pattern` | w:shd val（pct5…pct95、horzStripe 等） | a:pattFill prst | patternFill patternType（gray125、darkGrid 等） | 只能近似成纯色 | 近似成纯色 |
| 渐变 | 段落和单元格不支持，退成第一个色 | a:gradFill | gradientFill | linear-gradient | gradient.linear |
| 图片 | 不支持 | a:blipFill | 不支持 | background-image | 不支持 |

`pattern` 的名字取 Word 和 Excel 的交集，具体列表实现时定。

## 六、盒子（单元格、文本框、提示框这类容器）

| 字段 | 值 | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|---|
| `padding` / `paddingTop` … | 长度 | w:tcMar；段落用边框 space 近似 | a:bodyPr lIns tIns rIns bIns | — | padding | inset |
| `verticalAlign` | `top` `middle` `bottom` | w:vAlign | a:bodyPr anchor | vertical | vertical-align | align |
| `width` / `height` | 长度或百分比 | w:tcW、w:trHeight | a:xfrm ext | 列宽、行高 | width / height | width / height |
| `textRotation` | `0` `90` `270` | w:textDirection | a:bodyPr vert | textRotation | writing-mode | rotate |
| `radius` | 长度 | — | prstGeom roundRect | — | border-radius | radius |

## 七、列表（对应 w:numbering、a:pPr 的 bu*、CSS list-style）

Word 的列表定义在 numbering.xml：`w:abstractNum` 下每一级一个 `w:lvl`，段落用 `w:numPr` 引用。PPT 把项目符号直接写在段落的 a:pPr 上。

| 字段 | 值 | Word w:lvl | PPT a:pPr | HTML | PDF |
|---|---|---|---|---|---|
| `listStyleType` | 无序：`disc` `circle` `square` `dash` `none`，或任意字符如 `"▸"`；有序：`decimal` `decimal-leading-zero` `lower-alpha` `upper-alpha` `lower-roman` `upper-roman` `cjk-decimal`（一、二、三） `cjk-ideographic`（甲、乙、丙） `circled-decimal`（①②③） | w:numFmt（bullet、decimal、decimalZero、lowerLetter、upperLetter、lowerRoman、upperRoman、chineseCounting、ideographTraditional、decimalEnclosedCircle） + w:lvlText | a:buChar / a:buAutoNum type（arabicPeriod、alphaLcParenR、romanUcPeriod、circleNumDbPlain、ea1ChsPeriod …） | list-style-type | list marker / enum numbering |
| `listMarkerFormat` | 编号的模板，`%k` 是第 k 级的编号，和 Word 一样：一级 `"%1."` `"(%1)"` `"第%1条"`，二级 `"%2)"` `"%1.%2"`；指向更深级别的 `%k` 没有编号可填，去掉；本级是 `decimal` 时上几级也写成阿拉伯数字（上级「一、」，本级出「2.1」）；符号级别不用模板 | w:lvlText（+ w:isLgl） | 只能从 buAutoNum 的固定几种里选，选不到就退成 `%1.` | ::marker content / counters() | enum numbering 函数 |
| `listMarkerImage` | 图片路径，当项目符号 | w:lvlPicBulletId | a:buBlip | list-style-image | marker 里放 image |
| `listMarkerColor` | 颜色 | w:lvl 的 w:rPr color | a:buClr | ::marker color | marker 用 text fill |
| `listMarkerFont` | 字体名（Wingdings 这类符号字体） | w:lvl 的 w:rFonts | a:buFont | ::marker font-family | — |
| `listMarkerSize` | 百分比数字，相对正文 | w:lvl 的 w:sz | a:buSzPct | ::marker font-size | — |
| `listMarkerAlign` | `left` `center` `right`：编号在编号区里怎么对齐（1. 到 10. 右对齐时小数点能对齐） | w:lvlJc | — | ::marker text-align（支持差） | — |
| `listMarkerGap` | 编号和正文之间：长度，或 `tab` `space` `none` | w:suff + 制表位 | indent 和 marL 的差 | ::marker padding / gap | body-indent |
| `listIndent` | 每一级缩进多少（正文左边到哪） | w:ind left | marL（逐级） | padding-left | indent |
| `listHanging` | 编号区宽度，就是悬挂缩进 | w:ind hanging | indent（负数） | 负 text-indent | 用 body-indent 近似 |
| `listStart` | 有序列表从几开始；和 List 的 `continueFrom` 不能一起写 | w:start | buAutoNum startAt | start 属性 | enum start |
| `itemSpacing` | 长度：相邻两项之间的距离，可以按级写在 `levelN` 上 | 下一项 w:spacing before，上一项 after 写 0 | a:spcBef | 下一项 margin-top，上一项 margin-bottom 0 | 下一项 block above |

多级列表：List 的 props 里 items 可以嵌套，每一级的样式是「元素样式 + `levelN` 部位」。`levelN` 可以写在元素的 `parts` 里，也可以写成 `List.level2` 这样的组件样式键（见第十一节）。`listIndent` 每级自动累加。

决定编号长什么样的五个字段例外：`listStyleType`、`listMarkerFormat`、`listMarkerImage`、`listMarkerFont`、`listStart` 写在元素样式上只管第一级。第二级起它们只看自己的 `levelN`，没写就用这一级的默认：

- 编号类型：有序依次 `decimal` `lower-alpha` `lower-roman`，无序依次 `disc` `circle` `square`，三级一轮。这一级有序还是无序看 items 里的 `ordered`，没写跟上一级。
- 模板：`"%N."`（N 是这一级）。
- 起始：1。符号字体、图片符号：不用。

这样元素样式上写个 `decimal` 或 `"%1、"`，下级不会跟着变成「1、」「1、」，Word 里符号子层也不会把 `%2` 这类模板原样显示出来。其余列表字段（颜色、字号、对齐、间距、缩进）照旧管所有级别。

符号级别（`disc` `circle` `square` `dash` `none` 或自定字符）只显示符号，`listMarkerFormat` 写了也不用。Word 的 lvlText 在符号级别就是符号本身。

同一级下可以有的子列表有序、有的无序，四种格式都各显示各的类型，有序子列表都从 1 起。docx 的写法：每个列表一个 abstractNum，每一级的定义取这一级第一次出现的子列表；后面哪串子列表的定义和它不一样，就给这串单开一个 w:num，指向同一个 abstractNum，用 w:lvlOverride 换掉这一级的 w:lvl，加 w:startOverride 从头数。上几级还是同一个 abstractNum，父级编号照常连续。

### 项间距（`itemSpacing`）

`itemSpacing` 管相邻两项之间的距离，不管整个列表的 `spaceBefore`（第一项上面）和 `spaceAfter`（最后一项下面）。它跟别的列表字段一样会继承，元素样式上写一个就管所有级别，`levelN` 上写的只管那一级。

- 相邻两项之间用两项里级别深的那一级的值。父项和它的第一个子项之间、最后一个子项和下一个父项之间，都按子列表那一级算。
- 写了 `itemSpacing` 的地方不用 `item` 部位的 `spaceBefore`/`spaceAfter`。没写时照旧：项与项之间是 `item` 部位的段后和段前里大的（和 Word 一样）。
- docx：距离写在下一项的 w:spacing before 上，上一项的 after 写 0。Word 段前段后取大的，这样不会叠加，Word 里量出来正好是 `itemSpacing`。document-render 不写 w:contextualSpacing，不会把项间距吃掉。
- pdf：下一项的 block above。html：下一项 margin-top，上一项 margin-bottom 写 0。
- md：没有间距，忽略。
- parse：同一列表里每一级的项间距都一致时读回成 `itemSpacing`（最浅那一级写在元素上，别的级不一样才写进 `levelN`）；不一致时照旧读成 `item` 部位的段前段后。

### 接着编号（List 的 `continueFrom`）

两段列表中间隔着正文时，后一段的 props 写 `"continueFrom": "前一段的id"`，第一级从前一段最后一项之后接着数。前一段的起始号算在内（前一段 `listStart: 3`、三项，这段从 6 起）。只接第一级，子级跟 Word 一样在新的第一级项下从头数。可以链着接。

- 被接的必须是 List，而且在正文里出现在它前面；不存在、不是 List、在后面（包括指向自己）都会报错。页眉页脚里的列表不能用。
- 和 `listStart` 不能一起写（第一级的 `listStart` 不管写在元素、`level1` 还是组件样式上），一起写报错。
- docx：第一级定义和被接的列表一样时直接用它的 w:num，Word 里是同一个列表，真正接着数；不一样时（比如这段换成罗马数字）单开 abstractNum，第一级 w:start 写算好的号，数字对，但 Word 里不联动。
- pdf、html：算好起始号，直接写编号文字。md：写算好的数字。
- parse：正文里不挨着的两组段落用同一个 numId（或者同一个 abstractNum、后一个 num 第一级没写 startOverride）时，后一组读成带 `continueFrom` 的 List。

### 列表项里的块（项的 `children`）

列表项写成对象时，`children` 里按顺序写元素 id（`Text`、`Table`、`Image`、`Code`、`Math`、`Quote`、`Callout`、`Divider`），这些块排在这一项文字后面、子列表（`items`）前面，是这一项的一部分。和 Word 里「同一项按 Shift+Enter 或无编号续段」一样，块不带编号，左边对齐到这一项的文字。

- 左边位置 = 这一级的 `listIndent` × 级数，也就是这一级文字开始的地方。实现上就是在块自己的 `marginLeft` 上加这个数，块别的样式照常。
- docx：段落写 w:ind left（没有 hanging、firstLine，不带 w:numPr）；表格写 w:tblInd，撑满的表格宽度扣掉缩进；图片、代码、公式都在段落上缩进。
- pdf、html：块的左外边距加上缩进；撑满的表格宽度扣掉缩进。
- md：按 GFM 列表项的续块写，前面空一行，每行缩进到编号后文字的位置。
- 校验：引用的元素要存在、只能被引用一次、类型在上面的名单里，否则 check 报错，报错里带第几项（子列表的项写成「2.1」）。
- parse：一组编号段落里，紧跟在某项后面、没有编号、左缩进和那一项这一级的文字对齐（差不到 1 缇），而且没有首行缩进、悬挂缩进的段落，读回成这一项的 `children`；表格看 w:tblInd。标题、目录、带分节符的段落不算。判定不上的照旧读成列表后面的普通段落，宁可多断开，不乱并。

### 标题编号（`Heading`、`Heading.N` 上的列表字段）

标题的自动编号沿用列表字段，写在组件样式 `Heading`（所有级别）或 `Heading.N`（某一级）上，只看样式表里写的值，不看继承来的：

| 字段 | 标题上的意思 | 默认 |
|---|---|---|
| `listStyleType` | 这一级的数字写法；`none` 关掉 | `decimal` |
| `listMarkerFormat` | 模板，`%k` 是第 k 级标题的号 | 把 1 到 N 级里开了编号的级别用点连起来：`%1`、`%1.%2` |
| `listStart` | 起始号 | 1 |
| `listMarkerGap` | 编号后面：`space`、`none`、`tab`（长度按 `tab` 处理并警告） | `space` |
| `listHanging` | `tab` 时编号区宽度 | `3em` |
| `listMarkerColor` `listMarkerFont` `listMarkerSize` | 只改编号 | 跟标题文字 |

写了 `listStyleType`（不是 `none`）或 `listMarkerFormat` 的级别才开编号。计数和 Word 一样：本级加一，更深的级别回到起始号之前；上级从没出现过时按「起始号 − 1」填（先出现二级标题会是「0.1」）。本级是 `decimal` 时上几级也写成阿拉伯数字（Word 的 isLgl）。标题 props 写 `"numbered": false` 时不编号、不计数。Section 写 `"restartHeadingNumbering": true` 时各级从头数。编号字段写在单个标题的 `style` 里会报错（Word 的编号定义是全文一份，单个标题改不了）。

- docx：一个 abstractNum，9 级；开了编号的级别写 w:pStyle 绑到 `Heading1`…`Heading6`，标题样式的 w:pPr 里写 w:numPr 指回来，标题段落本身不写编号。这就是 Word 的「多级列表链接到标题样式」：增删标题自动重排，导航窗格、目录（TOC 域更新后）都带编号。`numbered: false` 的标题写 numId 0。重新编号的节单开一个 w:num，各级 w:startOverride，节里的标题直接写这个 numId。编号字体字号跟段落标记走，所以编号标题写段落标记的 w:rPr（标题文字的样式）。缩进：`space`、`none` 时 w:ind left 是标题的 `marginLeft`、firstLine 是 `textIndent`；`tab` 时 left = `marginLeft` + `listHanging`，hanging = `listHanging`。
- pdf：编号算好写进标题内容，正文、目录（outline）、PDF 书签里都有。`tab` 时编号放在宽 `listHanging` 的框里（编号更宽时框跟着变宽），标题悬挂缩进 `listHanging`。
- html：标题前面加 `<span class="document-render-heading-number">`；`tab` 时编号是宽 `listHanging` 的行内块，标题 `padding-left` 加负 `text-indent`。
- md：编号写进标题文字，`## 1.2 术语`。
- 目录：四种格式都带编号，编号和标题之间是一个空格（`none` 时没有）。docx 预先填好的目录项也是这样；Word 更新域后由 Word 重排。
- parse：标题样式上有 w:numPr（不是段落自己写的）时读成编号标题，不当列表：编号定义读回 `Heading.N` 的这些字段，缩进换回标题自己的 `marginLeft`、`textIndent`；段落写 numId 0 的读成 `"numbered": false`；标题直接指向同一个 abstractNum 的另一个 num、这个 num 有 startOverride 时，所在的节读成 `restartHeadingNumbering`。标题样式绑的编号级别和标题级别对不上（比如「标题 2」绑第 1 级）时不读编号，给警告。

## 八、图片位置（对应 wp:inline / wp:anchor、p:pic 的 a:xfrm、xlsx 的 xdr anchor）

| 字段 | 值 | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|---|
| `position` | `inline`（默认，跟文字走）`float`（浮动，文字绕排）`absolute`（按页面或幻灯片定位，不占位置） | wp:inline / wp:anchor | 都是绝对定位，inline 时按版式顺排 | twoCellAnchor / oneCellAnchor | 正常流 / float / position:absolute | 正常 / place(float: true) / place |
| `float` | `left` `right` | wp:anchor + positionH align | — | — | float | place(left / right) |
| `wrap` | `square` `tight` `topAndBottom` `none` `behind` `inFront` | wrapSquare / wrapTight / wrapTopAndBottom / wrapNone + behindDoc | — | — | float + shape-outside（tight 近似 square） | float: true 只有 topAndBottom 和 square 近似 |
| `wrapDistance` | 长度，或四边 `{ top, right, bottom, left }`，默认 6pt，四边写法里没写的边也是 6pt | distT / distR / distB / distL | — | — | 靠文字一边的 margin；写了时加 margin-top / bottom | 不绕排，写了时上下两边当段前段后 |
| `anchorTo` | `paragraph` `margin` `page`，PPT 里是 `slide` | positionH / positionV relativeFrom（column、margin、page） | 幻灯片 | 单元格 | 包含块 | place 的参照 |
| `x` / `y` | 长度：相对 `anchorTo` 的偏移 | wp:posOffset（EMU） | a:off（EMU） | xdr:from 列行 + 偏移 | left / top | dx / dy |
| `horizontalAlign` | `left` `center` `right` | positionH align | 按幻灯片宽算 | — | margin auto | place(center) |
| `verticalAlign` | `top` `middle` `bottom` | positionV align | 按幻灯片高算 | — | — | place(top / bottom) |
| `objectFit` | `contain` `cover` `fill`：宽高都给时怎么缩放 | 按比例算 extent；cover 用 a:srcRect 裁 | a:stretch / a:srcRect | 按比例算 | object-fit | fit |
| `rotation` | 角度 | a:xfrm rot | a:xfrm rot | — | transform | rotate |
| `opacity` | 0–1 | a:alphaModFix | a:alphaModFix | — | opacity | — |

图片的 `width`、`height` 继续放在 Image 的 props 里（它们是内容尺寸，不是样式），上面这些是样式。

Image 的 `alt` 是替代文字：docx 写进 wp:docPr 的 descr（Word 里「编辑替代文字」看到的就是它），HTML 是 img 的 alt，PDF 写进图片的 alt。图片放不进 docx 时（SVG、远程图片），改成显示 `[alt]`。

## 九、表格（对应 w:tblPr、w:trPr、xlsx 工作表）

这些是表格组件的样式，不是每个元素都能用。

| 字段 | 值 | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|---|
| `tableLayout` | `fixed` `auto` | w:tblLayout | 固定 | 固定 | table-layout | 列宽 |
| `cellPadding` | 长度 | w:tblCellMar | 逐格 | — | td padding | inset |
| `repeatHeader` | 布尔，默认开 | w:tblHeader | — | 打印标题行 | thead | table.header(repeat) |
| `rowBreak` | 布尔：行内能不能跨页 | w:cantSplit | — | — | break-inside | — |
| `bandSize` | 隔几行算一条带，默认 1 | w:tblStyleRowBandSize | — | — | nth-child(an+b) | fill 函数 |

表头、首行末行、奇偶行这些部位的样式见第十二节。

部位上可以写 `padding`、`paddingLeft` 这些，只改落在这个部位里的格子，没写的边还用 `cellPadding`。docx 写成这一格的 w:tcMar，HTML 是 td 的 padding，PDF 是这一格的 inset。

### 列宽

列宽写在 Table 的 props `widths` 里，一列一个值，三种写法可以混用：

- 数字：比例。固定列和 auto 列占完以后，剩下的宽度按比例分。
- 带单位的字符串：固定宽度，比如 `"5.65pt"`、`"20mm"`。不带单位的字符串（`"5"`）当 pt。
- `"auto"`：按格子里的文字撑开。

只要有一列是比例，表格就撑满版心（或撑到样式里的 `width`）。全是固定列和 auto 列时，表格宽度就是各列加起来，不撑满。比如章节标题条：`["5.65pt", "auto"]`，左边一条细色块，右边按标题长短变宽。

各格式怎么做：HTML 用自动布局，auto 列不写宽度；PDF 是 Typst 的 `auto` 列；docx 的 auto 列写 `w:tcW w:type="auto"`，表格宽度写 auto、布局改成 autofit，gridCol 按字数估一个初始宽度（全角字 1em、半角 0.6em，加字间距、左右 padding 和 3pt 余量），Word 打开后按内容重排。有 auto 列又没写 `tableLayout` 时，三种格式都按 `auto` 布局。

### 合并单元格、单格样式、行高

`rows` 和 `columns` 里的格子可以写字符串、run 数组，也可以写成对象：

```json
{ "text": "检测结果", "rowSpan": 2, "colSpan": 3, "style": { "borderLeft": "6pt solid #8DC63F" } }
```

- `text`（也可以叫 `runs`）：字符串或 run 数组。
- `rowSpan`、`colSpan`：往下、往右合并几格，含自己。被合并掉的位置在后面的行里不用写，行里少写几格就行。超出表格或互相重叠会报错或给警告。
- `style`：只给这一格的样式，叠在部位样式之后。合并格的边框按整块算：上边框写在首行，下边框写在末行。docx 是 w:gridSpan、w:vMerge，HTML 是 rowspan、colspan，PDF 是 table.cell 的 rowspan、colspan。

行高写在 props `rowHeights`，一行一个值：

- 长度（`"28.35pt"`、数字当 pt）：最小高度，内容多了会撑高。docx 写 `w:hRule="atLeast"`，HTML 是 tr 的 height，PDF 只对单行文字的格子有效。
- `{ "height": "51.05pt", "rule": "exact" }`：固定高度，内容多了会被裁掉。docx 写 `w:hRule="exact"`，PDF 是 Typst 的 rows。
- `null`：这一行不管。

### 行内图片

run 里写 `image` 就是行内图片，格子、段落、页眉页脚里都能用：

```json
{ "image": "images/logo.png", "width": 78.4, "height": 39.3 }
```

`width`、`height` 单位 pt，只写一个时按图片比例算另一个。`text` 是替代文字，不显示。docx 是 wp:inline，替代文字写进 wp:docPr 的 descr；HTML 是 img，替代文字是 alt；PDF 是 box(image)，替代文字写进 image 的 alt。

## 十、页面（`meta.page`，对应 w:sectPr）

| 字段 | 值 | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|---|
| `size` | `A4` `Letter` …，或 `{ width, height }` | w:pgSz | p:sldSz | pageSetup paperSize | @page size | page width / height |
| `landscape` | 布尔 | w:pgSz orient | — | orientation | @page | flipped |
| `margin` / `marginTop` … | 长度 | w:pgMar | — | pageMargins | @page margin | margin |
| `headerDistance` / `footerDistance` | 长度，默认 0.5in | w:pgMar header / footer | — | pageMargins header / footer | 页眉页脚框离纸边的距离（`--paged`） | 页眉页脚框的内边距 |
| `gutter` | 长度 | w:pgMar gutter | — | — | — | binding |
| `columns` | 数字，或 `{ count, gap }` | w:cols | — | — | columns | columns |
| `verticalAlign` | `top` `center` | w:vAlign | — | — | — | — |

`meta.page` 是第一节的设置。另外几个字段管页眉页脚和页码：

| 字段 | 值 | Word | HTML（`--paged`） | PDF |
|---|---|---|---|---|
| `header` / `footer` | 元素 id，或 `{ default, first, even }` | w:headerReference / w:footerReference | `<template>`，排完版由脚本放进页边距 | page header / footer |
| `pageNumberStart` | 数字 | w:pgNumType start | 脚本数页码时从这里重新数 | 自己数页码 |
| `pageNumberFormat` | `decimal`（默认）`lowerRoman` `upperRoman` `lowerLetter` `upperLetter` `chineseCounting` | w:pgNumType fmt | 脚本按格式写页码 | 按格式写页码 |

### 分节

`Section` 是容器组件，只能直接放在 root 下面。每一节从新的一页开始，可以改纸张、方向、页边距、分栏、页眉页脚，也可以让页码重新开始、换页码格式。`Section` 的 props 就是上表的这些字段，都可以不写，不写的沿用上一节，跟 Word 的「链接到前一节」一样。只有 `pageNumberStart` 不沿用：没写就接着上一节往下数。

root 下面不写 `Section` 也行，整份文档就是一节，用 `meta.page` 的设置。也可以开头一段不放进 `Section`，算作第一节，后面全用 `Section`；但 `Section` 后面不能再跟不在 `Section` 里的内容。

```json
"elements": {
  "doc":   { "type": "Document", "children": ["cover", "body"] },
  "cover": { "type": "Section", "props": { "header": { "first": null }, "pageNumberFormat": "lowerRoman" }, "children": ["title"] },
  "body":  {
    "type": "Section",
    "props": { "pageNumberStart": 1, "pageNumberFormat": "decimal", "header": { "default": "hdr-odd", "even": "hdr-even" } },
    "children": ["ch1", "p1"]
  },
  "foot":  { "type": "Text", "props": { "text": ["第 ", { "field": "page" }, " 页，共 ", { "field": "pages" }, " 页"] } }
}
```

### 页眉页脚

- 写一个元素 id，等于只改 `default`（普通页用的那个）。
- 写对象时分开设：`default` 普通页，`first` 本节第一页，`even` 偶数页。写 `null` 表示这一种留空，不写表示沿用上一节。
- 只要设了 `first`（包括 `null`），本节首页就单独处理（Word 的「首页不同」）；页眉和页脚共用这个开关，只给页眉设了 `first` 时，首页的页脚用 `default`。
- 任何一节设了 `even`，整份文档就开「奇偶页不同」（Word 里这是整个文档的开关），没设 `even` 的节偶数页用 `default`。
- `"header": null` 写在 `Section` 上，表示清掉从上一节接过来的全部页眉。
- 页眉页脚里的元素跟正文一样写在 `elements` 里，但不能同时出现在正文里，也不能放 PageBreak、ColumnBreak、Section、Slide。样式继承根元素（Document）的样式。
- 位置照 Word（2026-11-22 Windows Word 2024 实测）：页眉第一行的行盒顶在页眉距处，第一段的段前加在下面；页脚最后一段的段后也算，段后的下沿对齐页脚距，所以 Text 默认段后 6pt 的页脚比只看行盒高 6pt。以前 pdf 丢了段后，页脚比 Word 低约 4～6pt；改后差 0.2pt 以内（Word 自己按 1/600 英寸取整）。
- html（`--paged`）：页眉页脚放进页边距后带上根元素的字体、字号；浏览器把行高多出来的部分上下平分，Word 不平分，所以按第一行（页眉）、最后一行（页脚）把框挪到 Word 的基线上。浏览器按整像素排，还会差半个像素左右。
- 没做的：页眉、页脚比页边距高时，Word 会把正文往下（往上）挤，pdf、html 不挤，内容会和正文叠在一起。

### 页码

runs 里写 `{ "field": "page" }` 是当前页码，`"pages"` 是全文总页数，`"sectionPages"` 是本节页数。可以带文字样式；`text` 可以不写，写了就是 Word 打开时更新前显示的值。页码按 Word 的规则数：每节接着上一节往下数，节上写了 `pageNumberStart` 就从那个数重新开始；奇偶页按页码数，不按物理页数。

`pageNumberFormat` 管页码用什么数字，名字跟 Word 一样：

| 值 | 样子 |
|---|---|
| `decimal` | 1, 2, 3 |
| `lowerRoman` | i, ii, iii, iv |
| `upperRoman` | I, II, III, IV |
| `lowerLetter` | a, b, …, z, aa, bb（过了 z 重复字母，跟 Word 一样） |
| `upperLetter` | A, B, …, Z, AA, BB |
| `chineseCounting` | 一、二、三……十、十一、二十 |

- 只有 `page` 跟格式走。`pages` 和 `sectionPages` 一直是阿拉伯数字，跟 Word 的 NUMPAGES、SECTIONPAGES 一样。
- pdf 的目录（TableOfContents）也按标题所在节的格式和起始页码写页码。
- 页码是 0 或负数时没有罗马数字、字母写法，照阿拉伯数字写。
- 常见用法：前言一节写 `lowerRoman`、`pageNumberStart: 1`，正文一节写 `decimal`、`pageNumberStart: 1`。例子见 `examples/frontmatter.json`。

各格式的差别：

- docx：写成 Word 的域（PAGE、NUMPAGES、SECTIONPAGES），Word 打开后自己算。LibreOffice 不认 SECTIONPAGES，只显示 `text` 的值。
- pdf：Typst 排完版后按上面的规则算好。
- html：直接打开或打印时没有页眉页脚，页码显示 `text` 的值；加 `--paged` 后由脚本排完版再放页眉页脚、按格式填页码。目录的页码由 Paged.js 自己数，不跟 `pageNumberStart` 和 `pageNumberFormat`。屏幕上每节按自己的纸张显示，但打印时 Paged.js 只用第一节的纸张，横竖混排的文档打出来横向那几页会被裁掉，要打印请用 pdf。

### 分栏符

分栏的节里放一个 `{ "type": "ColumnBreak" }`，后面的内容从下一栏顶上开始，跟 Word 的「分栏符」一样。没有 props。

- docx：写成 `<w:br w:type="column"/>`，单独占一段。
- pdf：Typst 的 `colbreak()`。
- html：后面的元素加 `break-before: column`。
- 节没分栏时：docx 和 pdf 当分页（Word、Typst 都这样），html 忽略。
- Markdown 没有分栏，丢掉并给一次警告。
- 页眉页脚里不能放。

## 十一、样式集和继承

### 样式集

样式集是一整套外观：全局样式加各个组件的样式，组件样式继承全局样式。同一份文档换一个样式集，就换一套外观。

```json
{
  "styleSet": "formal",
  "styleSets": {
    "formal": {
      "global": {
        "fontFamily": { "latin": "Times New Roman", "eastAsia": "宋体" },
        "fontSize": 12,
        "lineHeight": 1.5
      },
      "components": {
        "Heading": { "fontFamily": { "eastAsia": "黑体" }, "bold": true },
        "Heading.1": { "fontSize": 22, "align": "center" },
        "Text": { "textIndent": "2em" },
        "Table.header": { "background": "#F2F2F2", "bold": true },
        "List.level2": { "listStyleType": "circle" }
      },
      "styles": { "muted": { "color": "#666666" } }
    },
    "modern": {
      "extends": "formal",
      "global": { "fontFamily": { "latin": "Inter", "eastAsia": "思源黑体" }, "fontSize": 11 }
    }
  },
  "styles": { "Heading.2": { "color": "#1A73E8" }, "warn": { "color": "#D93025" } }
}
```

- `styleSet`：这份文档用哪个样式集；命令行 --style-set 可以临时换
- `global`：全局样式：所有组件都继承
- `components`：组件样式：键是组件名，或「组件名。部位」
- `styles`：这个样式集里的命名样式，元素用 style: muted 引用
- `extends`：在 formal 基础上改
- `styles`：不属于任何样式集：哪个样式集都生效

- 样式集可以写在文档里，也可以单独成 JSON 文件（文件里要写 `name`），放在文档旁边的 `styles/` 目录会自动读，也可以用 `--style-sets` 指定文件或目录。这样一套公司模板能给很多文档用。文档里写的同名样式集优先。
- 顶层 `styles` 不属于任何样式集。键是组件名或「组件名。部位」时是组件样式，其他是命名样式。
- 样式集之间用 `extends` 继承，只能单继承，成环报错。
- `global` 里只放会继承的字段（文字和段落），写边框、背景这类字段会报错，因为它们不该铺到每个组件上。

#### 变量

`global` 里以 `$` 开头的键是变量，不是样式字段。用的地方写 `${名字}`：

```json
"global": { "$color1": "#8DC63F", "$color2": "#666666", "$line": "1pt solid ${color1}", "fontSize": 11 },
"components": { "Heading": { "color": "${color1}" } },
"styles": { "box": { "borderBottom": "${line}" } }
```

- 变量能在样式集的 `components`、`styles`、`global`，顶层 `styles`，元素的 `style`、`parts`、单格 `style`，以及组件 props（比如 run 的 `color`、自定义组件的参数）里用。
- 整个值就是 `${x}` 时，换成变量原来的值，数字、对象都行；夹在别的文字里时按文字拼进去，这时变量只能是文字或数字。
- 变量值里也能引用别的变量，成环会报错；没定义的变量也报错。
- `extends` 的子样式集能用父样式集的变量，同名的子样式集覆盖父的。
- 要写字面的 `${`，写成 `$${`。
- 没选样式集时没有变量。

组件样式的键：

| 键 | 管什么 |
|---|---|
| `Heading`、`Text`、`List`、`Table`、`Image`、`Slide` | 这类组件的全部元素 |
| `Heading.1` … `Heading.6` | 某一级标题 |
| `List.level1` … `List.level9`、`List.marker` | 某一级列表、编号本身 |
| `Table.header`、`Table.firstRow` … `Table.evenColumn` | 表格各部位，见第十二节 |
| `Image.caption` | 图片说明 |
| 自定义组件名 | 加在展开后的模板根元素上 |

### 继承顺序

一个元素的样式按下面的顺序叠，后面的覆盖前面的：

1. 父元素传下来的文字、段落、列表字段（像 CSS 继承；边框、背景、盒子、图片位置字段不继承）。根元素的「父元素」是 document-render 内置的全局默认值，再叠上样式集的 `global`（有 `extends` 时先叠父样式集的）。
2. document-render 内置的组件默认样式：先 `Heading`，再 `Heading.1`。
3. 样式集的组件样式：先 `Heading`，再 `Heading.1`。
4. 顶层 `styles` 里的组件样式：同样先 `Heading` 再 `Heading.1`。
5. 自定义组件模板里写的 `style`（内层模板在前，外层模板给实例写的在后）。
6. 指名这个自定义组件的组件样式：`PriceTable`（只管模板根元素）和 `PriceTable:t`（管模板里的元素 `t`）。
7. 元素自己的 `style`：样式名、样式名数组、样式对象，后面的覆盖前面的。样式名先在当前样式集的 `styles` 里找，找不到再到顶层 `styles` 找。
8. runs 上的文字字段：先是元素的文字样式，行内代码再叠 `InlineCode` 组件样式，行内公式叠 `InlineMath` 组件样式，链接默认蓝色加下划线，然后是 `bold` 这类简写，最后是 run 自己的 `style`。

几点说明：

- 第 1 步排最低：组件样式总能盖过从父元素继承来的值，和 CSS 一样。
- 第 4 步排在第 3 步后面：样式集当底子，文档自己写的组件样式优先。
- 第 5 步排在第 3、4 步后面：自定义组件的设计不会被通用的 `Table` 样式冲掉；要改它，用第 6 步的键指名。
- 部位样式（第十二节）按同样的顺序叠：内置 → 样式集 → 顶层 styles → 模板 `parts` → `组件名:元素id.部位` → 元素自己的 `parts`。
- runs 上的 `style` 只能写文字字段，外加 `background`（行内底色）。

docx 现在把算好的样式直接写在段落和 run 上（直接格式）；标题另外挂 Word 的「标题 1」到「标题 6」样式（导航窗格和目录能用），代码挂 `Code`、`InlineCode` 样式名。把样式集整体写成 Word 样式表，留到以后。

## 十二、部位样式和组件结构

### 部位

一个元素可以分成几个部位，每个部位单独设样式。表格的部位照 Word 表格样式的条件格式（w:tblStylePr）来定：

| 部位 | 管什么 | Word w:tblStylePr type | Excel 表格样式 | HTML | PDF |
|---|---|---|---|---|---|
| `table` | 整张表 | 表格样式本身 | wholeTable | table | table |
| `header` | 表头（columns 那一行），等于 `firstRow` | firstRow | headerRow | thead tr | table.header |
| `firstRow` / `lastRow` | 第一行、最后一行（如合计行） | firstRow / lastRow | headerRow / totalRow | tr:first-child / :last-child | fill、stroke 函数按行号判断 |
| `firstColumn` / `lastColumn` | 第一列、最后一列 | firstCol / lastCol | firstColumn / lastColumn | td:first-child / :last-child | 同上 |
| `oddRow` / `evenRow` | 奇数行、偶数行（不算表头） | band1Horz / band2Horz | firstRowStripe / secondRowStripe | tbody tr:nth-child(odd / even) | 同上 |
| `oddColumn` / `evenColumn` | 奇数列、偶数列 | band1Vert / band2Vert | firstColumnStripe / secondColumnStripe | nth-child | 同上 |
| `topLeftCell` `topRightCell` `bottomLeftCell` `bottomRightCell` | 四个角的格子 | nwCell / neCell / swCell / seCell | — | 组合选择器 | 同上 |
| `cell` | 每个格子 | 表格样式的 w:tcPr | — | td | table.cell |

一个格子同时落在几个部位时，按 Word 的顺序叠：`table` → `cell` → 奇偶列 → 奇偶行 → 首末列 → 首末行 → 四角，后面的覆盖前面的。

表头默认粗体、浅灰底（#F3F3F3）。不要灰底就在表头部位写 `"background": "none"`，三种格式都不画底色；某一格自己写的 `background` 还是照画：

```json
"parts": { "header": { "background": "none" } }
```

哪些部位生效用 Table 的 props `look` 开关，对应 Word 的 w:tblLook：

```json
{
  "props": {
    "look": {
      "firstRow": true,
      "lastRow": true,
      "firstColumn": false,
      "bandedRows": true,
      "bandedColumns": false
    }
  }
}
```

docx 里各部位的样式现在逐格写成直接格式。docx-rs 还不支持 w:tblStylePr，所以还生成不了真正的 Word 表格样式，以后补。

其他组件的部位：

| 组件 | 部位 |
|---|---|
| List | `level1` … `level9`、`marker`、`item` |
| Image | `caption` |
| Slide | `title`、`body` |
| Heading | 用 `Heading.1` … 区分级别，不算部位 |

### 部位样式写在哪

三个地方都能写，写法一样，部位名做键：

```json
{
  "prices": {
    "type": "Table",
    "props": {
      "columns": ["品名", "单价"],
      "rows": [["苹果", "3.5"], ["梨", "4.0"], ["合计", "7.5"]]
    },
    "style": { "border": "0.5pt solid #CCCCCC" },
    "parts": {
      "header": { "background": "#1A73E8", "color": "#FFFFFF", "bold": true },
      "evenRow": { "background": "#F5F7FA" },
      "lastRow": { "bold": true, "borderTop": "1pt solid #333333" }
    }
  },
  "styles": {
    "Table.header": { "bold": true },
    "Table.oddRow": { "background": "#FAFAFA" }
  }
}
```

- `style`：CCCCCC" }
- `lastRow`：333333" }

### 组件自带结构和样式

自定义组件的模板本身就是结构：里面放一张表，表的部位样式直接写在模板元素的 `parts` 上，用这个组件的人只管传数据。

```json
{
  "name": "PriceTable",
  "description": "报价表：蓝色表头、斑马纹、合计行加粗",
  "props": { "rows": { "type": "array", "required": true } },
  "template": {
    "root": "t",
    "elements": {
      "t": {
        "type": "Table",
        "props": {
          "columns": ["品名", "数量", "单价", "金额"],
          "rows": "{{rows}}",
          "look": { "lastRow": true }
        },
        "style": { "border": "0.5pt solid #D0D7DE" },
        "parts": {
          "header": { "background": "#1A73E8", "color": "#FFFFFF", "bold": true },
          "evenRow": { "background": "#F5F7FA" },
          "lastRow": { "bold": true, "borderTop": "1pt solid #333333" },
          "lastColumn": { "align": "right" }
        }
      }
    }
  }
}
```

- `style`：D0D7DE" }
- `lastRow`：333333" }

样式集要改某个自定义组件里面的样式，用「组件名：模板元素 id」，再接部位：

```json
{
  "styleSets": {
    "dark": {
      "components": {
        "PriceTable": { "fontSize": 10 },
        "PriceTable:t.header": { "background": "#333333" }
      }
    }
  }
}
```

- `PriceTable`：组件根元素
- `PriceTable`：组件里表格 t 的表头

组件自带的样式排在通用组件样式之后（见第十一节第 5 步）：`Table` 这类通用样式改不动它，要改就用 `PriceTable`、`PriceTable:t.header` 这样指名的键，或者在元素上直接写。

## 十三、代码：InlineCode 和 Code

代码不靠猜，也不靠字体判断，用组件明确标出来：

- **InlineCode**：行内代码，写在文字的 runs 里，和普通 run 并列。取代现在 run 上的 `code: true`。
- **Code**：整段代码，是块级组件，和 Text 并列。

```json
{
  "intro": {
    "type": "Text",
    "props": {
      "text": [
        "运行",
        { "type": "InlineCode", "text": "document-render render doc.json -o out.pdf" },
        "生成 PDF。"
      ]
    }
  },
  "sample": {
    "type": "Code",
    "props": {
      "language": "rust",
      "code": "fn main() {\n    println!(\"你好\");   // 中文注释走 eastAsia 字体\n}\n",
      "lineNumbers": false
    }
  }
}
```

- `language`：只做标记，第一版不做语法高亮

字体和外观写在组件样式里，跟着样式集走：

```json
{
  "styleSets": {
    "formal": {
      "components": {
        "InlineCode": {
          "fontFamily": { "latin": "JetBrains Mono", "eastAsia": "思源黑体" },
          "background": "#F3F4F6"
        },
        "Code": {
          "fontFamily": { "latin": "JetBrains Mono", "eastAsia": "思源黑体" },
          "fontSize": 9,
          "background": "#F6F8FA",
          "padding": 8
        }
      }
    }
  }
}
```

| | Word | PPT | Excel | HTML | PDF |
|---|---|---|---|---|---|
| InlineCode | 字符样式（w:style type="character"，名为 InlineCode） | a:rPr 直接写字体 | 单元格富文本里的一段 | `<code>` | `raw(block: false)` |
| Code | 段落样式（名为 Code），保留空格和换行（xml:space="preserve"，换行用 w:br） | 文本框 | 不支持，退成文本 | `<pre><code>` | `raw(block: true)` |

- 内置默认样式给它们一套等宽字体回退：macOS 用 Menlo，Windows 用 Consolas，Linux 用 DejaVu Sans Mono。
- InlineCode 只能放文字类 props（Text、Heading、List 项、表格格子），不能单独当元素用。
- `language` 第一版只写进 HTML 的 `class="language-rust"`，以后再考虑高亮。
- 行高按代码自己的字体和字号算，规则同「行距」一节。Word 里代码段落的字号就是代码字号，默认 9pt × 1.4，每行至少 12.6pt。Word 逐行看有没有中文：带中文的行算上东亚字体，会高一些，其他行只算等宽字体。pdf 照这样逐行算；html 给 `<pre>` 写等宽字体的行高，带中文的行再包一层写自己的行高，浏览器按整像素排，单行会差半像素左右（2026-11-22 Windows Word 2024 实测）。

## 十四、公式：InlineMath 和 Math

公式写 LaTeX，和代码一样分行内、块级两种：

- **InlineMath**：行内公式，写在文字的 runs 里，`text` 是 LaTeX 源码，不用写 `$`。
- **Math**：块级公式，单独一行，默认居中。`latex` 是源码，不用写 `$$`；`number` 可选，写了就在右边显示「(1)」这样的编号。

```json
{
  "intro": {
    "type": "Text",
    "props": {
      "text": ["勾股定理 ", { "type": "InlineMath", "text": "a^2+b^2=c^2" }, "。"]
    }
  },
  "root-formula": {
    "type": "Math",
    "props": { "latex": "x = \\frac{-b \\pm \\sqrt{b^2-4ac}}{2a}", "number": 1 },
    "style": { "fontSize": 12 }
  }
}
```

- JSON 里反斜杠要写两个：`\\frac` 才是 LaTeX 的 `\frac`。
- 样式照常层叠：Math 内置 `align: center`、`spaceBefore: 6`、`spaceAfter: 6`，`fontSize`、`color`、`align` 都能改；行内公式跟着所在段落的文字样式走，也能单独设 `InlineMath` 组件样式。
- `document-render check` 会把解析不了的公式列成警告。渲染时这种公式显示 LaTeX 原文，不会让整个文件出错。

能用的写法以 [math-core](https://github.com/tmke8/math-core) 为准，常见的都行：上下标、`\frac`、`\sqrt`、`\sqrt[3]{}`、`\sum`、`\prod`、`\int` 带上下限、`\lim`、希腊字母、`\mathbf`、`\mathbb`、`\text`、`\left( \right)`、`pmatrix`/`bmatrix` 这类矩阵、`cases`、`\binom`、`\hat`/`\vec`/`\bar` 这类重音、`\overline`、`\underbrace`。

| | Word | HTML | PDF | Markdown |
|---|---|---|---|---|
| InlineMath | 段落里的 `<m:oMath>`，夹在普通 run 中间 | `<math>`（MathML） | `$…$` | `$…$` |
| Math | 一段里放 `<m:oMathPara>`，对齐写在 `m:jc` 上 | `<math displaystyle="true">`，外层 div 管对齐 | `$display(…)$` | `$$` 单独成块，编号写成 `\tag{1}` |

几个格式各自的情况：

- **docx**：转成 Word 的公式（OMML），在 Word 里双击就能改。求和、积分是 `m:nary`，分数 `m:f`，根号 `m:rad`，矩阵 `m:m`，能拉伸的括号 `m:d`，重音 `m:acc`，上下划线 `m:bar`，上下大括号 `m:groupChr`，`\lim` 这类下面带字的是 `m:limLow`。公式字体用 Cambria Math，字号、颜色、粗体跟着样式。带编号的块级公式是一段里放居中、右对齐两个制表位，公式这时按行内大小排（分数会小一点）。行内公式不能带链接，写了 `href` 也不加。
- **pdf**：转成 Typst 公式，字体用 Typst 自带的 New Computer Modern Math，`\text{}` 里的中文往后找正文字体。万一转出来的源码 Typst 排不了，整份文档的公式改成显示 LaTeX 原文再排一次，并给警告。
- **html**：MathML 直接写进页面，不用 MathJax 这类脚本，离线也能看。括号拉伸、大号求和符号要靠系统里的公式字体：macOS 有 STIX Two Math，Windows 有 Cambria Math，Linux 没装公式字体时会显示成普通大小。
- **Markdown**：GitHub 的写法。Markdown 没有对齐写法，`align` 不管用，块级公式按 GitHub 默认居中显示。表格格子里公式里的 `|` 没有转义，会把格子拆开，写绝对值时用 `\lvert x \rvert`。

docx 和 pdf 认不出的结构（比如左上下标）退成纯文字，并给一次警告。

