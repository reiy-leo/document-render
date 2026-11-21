//! PDF 渲染器：先把元素树写成 Typst 标记，再用 typst 库编译成 PDF。
//!
//! 文字一律写成 Typst 字符串字面量（`#"..."`），不用转义标记语法。
//! 图片通过虚拟文件 `/assets/N.ext` 交给 typst。
//! 每个元素写成一个 block，样式用 block 里的 set 规则表达，只写和父元素不同的部分。

use std::{collections::HashMap, fmt::Write, sync::OnceLock};

use typst::{
    Library, LibraryExt, World,
    diag::{FileError, FileResult},
    foundations::{Bytes, Datetime, Duration},
    syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot},
    text::{Font, FontBook, FontInfo, FontWeight},
    utils::LazyHash,
};
use typst_kit::fonts::{self, FontStore};
use typst_layout::PagedDocument;

use crate::{
    assets,
    components::{
        CodeProps, Component, Field, ImageProps, Inline, ListProps, Run, TableOfContentsProps,
        TableProps,
    },
    doc::{Doc, HfNodes, Node, PageNumberFormat, SectionLayout},
    render::{
        Options, Output,
        common::{self, Grid, Warn},
        metrics,
    },
    style::{
        Align, AnchorTo, Background, Border, BorderStyle, Caps, ColWidth, Color, Edge, HAlign, Len,
        LineHeight, MarkerGap, Position, Side, Strike, Style, UnderlineStyle, VerticalAlign, Wrap,
        WrapMode,
    },
};

const FORMAT: &str = "pdf";

/// 默认字体顺序：西文无衬线在前，中文按 macOS、Linux、Windows 常见字体排，
/// 最后是 typst 自带字体兜底。typst 按字形逐个往后找。
/// 前两组和 fonts::DEFAULT_LATIN、DEFAULT_CJK 一样（有测试对着）。
pub(crate) const FONTS: &[&str] = &[
    "Helvetica Neue",
    "Arial",
    "Liberation Sans",
    "PingFang SC",
    "Hiragino Sans GB",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
    "Libertinus Serif",
];
const MONO_FONTS: &[&str] = crate::fonts::DEFAULT_MONO;
/// 公式字体，typst 自带。
const MATH_FONT: &str = "New Computer Modern Math";

pub fn render(doc: &Doc, tree: &Node, opts: &Options) -> anyhow::Result<Output> {
    let m = markup_with(doc, tree, opts, false);
    let (mut source, mut warnings) = (m.out, m.warn.list);
    if std::env::var_os("DOCUMENT_RENDER_DEBUG_TYPST").is_some() {
        eprintln!("{source}");
    }
    let mut world = DocumentRenderWorld::new(source.clone(), m.files);
    let mut warned = typst::compile::<PagedDocument>(&world);
    // 公式转出来的 Typst 源码万一排不了，整份文档的公式都改成显示 LaTeX 原文再排一次，不让整个 PDF 失败
    if warned.output.is_err() && m.had_math {
        let msgs: Vec<String> = match &warned.output {
            Err(errs) => errs.iter().map(|e| e.message.to_string()).collect(),
            Ok(_) => Vec::new(),
        };
        let again = markup_with(doc, tree, opts, true);
        source = again.out;
        warnings = again.warn.list;
        warnings.push(format!(
            "pdf：公式排版失败（{}），所有公式改成显示 LaTeX 原文",
            msgs.join("；")
        ));
        world = DocumentRenderWorld::new(source.clone(), again.files);
        warned = typst::compile::<PagedDocument>(&world);
    }
    for w in warned.warnings.iter() {
        // 字体找不到的情况渲染前已经查过（crate::fonts），报错或给了警告；默认字体列表本来就是按平台挑着用的
        let msg = w.message.to_string();
        if msg.starts_with("unknown font family: ") {
            continue;
        }
        let w = format!("typst：{msg}");
        if !warnings.contains(&w) {
            warnings.push(w);
        }
    }
    let document = warned.output.map_err(|errs| {
        let msgs: Vec<String> = errs.iter().map(|e| e.message.to_string()).collect();
        anyhow::anyhow!("typst 排版失败：{}", msgs.join("；"))
    })?;
    let bytes = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default()).map_err(|errs| {
        let msgs: Vec<String> = errs.iter().map(|e| e.message.to_string()).collect();
        anyhow::anyhow!("生成 PDF 失败：{}", msgs.join("；"))
    })?;
    Ok(Output { bytes, warnings })
}

/// 图片虚拟文件：路径和内容。
pub type Files = Vec<(String, Vec<u8>)>;

/// 生成 Typst 源码。返回源码、图片文件、警告。
/// 页码函数，见 preamble。
const FIELDS_FN: &str = r#"#let document-render-num(n, fmt) = if n < 1 or fmt == "1" { str(n) } else if fmt == "a" or fmt == "A" {
  str.from-unicode((if fmt == "a" { 97 } else { 65 }) + calc.rem(n - 1, 26)) * (calc.quo(n - 1, 26) + 1)
} else { numbering(fmt, n) }
#let document-render-info(loc: none) = {
  let p = if loc == none { here().page() } else { loc.page() }
  let num = 1
  let fmt = "1"
  let prev = none
  let next = none
  for m in query(<document-render-sec>) {
    let sp = m.location().page()
    if sp > p { next = sp; break }
    let v = m.value
    if v.start != none { num = v.start } else if prev != none { num = num + sp - prev }
    fmt = v.fmt
    prev = sp
  }
  if prev == none { prev = 1 }
  let end = locate(<document-render-end>).page()
  (
    page: num + p - prev,
    page-text: document-render-num(num + p - prev, fmt),
    first: prev == p,
    total: end,
    section-pages: (if next == none { end + 1 } else { next }) - prev,
  )
}
"#;

pub fn markup(doc: &Doc, tree: &Node, opts: &Options) -> (String, Files, Vec<String>) {
    let m = markup_with(doc, tree, opts, false);
    (m.out, m.files, m.warn.list)
}

/// `math_fallback` 为 true 时公式一律显示 LaTeX 原文（等宽字体）。
fn markup_with<'a>(doc: &Doc, tree: &Node, opts: &'a Options, math_fallback: bool) -> Markup<'a> {
    let mut m = Markup {
        out: String::new(),
        files: Vec::new(),
        warn: Warn::default(),
        opts,
        margins: [0.0; 4],
        content_width: 0.0,
        cur_id: String::new(),
        math_fallback,
        had_math: false,
        marker_font: None,
        heading_numbering: common::heading_numbering(tree).cloned(),
    };
    m.preamble(doc);
    let base = Style::default();
    m.node(tree, &base);
    m.out.push_str("#metadata(none) <document-render-end>\n");
    m
}

// Word 在节开头和手动分页之后保留段前距，typst 会把页顶的 above 吞掉；放一个零高的块垫着
const TOP_ANCHOR: &str = "#block(height: 0pt, above: 0pt, below: 0pt)\n";

struct Markup<'a> {
    out: String,
    files: Files,
    warn: Warn,
    opts: &'a Options,
    /// 上右下左页边距，pt
    margins: [f32; 4],
    /// 当前节版心（一栏）的宽度，pt；auto 列和比例列混写时用来分宽度
    content_width: f32,
    /// 正在排的元素 id，公式出错时报给用户
    cur_id: String,
    /// 公式一律显示 LaTeX 原文
    math_fallback: bool,
    /// 文档里有没有公式
    had_math: bool,
    /// 正在排的列表项的编号字体（listMarkerFont）和编号字号相对段落字号的比例，
    /// Word 量行高时把编号也算进去
    marker_font: Option<(String, f32)>,
    /// 标题编号定义（从根 Document 上取）
    heading_numbering: Option<crate::doc::HeadingNumbering>,
}

impl Markup<'_> {
    fn preamble(&mut self, doc: &Doc) {
        let o = &mut self.out;
        // 页码这类域都靠它算：每节开头放一个 <document-render-sec> 标记（值是重新开始的页码或 none，和页码格式），
        // 文末放 <document-render-end>。页码自己数，不用 counter(page)，这样首页判断、重排页码和
        // Word 的规则一致。
        o.push_str(FIELDS_FN);
        let mut docset = Vec::new();
        if let Some(t) = &doc.meta.title {
            docset.push(format!("title: {}", lit(t)));
        }
        if let Some(a) = &doc.meta.author {
            docset.push(format!("author: {}", lit(a)));
        }
        if !docset.is_empty() {
            let _ = writeln!(o, "#set document({})", docset.join(", "));
        }
        let lang = doc.meta.lang.as_deref().unwrap_or("zh");
        let _ = writeln!(
            o,
            "#set text(font: {}, lang: {}, top-edge: \"ascender\", bottom-edge: \"descender\")",
            lits(FONTS),
            lit(&lang_code(lang))
        );
        let _ = writeln!(o, "#set par(spacing: 0pt)");
        let _ = writeln!(o, "#set block(above: 0pt, below: 0pt)");
        let _ = writeln!(o, "#set figure(numbering: none)");
        // 标题的外观全由样式决定，heading 只留着生成书签
        let _ = writeln!(o, "#show heading: it => it.body");
        // 公式用 typst 自带的 New Computer Modern Math，\\text 里的中文往后找正文字体
        let mut mf = vec![MATH_FONT];
        mf.extend(FONTS);
        let _ = writeln!(o, "#show math.equation: set text(font: {})", lits(&mf));
    }

    /// 公式转成 Typst：行内 `$…$`，块级 `$display(…)$`。转不了就显示 LaTeX 原文。
    fn math_src(&mut self, latex: &str, block: bool) -> String {
        self.had_math = true;
        let raw =
            |latex: &str| format!("#text(font: {})[#{}]", lits(MONO_FONTS), lit(latex.trim()));
        if self.math_fallback {
            return raw(latex);
        }
        match crate::math::parse(latex, block) {
            Ok(root) => {
                let mut t = crate::math::Typst::new();
                let body = t.math(&root);
                self.warn
                    .math_partial(FORMAT, &self.cur_id, latex, &t.unsupported);
                if body.trim().is_empty() {
                    String::new()
                } else if block {
                    format!("$display({body})$")
                } else {
                    format!("${body}$")
                }
            }
            Err(e) => {
                let id = self.cur_id.clone();
                self.warn.math_failed(FORMAT, &id, latex, &e);
                raw(latex)
            }
        }
    }

    fn node(&mut self, n: &Node, parent: &Style) {
        let s = &n.style;
        self.cur_id = n.id.clone();
        match &n.component {
            Component::Document(_) => {
                let body = self.children_src(n);
                let src = self.scope_src(body, s, parent, &n.id);
                self.out.push_str(&src);
            }
            Component::Slide(p) => {
                self.out.push_str("#pagebreak(weak: true)\n");
                let mut body = String::new();
                if let Some(t) = &p.title {
                    let mut ts = s.clone();
                    ts.merge(&n.part("title"));
                    let inner = format!("#{}", lit(t));
                    body.push_str(&self.block_src(
                        format!("#heading(level: 2)[{inner}]"),
                        &ts,
                        s,
                        &n.id,
                    ));
                }
                let mut bs = s.clone();
                bs.merge(&n.part("body"));
                let kids = {
                    let saved = std::mem::take(&mut self.out);
                    self.kids(&n.children, &bs);
                    std::mem::replace(&mut self.out, saved)
                };
                body.push_str(&self.scope_src(kids, &bs, s, &n.id));
                let src = self.scope_src(body, s, parent, &n.id);
                self.out.push_str(&src);
                self.out.push_str("#pagebreak(weak: true)\n");
            }
            Component::Heading(p) => {
                let body = self.inline(&p.text, s);
                // typst 的 heading 自带 1.4em / 1.2em 字号和粗体，而且盖过用户的 show-set，
                // 只能在 show 函数里按样式把字号、粗细再设一遍（不放进标题内容，目录里的条目才不受影响）
                let weight = if s.bold == Some(true) {
                    "bold"
                } else {
                    "regular"
                };
                let size = s.size_pt();
                // 编号：typst 的 heading numbering 写成返回算好编号的函数（不用 typst 自己的计数），
                // 编号后面带上分隔的空格。正文由下面的 show 规则排；目录（outline）用 prefix 显示它，
                // 和 Word 更新后的目录一样是「编号 标题」
                let (numbering, show) = match &p.number {
                    None => (
                        String::new(),
                        format!("text(size: {size}pt, weight: \"{weight}\", it.body)"),
                    ),
                    Some(no) => {
                        let hn_own = self.heading_numbering.clone();
                        let hn = hn_own.as_ref();
                        let ms = hn
                            .and_then(|h| h.level(p.level))
                            .cloned()
                            .unwrap_or_default();
                        let fs = s.size_pt();
                        let mut mt = Style::default();
                        if let Some(c) = &ms.list_marker_color {
                            mt.color = Some(c.clone());
                        }
                        if let Some(f) = &ms.list_marker_font {
                            mt.font_family = Some(crate::style::FontFamily {
                                latin: Some(f.clone()),
                                east_asia: Some(f.clone()),
                                complex: None,
                            });
                        }
                        if let Some(pct) = ms.list_marker_size {
                            mt.font_size = Some(Len::Pt(fs * pct / 100.0));
                        }
                        let margs = self.text_args(&mt, &Style::default(), &n.id);
                        let mut m = format!("#{}", lit(no));
                        if !margs.is_empty() {
                            m = format!("#text({margs})[{m}]");
                        }
                        let sep = common::heading_sep(hn, p.level);
                        let numbering = format!(", numbering: (..n) => [{m}#{}]", lit(sep));
                        let show = match hn.map(|h| h.gap(p.level)) {
                            // 制表符：编号占 listHanging 宽的一列（编号更宽时这列跟着变宽，像 Word 跳到下一个制表位），
                            // 标题折行时和第一行文字对齐
                            Some(MarkerGap::Tab | MarkerGap::Len(_)) => {
                                let h = hn.map_or(fs * 3.0, |x| x.hanging_pt(p.level, fs));
                                format!(
                                    "text(size: {size}pt, weight: \"{weight}\")[#context {{ let m = (it.numbering)(); grid(columns: (calc.max({h}pt, measure(m).width), 1fr), m, it.body) }}]"
                                )
                            }
                            _ => format!(
                                "text(size: {size}pt, weight: \"{weight}\")[#(it.numbering)()#it.body]"
                            ),
                        };
                        (numbering, show)
                    }
                };
                self.block(
                    format!(
                        "#show heading: it => {show}\n#heading(level: {}{numbering})[{body}]",
                        p.level
                    ),
                    s,
                    parent,
                    &n.id,
                );
            }
            Component::Text(p) => {
                let mut body = self.inline(&p.text, s);
                // 空段落也占一行（和 Word 一样），放一个零宽空格撑起行盒
                if body.trim().is_empty() {
                    body = "#sym.zws".into();
                }
                self.block(format!("#par[{body}]"), s, parent, &n.id);
            }
            Component::Quote(p) => {
                let body = self.inline(&p.text, s);
                self.block(format!("#par[{body}]"), s, parent, &n.id);
            }
            Component::Callout(p) => {
                let body = self.inline(&p.text, s);
                self.block(format!("#par[{body}]"), s, parent, &n.id);
            }
            // 空盒子，线是它的下边框
            Component::Divider(_) => self.block(String::new(), s, parent, &n.id),
            Component::TableOfContents(p) => self.toc(n, p, parent),
            Component::Code(p) => self.code(n, p, parent),
            Component::Math(p) => {
                let eq = self.math_src(&p.latex, true);
                let body = match &p.number {
                    Some(no) => format!(
                        "#grid(columns: (1fr, auto, 1fr), align: (horizon, horizon, right + horizon), [], [{eq}], [#{}])",
                        lit(&format!("({no})"))
                    ),
                    None => format!("#par[{eq}]"),
                };
                self.block(body, s, parent, &n.id);
            }
            Component::List(p) => self.list(n, p, parent),
            Component::Table(p) => self.table(n, p, parent),
            Component::Image(p) => self.image(n, p, parent),
            Component::PageBreak(_) => {
                self.out.push_str("#pagebreak()\n");
                self.out.push_str(TOP_ANCHOR);
            }
            // 不分栏时 typst 的 colbreak 等于分页，跟 Word 一样
            Component::ColumnBreak(_) => self.out.push_str("#colbreak()\n"),
            Component::Section(p) => self.section(n, p.layout(), parent),
        }
    }

    /// 一节：换页面设置（typst 会从新的一页开始），放节标记，再排内容。
    fn section(&mut self, n: &Node, lay: &SectionLayout, parent: &Style) {
        let page = &lay.page;
        let mut ws = Vec::new();
        let (w, h) = page.dims_pt(&mut ws);
        for x in ws {
            self.warn.push(x);
        }
        let [mt, mr, mb, ml] = page.margins_pt();
        let gutter = page.gutter.map(|g| g.abs_pt()).unwrap_or(0.0);
        self.margins = [mt, mr, mb, ml + gutter];
        let cols = page.columns.as_ref().map_or(1, |c| c.count());
        let gap = page
            .columns
            .as_ref()
            .filter(|c| c.count() > 1)
            .map_or(0.0, |c| c.gap_pt());
        self.content_width = ((w - ml - mr - gutter) - gap * (cols as f32 - 1.0)) / cols as f32;
        let (hd, fd) = page.hf_distance_pt();
        let header = self.hf_src(&lay.header, lay, parent, true, hd);
        let footer = self.hf_src(&lay.footer, lay, parent, false, fd);
        let mut o = String::new();
        if lay.index > 0 {
            o.push_str("#pagebreak(weak: true)\n");
        }
        let _ = writeln!(
            o,
            "#set page(width: {w}pt, height: {h}pt, margin: (top: {mt}pt, right: {mr}pt, bottom: {mb}pt, left: {}pt), columns: {cols}, header: {header}, footer: {footer}, header-ascent: 0pt, footer-descent: 0pt)",
            ml + gutter
        );
        if let Some(c) = page.columns.as_ref().filter(|c| c.count() > 1) {
            let _ = writeln!(o, "#set columns(gutter: {}pt)", c.gap_pt());
        }
        let start = lay
            .page_number_start
            .map_or("none".to_string(), |x| x.to_string());
        let fmt = typst_numbering(page.page_number_format.unwrap_or_default());
        let _ = writeln!(
            o,
            "#metadata((start: {start}, fmt: {})) <document-render-sec>",
            lit(fmt)
        );
        o.push_str(TOP_ANCHOR);
        self.out.push_str(&o);
        let body = self.children_src(n);
        let src = self.scope_src(body, &n.style, parent, &n.id);
        self.out.push_str(&src);
    }

    /// 页眉或页脚的 typst 表达式：按本节首页、偶数页、其他页挑内容。
    /// 页眉顶端离纸边 `dist`，页脚底端离纸边 `dist`，跟 Word 一样。
    fn hf_src(
        &mut self,
        set: &HfNodes,
        lay: &SectionLayout,
        parent: &Style,
        top: bool,
        dist: f32,
    ) -> String {
        if set.is_empty() {
            return "none".into();
        }
        let one = |m: &mut Self, n: Option<&Node>| -> String {
            let Some(n) = n else {
                return "none".into();
            };
            let saved = std::mem::take(&mut m.out);
            m.node(n, parent);
            let body = std::mem::replace(&mut m.out, saved);
            let (inset, align) = if top {
                (format!("(top: {dist}pt)"), "top")
            } else {
                (format!("(bottom: {dist}pt)"), "bottom")
            };
            format!("block(width: 100%, height: 100%, inset: {inset}, align({align}, [\n{body}]))")
        };
        let default = one(self, set.default.as_ref());
        let mut src = String::from("context { let i = document-render-info(); ");
        if lay.title_page {
            let f = one(self, set.for_first());
            let _ = write!(src, "if i.first {{ {f} }} else ");
        }
        if lay.even_odd {
            let e = one(self, set.for_even());
            let _ = write!(src, "if calc.even(i.page) {{ {e} }} else ");
        }
        let _ = write!(src, "{{ {default} }} }}");
        src
    }

    fn children_src(&mut self, n: &Node) -> String {
        let saved = std::mem::take(&mut self.out);
        self.kids(&n.children, &n.style);
        std::mem::replace(&mut self.out, saved)
    }

    /// 排一串兄弟元素；边框设置相同的相邻段落像 Word 那样合成一个框（见 common::pbdr_groups）。
    fn kids(&mut self, kids: &[Node], parent: &Style) {
        let groups = common::pbdr_groups(kids);
        let mut i = 0;
        while i < kids.len() {
            match groups.iter().find(|g| g.start == i) {
                Some(g) => {
                    self.pbdr_group(&kids[g.clone()], parent);
                    i = g.end;
                }
                None => {
                    self.node(&kids[i], parent);
                    i += 1;
                }
            }
        }
    }

    /// 一组合框段落：外面一个只画边框的块，里面各段去掉边框、段前段后，按 Word 的规则留间距，
    /// 段与段之间画上面那段的 borderBetween。
    fn pbdr_group(&mut self, kids: &[Node], parent: &Style) {
        let g = common::pbdr_group(kids);
        let saved = std::mem::take(&mut self.out);
        for (i, (k, ms)) in kids.iter().zip(&g.members).enumerate() {
            let mut m = k.clone();
            m.style = ms.clone();
            self.node(&m, parent);
            if let Some(Some(b)) = g.lines.get(i) {
                self.out.push_str(&between_line(b));
            }
        }
        let body = std::mem::replace(&mut self.out, saved);
        let src = self.block_src(body, &g.outer, parent, &kids[0].id);
        self.out.push_str(&src);
    }

    fn block(&mut self, body: String, s: &Style, parent: &Style, id: &str) {
        let src = self.block_src(body, s, parent, id);
        self.out.push_str(&src);
    }

    /// 容器（Document、Slide）：里面可能有分页，不能包进 block。
    /// 没有盒子样式时只用一个内容块限定 set 规则的范围。
    fn scope_src(&mut self, body: String, s: &Style, parent: &Style, id: &str) -> String {
        if s.has_box() || s.width.is_some() || s.height.is_some() {
            return self.block_src(body, s, parent, id);
        }
        let src = self.block_src(String::new(), s, parent, id);
        // block_src 的形状是 `#block(...)[\n{sets}\n]`，取出 sets
        let sets = src
            .split_once("[\n")
            .and_then(|(_, rest)| rest.rsplit_once("\n]"))
            .map(|(x, _)| x.to_string())
            .unwrap_or_default();
        let mut out = String::new();
        if s.page_break_before == Some(true) {
            out.push_str("#pagebreak(weak: true)\n");
        }
        let _ = writeln!(out, "#[\n{sets}\n{body}]");
        out
    }

    /// 把一段内容包上段落和盒子样式。
    fn block_src(&mut self, body: String, s: &Style, parent: &Style, id: &str) -> String {
        let fs = s.size_pt();
        let mut sets = String::new();
        let text = self.text_args(s, parent, id);
        if !text.is_empty() {
            let _ = writeln!(sets, "#set text({text})");
        }
        let mut par = Vec::new();
        macro_rules! changed {
            ($f:ident) => {
                s.$f.is_some() && parent.$f != s.$f
            };
        }
        if s.line_height.is_some() || changed!(font_family) || changed!(font_size) {
            // 行盒按 Windows Word 的规则算（字体度量、混排、行距规则都在 metrics 里），
            // 行间距交给行盒，leading 归零。行盒跟字体和这段有没有中文有关，
            // 所以每个块都写一次，不只在 lineHeight 变了的时候
            let marker = self.marker_font.clone();
            let marker = marker.as_ref().map(|(f, k)| (f.as_str(), *k));
            let cjk = metrics::has_cjk(&body);
            let _ = writeln!(
                sets,
                "#set text({})",
                line_edges(s, s.line_height, fs, cjk, marker)
            );
            par.push("leading: 0pt".into());
        }
        if changed!(align) {
            match s.align {
                Some(Align::Justify | Align::Distribute) => par.push("justify: true".into()),
                Some(_) => par.push("justify: false".into()),
                None => {}
            }
            if s.align == Some(Align::Distribute) {
                self.warn
                    .unsupported(FORMAT, "align: distribute（按 justify 处理）", id);
            }
        }
        if changed!(text_indent) || changed!(hanging_indent) || changed!(font_size) {
            if let Some(h) = s.hanging_indent {
                par.push(format!(
                    "hanging-indent: {}pt, first-line-indent: 0pt",
                    h.pt(fs, 0.0)
                ));
            } else if let Some(t) = s.text_indent {
                par.push(format!(
                    "first-line-indent: (amount: {}pt, all: true)",
                    t.pt(fs, 0.0)
                ));
            }
        }
        if !par.is_empty() {
            let _ = writeln!(sets, "#set par({})", par.join(", "));
        }
        if changed!(align)
            && let Some(a) = s.align
        {
            let a = match a {
                Align::Center => "center",
                Align::Right => "right",
                _ => "left",
            };
            let _ = writeln!(sets, "#set align({a})");
        }
        if changed!(widow_control)
            && let Some(w) = s.widow_control
        {
            let c = if w { "100%" } else { "0%" };
            let _ = writeln!(sets, "#set text(costs: (widow: {c}, orphan: {c}))");
        }
        if changed!(auto_space)
            && let Some(a) = s.auto_space
        {
            let _ = writeln!(
                sets,
                "#set text(cjk-latin-spacing: {})",
                if a { "auto" } else { "none" }
            );
        }
        for (f, on) in [
            ("borderDiagonal", s.border_diagonal.is_some()),
            ("contextualSpacing", s.contextual_spacing == Some(true)),
            ("wrap: false", s.wrap == Some(Wrap::Text(false))),
            ("emphasisMark", s.emphasis_mark.is_some()),
            ("scale", s.scale.is_some()),
        ] {
            if on {
                self.warn.unsupported(FORMAT, f, id);
            }
        }

        let mut body = body;
        if let Some(r) = s.text_rotation.filter(|r| *r == 90 || *r == 270) {
            let deg = if r == 90 { 90 } else { -90 };
            body = format!("#rotate({deg}deg, reflow: true)[{body}]");
        }
        let mut args = Vec::new();
        if let Some(bg) = s.bg() {
            args.push(format!("fill: {}", bg_src(bg)));
        }
        let strokes = strokes(s);
        if !strokes.is_empty() {
            args.push(format!("stroke: ({})", strokes.join(", ")));
        }
        let pads: Vec<String> = Edge::ALL
            .iter()
            .zip(["top", "right", "bottom", "left"])
            .filter_map(|(e, k)| s.padding_side(*e).map(|p| format!("{k}: {p}pt")))
            .collect();
        if !pads.is_empty() {
            args.push(format!("inset: ({})", pads.join(", ")));
        }
        if let Some(r) = s.radius {
            args.push(format!("radius: {}pt", r.pt(fs, 0.0)));
        }
        if let Some(b) = s.space_before {
            args.push(format!("above: {}pt", b.pt(fs, 0.0)));
        }
        if let Some(a) = s.space_after {
            args.push(format!("below: {}pt", a.pt(fs, 0.0)));
        }
        if s.keep_together == Some(true) {
            args.push("breakable: false".into());
        }
        if s.keep_with_next == Some(true) {
            args.push("sticky: true".into());
        }
        match s.width {
            Some(Len::Pct(p)) => args.push(format!("width: {p}%")),
            Some(w) => args.push(format!("width: {}pt", w.pt(fs, 0.0))),
            None => args.push("width: 100%".into()),
        }
        if let Some(h) = s.height {
            args.push(match h {
                Len::Pct(p) => format!("height: {p}%"),
                h => format!("height: {}pt", h.pt(fs, 0.0)),
            });
        }
        let mut src = String::new();
        if s.page_break_before == Some(true) {
            src.push_str("#pagebreak(weak: true)\n");
        }
        // 有图案边：内边距挪进网格中间那格，block 本身不要 inset
        let content = match stripe_frame(s, &sets, &body).or_else(|| tile_grid(s, &sets, &body)) {
            Some(grid) => {
                args.retain(|a| !a.starts_with("inset:"));
                grid
            }
            None => format!("{sets}{body}"),
        };
        let ml = s.margin_left.map(|x| x.pt(fs, 0.0)).unwrap_or(0.0);
        let mr = s.margin_right.map(|x| x.pt(fs, 0.0)).unwrap_or(0.0);
        if ml != 0.0 || mr != 0.0 {
            // 段前段后距要放在 pad 外层的块上，放在 pad 里面会被当成容器开头吞掉
            let (gap, rest): (Vec<String>, Vec<String>) = args
                .into_iter()
                .partition(|a| a.starts_with("above:") || a.starts_with("below:"));
            let inner = format!("#block({})[\n{content}\n]", rest.join(", "));
            let _ = writeln!(
                src,
                "#block({})[#pad(left: {ml}pt, right: {mr}pt)[{inner}]]",
                gap.join(", ")
            );
        } else {
            let inner = format!("#block({})[\n{content}\n]", args.join(", "));
            let _ = writeln!(src, "{inner}");
        }
        src
    }

    /// `set text(...)` 的参数：和父样式不同的字体、字号、颜色、粗斜体等。
    fn text_args(&mut self, s: &Style, parent: &Style, _id: &str) -> String {
        let mut a = Vec::new();
        if s.font_family.is_some() && s.font_family != parent.font_family {
            a.push(format!("font: {}", font_list(s, false)));
        }
        if s.font_size.is_some() && s.font_size != parent.font_size {
            a.push(format!("size: {}pt", s.size_pt()));
        }
        if let Some(c) = s.color.as_ref().filter(|_| s.color != parent.color) {
            a.push(format!("fill: {}", color(c)));
        }
        if s.bold.is_some() && s.bold != parent.bold {
            a.push(format!(
                "weight: {}",
                if s.bold == Some(true) {
                    "\"bold\""
                } else {
                    "\"regular\""
                }
            ));
        }
        if s.italic.is_some() && s.italic != parent.italic {
            a.push(format!(
                "style: {}",
                if s.italic == Some(true) {
                    "\"italic\""
                } else {
                    "\"normal\""
                }
            ));
        }
        if let Some(l) = s
            .letter_spacing
            .filter(|_| s.letter_spacing != parent.letter_spacing)
        {
            a.push(format!("tracking: {}pt", l.pt(s.size_pt(), 0.0)));
        }
        if let Some(k) = s.kerning.filter(|_| s.kerning != parent.kerning) {
            a.push(format!("kerning: {k}"));
        }
        if let Some(l) = s.lang.as_ref().filter(|_| s.lang != parent.lang) {
            a.push(format!("lang: {}", lit(&lang_code(l))));
        }
        if let Some(d) = s.direction.filter(|_| s.direction != parent.direction) {
            a.push(format!("dir: {}", d.as_str()));
        }
        a.join(", ")
    }

    fn inline(&mut self, t: &Inline, para: &Style) -> String {
        let mut out = String::new();
        for r in t.runs() {
            out.push_str(&self.run(r, para));
        }
        if t.ends_with_break() {
            out.push_str("#\"\u{200B}\"");
        }
        out
    }

    /// 文字换了样式环境（格子、列表项）：run 样式按新环境重算。
    fn inline_in(&mut self, t: &Inline, old: &Style, new: &Style) -> String {
        let mut out = String::new();
        for r in common::rebase_runs(t, old, new) {
            out.push_str(&self.run(&r, new));
        }
        if t.ends_with_break() {
            out.push_str("#\"\u{200B}\"");
        }
        out
    }

    fn run(&mut self, r: &Run, para: &Style) -> String {
        let s = &r.computed;
        if s.hidden == Some(true) {
            return String::new();
        }
        let mut piece = String::new();
        if let Some(src) = &r.image {
            let Some(path) = self.asset_path(&r.text, src) else {
                return String::new();
            };
            let mut args = vec![lit(&path)];
            // 行内图片的 text 是替代文字
            if !r.text.is_empty() {
                args.push(format!("alt: {}", lit(&r.text)));
            }
            if let Some(w) = r.width {
                args.push(format!("width: {w}pt"));
            }
            if let Some(h) = r.height {
                args.push(format!("height: {h}pt"));
            }
            return format!("#box(image({}))", args.join(", "));
        }
        if let Some(f) = r.field {
            let key = match f {
                // 跟 Word 一样，只有 PAGE 按节的页码格式显示，总页数和本节页数都是阿拉伯数字
                Field::Page => "page-text",
                Field::Pages => "total",
                Field::SectionPages => "section-pages",
            };
            let _ = write!(piece, "#context str(document-render-info().{key})");
        }
        if r.is_math() {
            piece = self.math_src(&r.text, false);
        }
        for (i, line) in r
            .text
            .split('\n')
            .enumerate()
            .filter(|_| r.field.is_none() && !r.is_math())
        {
            if i > 0 {
                piece.push_str("#linebreak()");
            }
            if !line.is_empty() {
                let _ = write!(piece, "#{}", lit(line));
            }
        }
        let args = self.text_args(s, para, "");
        if !args.is_empty() {
            piece = format!("#text({args})[{piece}]");
        }
        match s.caps {
            Some(Caps::Small) => piece = format!("#smallcaps[{piece}]"),
            Some(Caps::All) => piece = format!("#upper[{piece}]"),
            None => {}
        }
        // 上标下标跟 Word 一样用字体 OS/2 里的字号和偏移；有中日韩文字时看东亚字体。
        // Typst 的 auto 只看字体列表里第一个字体（西文），所以这里写死成 pt。
        if let Some(v @ (VerticalAlign::Super | VerticalAlign::Sub)) = s.vertical_align {
            let sup = v == VerticalAlign::Super;
            let (size, off) = metrics::script_metrics(s, metrics::has_cjk(&r.text), sup);
            let fs = s.size_pt();
            let shift = if sup { -off } else { off } * fs;
            piece = format!(
                "#{}(typographic: false, size: {}pt, baseline: {}pt)[{piece}]",
                if sup { "super" } else { "sub" },
                (size * fs * 1000.0).round() / 1000.0,
                (shift * 1000.0).round() / 1000.0
            );
        }
        if let Some(u) = s.underline.filter(|u| u.on()) {
            let mut st = Vec::new();
            let thick = match u.0 {
                UnderlineStyle::Thick => "1.2pt",
                _ => "0.6pt",
            };
            st.push(format!("thickness: {thick}"));
            if let Some(c) = &s.underline_color {
                st.push(format!("paint: {}", color(c)));
            }
            match u.0 {
                UnderlineStyle::Dotted => st.push("dash: \"dotted\"".into()),
                UnderlineStyle::Dashed => st.push("dash: \"dashed\"".into()),
                UnderlineStyle::Double | UnderlineStyle::Wavy => {
                    self.warn
                        .unsupported(FORMAT, "underline: double/wavy", &r.text)
                }
                _ => {}
            }
            piece = format!("#underline(stroke: ({}))[{piece}]", st.join(", "));
        }
        match s.strike {
            Some(Strike::Single) => piece = format!("#strike[{piece}]"),
            Some(Strike::Double) => {
                self.warn.unsupported(FORMAT, "strike: double", &r.text);
                piece = format!("#strike[{piece}]");
            }
            _ => {}
        }
        if let Some(h) = &s.highlight {
            piece = format!("#highlight(fill: {})[{piece}]", color(h));
        }
        if let Some(bg) = s.bg() {
            piece = format!(
                "#box(fill: {}, inset: (x: 2pt), outset: (y: 2pt), radius: 2pt)[#set text(top-edge: \"ascender\", bottom-edge: \"descender\");{piece}]",
                bg_src(bg)
            );
        }
        if let Some(h) = &r.href {
            piece = format!("#link({})[{piece}]", lit(h));
        }
        piece
    }

    /// 目录：typst 的 outline，页码由 typst 排完算出。
    fn toc(&mut self, n: &Node, p: &TableOfContentsProps, parent: &Style) {
        let s = &n.style;
        let mut body = String::new();
        if let Some(t) = &p.title {
            let ts = common::toc_title_style(n);
            let src = self.block_src(format!("#par[#{}]", lit(t)), &ts, s, &n.id);
            body.push_str(&src);
        }
        let e1 = common::toc_entry_style(n, 1);
        let e2 = common::toc_entry_style(n, 2);
        let fs = e1.size_pt();
        let ml = |x: &Style| x.margin_left.map_or(0.0, |l| l.pt(fs, 0.0));
        let base = ml(&e1);
        let step = ml(&e2) - base;
        let above = e1.space_before.map_or(0.0, |l| l.pt(fs, 0.0));
        let below = e1.space_after.map_or(0.0, |l| l.pt(fs, 0.0));
        let mut es = e1.clone();
        es.margin_left = None;
        es.space_before = None;
        es.space_after = None;
        let ol = format!(
            "#show outline.entry: it => block(above: {above}pt, below: {below}pt, inset: (left: {base}pt + (it.level - 1) * {step}pt), link(it.element.location(), [#if it.prefix() != none {{ it.prefix() }}#it.body()#box(width: 1fr, repeat(gap: 0.15em)[.])#context document-render-info(loc: it.element.location()).page-text]))\n#outline(title: none, depth: {}, indent: 0pt)",
            p.depth()
        );
        let src = self.block_src(ol, &es, s, &n.id);
        body.push_str(&src);
        self.block(body, s, parent, &n.id);
    }

    fn code(&mut self, n: &Node, p: &CodeProps, parent: &Style) {
        let s = &n.style;
        let mut body = format!(
            "#show raw: set text(font: {}, size: {}pt)\n",
            font_list(s, true),
            s.size_pt()
        );
        if p.line_numbers {
            body.push_str(
                "#show raw.line: it => [#box(width: 2.5em)[#text(fill: rgb(\"#999999\"))[#it.number]]#it.body]\n",
            );
        }
        let _ = write!(body, "#raw({}, block: true)", lit(&p.code));
        // 字体已经给 raw 了，外层不再设
        let mut outer = s.clone();
        outer.font_family = parent.font_family.clone();
        outer.font_size = parent.font_size;
        self.block(body, &outer, parent, &n.id);
    }

    fn list(&mut self, n: &Node, p: &ListProps, parent: &Style) {
        let lines = common::flatten_list(n, p);
        let gaps = common::item_gaps(&lines);
        let marker_part = n.part("marker");
        let item_part = n.part("item");
        let mut body = String::new();
        for (i, l) in lines.iter().enumerate() {
            let s = &l.style;
            let fs = s.size_pt();
            let indent = s.list_indent.map(|x| x.pt(fs, 0.0)).unwrap_or(18.0) * l.level as f32;
            let hanging = s.list_hanging.map(|x| x.pt(fs, 0.0)).unwrap_or(14.0);
            let mut item = s.clone();
            item.merge(&item_part);
            let mut ms = item.text_only();
            ms.background = None;
            if let Some(c) = &s.list_marker_color {
                ms.color = Some(c.clone());
            }
            if let Some(f) = &s.list_marker_font {
                ms.font_family = Some(crate::style::FontFamily {
                    latin: Some(f.clone()),
                    east_asia: Some(f.clone()),
                    complex: None,
                });
            }
            if let Some(pct) = s.list_marker_size {
                ms.font_size = Some(Len::Pt(fs * pct / 100.0));
            }
            ms.merge(&marker_part);
            let margs = self.text_args(&ms, &item.text_only(), &n.id);
            let mut marker = match &s.list_marker_image {
                Some(src) => match self.asset_path(&n.id, src) {
                    Some(path) => format!("#box(image({}, height: 0.8em))", lit(&path)),
                    None => format!("#{}", lit("•")),
                },
                None => format!("#{}", lit(&l.marker)),
            };
            if !margs.is_empty() {
                marker = format!("#text({margs})[{marker}]");
            }
            let gap = match s.list_marker_gap {
                Some(MarkerGap::Len(g)) => g.pt(fs, 0.0),
                Some(MarkerGap::Space) => fs * 0.3,
                Some(MarkerGap::None) => 0.0,
                _ => fs * 0.5,
            };
            // 编号在编号框里的位置；右对齐时空隙留在右边
            let fill = match s.list_marker_align {
                Some(HAlign::Center) => "[#h(1fr)#m#h(1fr)]".to_string(),
                Some(HAlign::Right) => format!("[#h(1fr)#m#h({gap}pt)]"),
                _ => "[#m#h(1fr)]".to_string(),
            };
            let text = self.inline_in(l.item.text(), &n.style, &item);
            // 编号比悬挂缩进宽时（如「十二、」），编号框跟着变宽，像 Word 跳到下一个制表位
            let line = format!(
                "#set par(hanging-indent: {hanging}pt, first-line-indent: 0pt)\n#par[#context {{ let m = [{marker}]; box(width: calc.max({hanging}pt, measure([#m#h({gap}pt)#h(0pt)]).width + 1pt), {fill}) }}{text}]"
            );
            let mut ls = item.clone();
            ls.margin_left = Some(Len::Pt(indent - hanging));
            ls.text_indent = None;
            ls.hanging_indent = None;
            ls.space_before = None;
            ls.space_after = None;
            // 项与项之间：写了 itemSpacing 就是它；没写时和 Word 一样，
            // 上一项的段后和这一项的段前取大的（item 部位）
            if i > 0 {
                ls.space_before = match gaps[i - 1] {
                    Some(g) => Some(g),
                    None => {
                        let pt = |x: Option<Len>| x.map_or(0.0, |x| x.pt(fs, 0.0));
                        let g = pt(item_part.space_after).max(pt(item_part.space_before));
                        (g > 0.0).then_some(Len::Pt(g))
                    }
                };
            }
            ls.background = None;
            ls.border = None;
            ls.padding = None;
            self.marker_font = s
                .list_marker_font
                .clone()
                .map(|f| (f, s.list_marker_size.map_or(1.0, |p| p / 100.0)));
            body.push_str(&self.block_src(line, &ls, &n.style, &n.id));
            self.marker_font = None;
            // 这一项的续块：左边对齐到项的文字
            for b in l.item.blocks() {
                let b = common::indented_block(b, indent);
                let saved = std::mem::take(&mut self.out);
                self.node(&b, &n.style);
                let got = std::mem::replace(&mut self.out, saved);
                body.push_str(&got);
            }
        }
        self.block(body, &n.style, parent, &n.id);
    }

    fn table(&mut self, n: &Node, p: &TableProps, parent: &Style) {
        let g = Grid::of(p);
        let s = &n.style;
        let pad = s
            .cell_padding
            .map(|x| x.pt(s.size_pt(), 0.0))
            .unwrap_or(4.0);
        let ws = common::col_widths(p, &g, &mut self.warn, &n.id);
        let indent = s.margin_left.map_or(0.0, |x| x.pt(s.size_pt(), 0.0));
        let avail = match s.width {
            Some(Len::Pct(x)) => (self.content_width - indent) * x / 100.0,
            Some(w) => w.abs_pt(),
            None => self.content_width - indent,
        };
        let nat = common::auto_natural(n, p, &g, &ws, pad);
        let widths: Vec<String> = common::resolve_auto(&ws, &nat, avail)
            .iter()
            .map(|w| match w {
                ColWidth::Ratio(x) => format!("{x}fr"),
                ColWidth::Fixed(x) => format!("{x}pt"),
                ColWidth::Auto => "auto".to_string(),
            })
            .collect();
        let mut header = Vec::new();
        let mut body = Vec::new();
        g.warn_ragged(&mut self.warn, &n.id);
        let mut exact_rows = Vec::new();
        // Word 把横线的粗细算进行高：每行加它上面那条线（行间的线要扣掉下一行的上边距，见下面），
        // 最后一行再加下面那条。Typst 的线压在格子边上、不占高度，所以加进格子的上下内边距
        let width =
            |b: &Option<Border>| b.as_ref().filter(|b| !b.is_none()).map_or(0.0, |b| b.width);
        let mut above = vec![0.0f32; g.rows + 1];
        for r in 0..g.rows {
            for c in 0..g.cols {
                if !g.is_origin(r, c) {
                    continue;
                }
                let sl = g.slot(r, c);
                let e = common::span_edges(n, &p.look, &g, r, c);
                above[r] = above[r].max(width(&e[0]));
                let r1 = r + sl.row_span;
                above[r1] = above[r1].max(width(&e[2]));
            }
        }
        for r in 0..g.rows {
            let rh = g.row_height(p, r);
            exact_rows.push(match rh {
                Some(h) if h.exact => format!("{}pt", h.height),
                _ => "auto".to_string(),
            });
            for c in 0..g.cols {
                if !g.is_origin(r, c) {
                    continue;
                }
                let sl = *g.slot(r, c);
                let part = common::cell_parts(n, &p.look, &g, r, c);
                let mut cs = s.inherited();
                cs.merge(&part);
                let mut text = self.inline_in(sl.text(), s, &cs);
                let mut sets = self.text_args(&cs, s, &n.id);
                if !sets.is_empty() {
                    sets = format!("#set text({sets})\n");
                }
                // 格子自己的行高（比如表头的 lineHeight）
                {
                    let cjk = metrics::has_cjk(&text);
                    let _ = writeln!(
                        sets,
                        "#set text({})",
                        line_edges(&cs, cs.line_height, cs.size_pt(), cjk, None)
                    );
                }
                if let Some(a) = cs.align.filter(|a| s.align != Some(*a)) {
                    let a = match a {
                        Align::Center => "center",
                        Align::Right => "right",
                        _ => "left",
                    };
                    let _ = writeln!(sets, "#set align({a})");
                }
                let mut args = Vec::new();
                if let Some(bg) = part.bg() {
                    args.push(format!("fill: {}", bg_src(bg)));
                }
                let b = common::span_edges(n, &p.look, &g, r, c);
                if sl.row_span > 1 {
                    args.push(format!("rowspan: {}", sl.row_span));
                }
                if sl.col_span > 1 {
                    args.push(format!("colspan: {}", sl.col_span));
                }
                let st: Vec<String> = ["top", "right", "bottom", "left"]
                    .iter()
                    .zip(b.iter())
                    .zip(Edge::ALL)
                    .map(|((k, b), e)| format!("{k}: {}", cell_stroke(b.as_ref(), e)))
                    .collect();
                args.push(format!("stroke: ({})", st.join(", ")));
                let last = r + sl.row_span >= g.rows;
                // docx 里两行之间的线写在上面一格的下边，Word 让它先占下面一格的上边距：
                // 只有线比上边距粗的部分才加进行高（第一行的上框线、最后一行的下框线照加）
                let top_pad = part.padding_side(Edge::Top).unwrap_or(pad);
                let line = |e: &Edge| match e {
                    Edge::Top if r == 0 => above[0],
                    Edge::Top => (above[r] - top_pad).max(0.0),
                    Edge::Bottom if last => above[g.rows],
                    _ => 0.0,
                };
                let pads: Vec<String> = Edge::ALL
                    .iter()
                    .zip(["top", "right", "bottom", "left"])
                    .map(|(e, k)| {
                        format!("{k}: {}pt", part.padding_side(*e).unwrap_or(pad) + line(e))
                    })
                    .collect();
                args.push(format!("inset: ({})", pads.join(", ")));
                if let Some(v) = part.vertical_align {
                    args.push(format!(
                        "align: {}",
                        match v {
                            VerticalAlign::Middle => "horizon",
                            VerticalAlign::Bottom => "bottom",
                            _ => "top",
                        }
                    ));
                }
                let ins: Vec<f32> = Edge::ALL
                    .iter()
                    .map(|e| part.padding_side(*e).unwrap_or(pad) + line(e))
                    .collect();
                // 最小行高：Typst 的行只有固定高度，最小高度靠把内容撑到那么高（只看单行的格子）
                if let Some(h) = rh.filter(|h| !h.exact && sl.row_span == 1) {
                    let min = (h.height - ins[0] - ins[2]).max(0.0);
                    let va = match part.vertical_align {
                        Some(VerticalAlign::Middle) => "horizon",
                        Some(VerticalAlign::Bottom) => "bottom",
                        _ => "top",
                    };
                    text = format!(
                        "#layout(sz => {{ let c = [{sets}{text}]; let h = measure(block(width: sz.width, c)).height; block(height: calc.max(h, {min}pt), width: 100%, align({va}, c)) }})"
                    );
                    sets = String::new();
                }
                let cell = format!("table.cell({})[{sets}{text}]", args.join(", "));
                if g.has_header && r == 0 {
                    header.push(cell);
                } else {
                    body.push(cell);
                }
            }
        }
        if s.row_break == Some(false) {
            self.warn.unsupported(FORMAT, "rowBreak", &n.id);
        }
        let mut all = Vec::new();
        if !header.is_empty() {
            all.push(format!(
                "table.header(repeat: {}, {})",
                s.repeat_header.unwrap_or(true),
                header.join(", ")
            ));
        }
        all.extend(body);
        let src = format!(
            "#table(columns: ({},), rows: ({},), stroke: none, inset: 0pt, {})",
            widths.join(", "),
            exact_rows.join(", "),
            all.join(", ")
        );
        // 边框和背景已经落到格子上
        let mut outer = s.clone();
        outer.border = None;
        outer.border_top = None;
        outer.border_right = None;
        outer.border_bottom = None;
        outer.border_left = None;
        outer.border_inside_h = None;
        outer.border_inside_v = None;
        outer.padding = None;
        self.block(src, &outer, parent, &n.id);
    }

    /// 本地图片放进虚拟文件，返回路径；失败时给警告。
    fn asset_path(&mut self, id: &str, src: &str) -> Option<String> {
        let loaded = if assets::is_remote(src) {
            Err("pdf 暂不支持远程图片".to_string())
        } else {
            assets::load(src, self.opts.base_dir.as_deref()).map_err(|e| format!("{e:#}"))
        };
        match loaded {
            Ok(a) => {
                let ext = match a.mime {
                    "image/png" => "png",
                    "image/jpeg" => "jpg",
                    "image/gif" => "gif",
                    "image/svg+xml" => "svg",
                    "image/webp" => "webp",
                    _ => "bin",
                };
                let path = format!("/assets/{}.{ext}", self.files.len());
                self.files.push((path.clone(), a.bytes));
                Some(path)
            }
            Err(why) => {
                self.warn
                    .push(format!("图片 `{id}`：{why}，改成显示 alt 文字"));
                None
            }
        }
    }

    fn image(&mut self, n: &Node, p: &ImageProps, parent: &Style) {
        let s = &n.style;
        let Some(path) = self.asset_path(&n.id, &p.src) else {
            let alt = if p.alt.is_empty() { &p.src } else { &p.alt };
            self.block(
                format!("#emph[#{}]", lit(&format!("[{alt}]"))),
                s,
                parent,
                &n.id,
            );
            return;
        };
        let mut args = vec![lit(&path)];
        let fs = s.size_pt();
        match (s.width, p.width) {
            (Some(Len::Pct(x)), _) => args.push(format!("width: {x}%")),
            (Some(w), _) => args.push(format!("width: {}pt", w.pt(fs, 0.0))),
            (None, Some(w)) => args.push(format!("width: {w}pt")),
            _ => {}
        }
        match (s.height, p.height) {
            (Some(Len::Pct(x)), _) => args.push(format!("height: {x}%")),
            (Some(h), _) => args.push(format!("height: {}pt", h.pt(fs, 0.0))),
            (None, Some(h)) => args.push(format!("height: {h}pt")),
            _ => {}
        }
        if let Some(f) = s.object_fit {
            args.push(format!(
                "fit: \"{}\"",
                match f {
                    crate::style::ObjectFit::Fill => "stretch",
                    f => f.as_str(),
                }
            ));
        }
        if !p.alt.is_empty() {
            args.push(format!("alt: {}", lit(&p.alt)));
        }
        let mut img = format!("image({})", args.join(", "));
        if let Some(r) = s.rotation {
            img = format!("rotate({r}deg, reflow: true)[#{img}]");
        }
        if s.opacity.is_some() {
            self.warn.unsupported(FORMAT, "opacity", &n.id);
        }
        let mut fig = match &p.caption {
            Some(c) => {
                let mut cs = s.inherited();
                cs.merge(&n.part("caption"));
                let cap = format!("#{}", lit(c));
                let args = self.text_args(&cs, s, &n.id);
                let cap = if args.is_empty() {
                    cap
                } else {
                    format!("#text({args})[{cap}]")
                };
                format!("#figure({img}, caption: [{cap}], gap: 4pt)")
            }
            None => format!("#box({img})"),
        };
        let mut st = s.clone();
        st.width = None;
        st.height = None;
        match s.position.unwrap_or(Position::Inline) {
            Position::Inline => {
                if let Some(a) = s.horizontal_align {
                    fig = format!("#align({})[{fig}]", a.as_str());
                }
                self.block(fig, &st, parent, &n.id);
            }
            Position::Float => {
                // typst 不能让文字绕排图片，按左右对齐的独立块处理
                if !matches!(s.wrap, Some(Wrap::Image(WrapMode::TopAndBottom))) {
                    self.warn
                        .unsupported(FORMAT, "文字绕排（position: float）", &n.id);
                }
                let side = match s.float.unwrap_or(Side::Left) {
                    Side::Left => "left",
                    Side::Right => "right",
                };
                fig = format!("#align({side})[{fig}]");
                // 不绕排，wrapDistance 的上下两边当这个块的段前段后（没写 spaceBefore/After 时）
                if let Some(d) = s.wrap_distance {
                    let [t, _, b, _] = d.sides_pt();
                    st.space_before.get_or_insert(Len::Pt(t));
                    st.space_after.get_or_insert(Len::Pt(b));
                }
                self.block(fig, &st, parent, &n.id);
            }
            Position::Absolute => {
                let mut x = s.x.map(|v| v.abs_pt()).unwrap_or(0.0);
                let mut y = s.y.map(|v| v.abs_pt()).unwrap_or(0.0);
                if s.anchor_to == Some(AnchorTo::Page) {
                    x -= self.margins[3];
                    y -= self.margins[0];
                }
                let al = match (s.horizontal_align, s.vertical_align) {
                    (Some(h), _) => format!("top + {}", h.as_str()),
                    (_, Some(VerticalAlign::Bottom)) => "bottom + left".into(),
                    _ => "top + left".into(),
                };
                let _ = writeln!(self.out, "#place({al}, dx: {x}pt, dy: {y}pt)[{fig}]");
            }
        }
    }
}

/// 某一边的 typst stroke。要用图案画的线型（见 `common::border_tile`）这里不画：块交给 stripe_frame、tile_grid，
/// 表格格子交给 cell_stroke。
fn stroke(b: Option<&Border>, side: Edge) -> String {
    match b {
        None => "none".into(),
        Some(b) if b.is_none() => "none".into(),
        Some(b) if common::border_tile(b, side).is_some() => "none".into(),
        Some(b) => {
            let dash = match b.style {
                BorderStyle::Dashed => ", dash: \"dashed\"",
                BorderStyle::Dotted => ", dash: \"dotted\"",
                _ => "",
            };
            format!(
                "(paint: {}, thickness: {}pt{dash})",
                color(&b.color),
                b.width
            )
        }
    }
}

/// 合框段落之间的线（borderBetween）：占线宽那么高、和框里沿一样宽的一条。
/// 实线直接填色；双线、三线这类拆成几条实线；虚线、点线用 line；波浪线这类图案线退成实线。
fn between_line(b: &Border) -> String {
    let w = b.width;
    let open =
        format!("#block(width: 100%, height: {w}pt, above: 0pt, below: 0pt, breakable: false");
    match (common::border_tile(b, Edge::Bottom), b.style) {
        (Some(t), _) if !t.stripes.is_empty() => {
            let rects: String = t
                .stripes
                .iter()
                .map(|(off, th, c)| {
                    format!(
                        "#place(top + left, dy: {off:.3}pt, rect(width: 100%, height: {th:.3}pt, fill: rgb(\"{c}\"), stroke: none))"
                    )
                })
                .collect();
            format!("{open})[{rects}]\n")
        }
        (None, BorderStyle::Dashed | BorderStyle::Dotted) => format!(
            "{open})[#place(top + left, dy: {}pt, line(length: 100%, stroke: {}))]\n",
            w / 2.0,
            stroke(Some(b), Edge::Bottom)
        ),
        _ => format!("{open}, fill: {})\n", color(&b.color)),
    }
}

/// 一条线：(线中心离外沿的距离, 粗细, 颜色)。
type Line = (f32, f32, String);

/// 块的多线边框（双线、三线、粗细线、立体线）：每条线是一层 block 的 stroke，一层套一层往里画。
/// 线都是整条的矩形边，没有图案拼缝；拐角由 stroke 自己接上，跨页时每页各画一圈，和普通边框一样。
/// 线画在盒子里面、内边距外面，和 HTML 一致。有虚线、波浪线这类要铺图案的边时返回 None，交给 tile_grid。
fn stripe_frame(s: &Style, sets: &str, body: &str) -> Option<String> {
    // 每条边：(线宽, 各层的线：(线中心离外沿的距离, 粗细, 颜色))
    let mut sides: Vec<(f32, Vec<Line>)> = Vec::new();
    let mut any = false;
    for e in Edge::ALL {
        let tile = s.border_side(e).and_then(|b| common::border_tile(b, e));
        match tile {
            Some(t) if t.stripes.is_empty() => return None,
            Some(t) => {
                any = true;
                let lines = t
                    .stripes
                    .iter()
                    .map(|(off, th, c)| (off + th / 2.0, *th, c.clone()))
                    .collect();
                sides.push((t.width, lines));
            }
            None => sides.push((0.0, Vec::new())),
        }
    }
    if !any {
        return None;
    }
    let layers = sides.iter().map(|(_, l)| l.len()).max().unwrap_or(0);
    // 第 k 层 block 的边离外沿多远；这条边没有第 k 条线时就贴着线宽的里沿
    let pos = |i: usize, k: usize| -> f32 {
        let (w, l) = &sides[i];
        l.get(k).map(|x| x.0).unwrap_or(*w)
    };
    let fmt4 = |v: [f32; 4]| {
        format!(
            "(top: {:.3}pt, right: {:.3}pt, bottom: {:.3}pt, left: {:.3}pt)",
            v[0], v[1], v[2], v[3]
        )
    };
    let pads: [f32; 4] = std::array::from_fn(|i| s.padding_side(Edge::ALL[i]).unwrap_or(0.0));
    let mut open = String::new();
    // 最外一层只负责把第一条线挪到位
    let _ = write!(
        open,
        "#block(width: 100%, spacing: 0pt, inset: {})[",
        fmt4(std::array::from_fn(|i| pos(i, 0)))
    );
    for k in 0..layers {
        let strokes: Vec<String> = ["top", "right", "bottom", "left"]
            .iter()
            .enumerate()
            .map(|(i, key)| match sides[i].1.get(k) {
                Some((_, th, c)) => format!("{key}: (paint: rgb(\"{c}\"), thickness: {th:.3}pt)"),
                None => format!("{key}: none"),
            })
            .collect();
        let inset: [f32; 4] = std::array::from_fn(|i| {
            if k + 1 < layers {
                pos(i, k + 1) - pos(i, k)
            } else {
                sides[i].0 - pos(i, k) + pads[i]
            }
        });
        let _ = write!(
            open,
            "#block(width: 100%, spacing: 0pt, stroke: ({}), inset: {})[",
            strokes.join(", "),
            fmt4(inset)
        );
    }
    Some(format!("{open}\n{sets}{body}\n{}", "]".repeat(layers + 1)))
}

/// 块的图案边框（点划线、波浪线等）：用 3×3 网格把内容围起来，四边那几格用图案当 fill
/// （fill 跟着格子的大小走，跨页也能接上）。和 HTML 一样，线画在盒子里面、内边距外面；
/// 角上用横边的图案。没有图案边时返回 None。
fn tile_grid(s: &Style, sets: &str, body: &str) -> Option<String> {
    let mut fills: [Option<String>; 4] = Default::default();
    let mut w = [0.0f32; 4];
    for (i, e) in Edge::ALL.iter().enumerate() {
        let Some(b) = s.border_side(*e) else { continue };
        let Some(t) = common::border_tile(b, *e) else {
            continue;
        };
        w[i] = t.width;
        fills[i] = Some(tiling(&t, matches!(e, Edge::Left | Edge::Right)));
    }
    if fills.iter().all(Option::is_none) {
        return None;
    }
    let pads: Vec<String> = Edge::ALL
        .iter()
        .zip(["top", "right", "bottom", "left"])
        .map(|(e, k)| format!("{k}: {}pt", s.padding_side(*e).unwrap_or(0.0)))
        .collect();
    let cell = |f: &Option<String>, span: usize| match f {
        Some(f) => format!("grid.cell(colspan: {span}, fill: {f})[]"),
        None => format!("grid.cell(colspan: {span})[]"),
    };
    Some(format!(
        "{sets}#grid(columns: ({}pt, 1fr, {}pt), rows: ({}pt, auto, {}pt), {}, {}, block(width: 100%, inset: ({}))[{body}], {}, {})",
        w[3],
        w[1],
        w[0],
        w[2],
        cell(&fills[0], 3),
        cell(&fills[3], 1),
        pads.join(", "),
        cell(&fills[1], 1),
        cell(&fills[2], 3),
    ))
}

/// 一条边的 SVG 图案，当 fill 用。
fn tiling(t: &common::Tile, vertical: bool) -> String {
    let (tw, th) = if vertical {
        (t.width, t.period)
    } else {
        (t.period, t.width)
    };
    format!(
        "tiling(size: ({tw}pt, {th}pt), relative: \"self\", image(bytes({}), format: \"svg\", width: {tw}pt, height: {th}pt))",
        lit(&t.svg(vertical))
    )
}

/// 表格格子某一边的 stroke。格子边线的长短跟着行高走，所以图案线也写成 stroke 的 paint：
/// 不断的线（双线、粗细线、立体线）用硬边渐变画成几条，没有拼缝；虚线、波浪线用以线为中心的 SVG 图案铺。
/// 和 Typst 的 stroke 一样，线的中线压在格子边上。
fn cell_stroke(b: Option<&Border>, side: Edge) -> String {
    let Some(t) = b.and_then(|x| common::border_tile(x, side)) else {
        return stroke(b, side);
    };
    let vertical = matches!(side, Edge::Left | Edge::Right);
    let paint = if t.stripes.is_empty() {
        let (tw, th) = if vertical {
            (t.width, t.period)
        } else {
            (t.period, t.width)
        };
        format!(
            "tiling(size: ({tw}pt, {th}pt), relative: \"self\", image(bytes({}), format: \"svg\", width: {tw}pt, height: {th}pt))",
            lit(&t.svg_centered(vertical))
        )
    } else {
        stripe_gradient(&t, vertical)
    };
    format!("(paint: {paint}, thickness: {}pt)", t.width)
}

/// 几条平行线写成一个硬边渐变：从上（或左）往下（或右），线的地方是线的颜色，空隙透明。
fn stripe_gradient(t: &common::Tile, vertical: bool) -> String {
    let pct = |x: f32| format!("{:.3}%", (x / t.width * 100.0).clamp(0.0, 100.0));
    let mut stops = Vec::new();
    let mut y = 0.0;
    let clear = |c: &str| format!("rgb(\"{c}\").transparentize(100%)");
    for (off, th, c) in &t.stripes {
        if *off > y {
            stops.push(format!("({}, {})", clear(c), pct(y)));
            stops.push(format!("({}, {})", clear(c), pct(*off)));
        }
        stops.push(format!("(rgb(\"{c}\"), {})", pct(*off)));
        stops.push(format!("(rgb(\"{c}\"), {})", pct(off + th)));
        y = off + th;
    }
    if y < t.width
        && let Some((_, _, c)) = t.stripes.last()
    {
        stops.push(format!("({}, {})", clear(c), pct(y)));
        stops.push(format!("({}, 100%)", clear(c)));
    }
    let angle = if vertical { "0deg" } else { "90deg" };
    format!("gradient.linear({}, angle: {angle})", stops.join(", "))
}

fn strokes(s: &Style) -> Vec<String> {
    Edge::ALL
        .iter()
        .zip(["top", "right", "bottom", "left"])
        .filter_map(|(e, k)| {
            s.border_side(*e)
                .map(|b| format!("{k}: {}", stroke(Some(b), *e)))
        })
        .collect()
}

fn color(c: &Color) -> String {
    format!("rgb(\"{}\")", c.css())
}

fn bg_src(b: &Background) -> String {
    match b {
        Background::Gradient { angle, stops } => format!(
            "gradient.linear({}, angle: {}deg)",
            stops.iter().map(color).collect::<Vec<_>>().join(", "),
            angle - 90.0
        ),
        b => color(&b.solid()),
    }
}

fn lang_code(l: &str) -> String {
    l.split(['-', '_'])
        .next()
        .unwrap_or("zh")
        .to_ascii_lowercase()
}

/// Typst 字符串字面量。
/// 页码格式对应的 typst 编号写法；字母由 FIELDS_FN 里的 document-render-num 按 Word 的规则自己写。
fn typst_numbering(f: PageNumberFormat) -> &'static str {
    match f {
        PageNumberFormat::Decimal => "1",
        PageNumberFormat::LowerRoman => "i",
        PageNumberFormat::UpperRoman => "I",
        PageNumberFormat::LowerLetter => "a",
        PageNumberFormat::UpperLetter => "A",
        PageNumberFormat::ChineseCounting => "一",
    }
}

fn lit(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn lits(xs: &[&str]) -> String {
    let v: Vec<String> = xs.iter().map(|x| lit(x)).collect();
    format!("({},)", v.join(", "))
}

/// 样式里的字体（逗号分开的也拆开），后面接默认字体兜底。
fn font_list(s: &Style, mono: bool) -> String {
    let mut v: Vec<String> = Vec::new();
    if let Some(f) = &s.font_family {
        for x in f.fallback() {
            for y in x.split(',') {
                // 中文名（宋体）换成本机认的英文族名（SimSun）
                let y = crate::fonts::resolve(y).unwrap_or_else(|| y.trim().to_string());
                if !y.is_empty() && !v.contains(&y) {
                    v.push(y);
                }
            }
        }
    }
    if mono {
        v.extend(MONO_FONTS.iter().map(|x| x.to_string()));
    }
    v.extend(FONTS.iter().map(|x| x.to_string()));
    let refs: Vec<&str> = v.iter().map(String::as_str).collect();
    lits(&refs)
}

/// 字体只扫一次：typst 自带字体加系统字体。
pub(crate) fn font_store() -> &'static FontStore {
    static STORE: OnceLock<FontStore> = OnceLock::new();
    STORE.get_or_init(|| {
        let mut s = FontStore::new();
        s.extend(fonts::embedded());
        let mut sys: Vec<_> = fonts::system().collect();
        // 一族里只要有一个字重不是整百，整族都按名字归一，免得 Heavy 正好是 700 抢了 Bold
        let odd: std::collections::HashSet<String> = sys
            .iter()
            .filter(|(_, i)| i.variant.weight.to_number() % 100 != 0)
            .map(|(_, i)| i.family.clone())
            .collect();
        for (path, info) in sys.iter_mut() {
            if odd.contains(&info.family) {
                normalize_weight(info, &path.path, path.index);
            }
        }
        s.extend(sys);
        s
    })
}

/// 有的字体 OS/2 里的字重不是整百，比如 MiSans：Regular 330、Medium 380、Bold 630。
/// typst 按数值找最近的字重，要 400 会拿到 Medium，要 700 会拿到 Heavy。
/// 这种字体整族按各自的字重名（Regular、Bold……）改成标准数值；认不出的名字不动。
fn normalize_weight(info: &mut FontInfo, path: &std::path::Path, index: u32) {
    let Ok(data) = std::fs::read(path) else {
        return;
    };
    let Ok(face) = ttf_parser::Face::parse(&data, index) else {
        return;
    };
    let name = |id: u16| {
        face.names()
            .into_iter()
            .filter(|n| n.name_id == id && n.is_unicode())
            .find_map(|n| n.to_string())
    };
    let Some(sub) = name(ttf_parser::name_id::TYPOGRAPHIC_SUBFAMILY)
        .or_else(|| name(ttf_parser::name_id::SUBFAMILY))
    else {
        return;
    };
    let key: String = sub
        .to_lowercase()
        .replace("italic", "")
        .replace("oblique", "")
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .collect();
    let w = match key.as_str() {
        "thin" | "hairline" => 100,
        "extralight" | "ultralight" => 200,
        "light" => 300,
        "regular" | "book" | "" => 400,
        "medium" => 500,
        "semibold" | "demibold" => 600,
        "bold" => 700,
        "extrabold" | "ultrabold" => 800,
        "heavy" | "black" => 900,
        _ => return,
    };
    info.variant.weight = FontWeight::from_number(w);
}

fn library() -> &'static LazyHash<Library> {
    static LIB: OnceLock<LazyHash<Library>> = OnceLock::new();
    LIB.get_or_init(|| LazyHash::new(Library::default()))
}

struct DocumentRenderWorld {
    main: Source,
    files: HashMap<FileId, Bytes>,
}

fn file_id(path: &str) -> FileId {
    let vpath = VirtualPath::new(path).expect("内部生成的虚拟路径应当合法");
    RootedPath::new(VirtualRoot::Project, vpath).intern()
}

impl DocumentRenderWorld {
    fn new(source: String, files: Files) -> Self {
        let main = Source::new(file_id("/main.typ"), source);
        let files = files
            .into_iter()
            .map(|(p, b)| (file_id(&p), Bytes::new(b)))
            .collect();
        Self { main, files }
    }
}

impl World for DocumentRenderWorld {
    fn library(&self) -> &LazyHash<Library> {
        library()
    }

    fn book(&self) -> &LazyHash<FontBook> {
        font_store().book()
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::AccessDenied)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.files.get(&id).cloned().ok_or(FileError::AccessDenied)
    }

    fn font(&self, index: usize) -> Option<Font> {
        font_store().font(index)
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// 行高落到 Typst 的 top-edge/bottom-edge 上，按 Windows Word 的规则（见 metrics）。
/// `cjk` 是这一段里有没有中日韩文字，`marker` 是编号单独用的字体和它相对段落字号的比例。
fn line_edges(
    s: &Style,
    l: Option<LineHeight>,
    fs: f32,
    cjk: bool,
    marker: Option<(&str, f32)>,
) -> String {
    let m = metrics::style_line(s, cjk, marker);
    metrics::line_box(&m, l, fs).typst()
}
