//! 字体检查：渲染 pdf、docx 之前，先把文档实际用到的字体在本机找一遍。
//!
//! 本机字体就是 pdf 排版用的那一套：Typst 自带的字体加系统字体（见 `render::pdf::font_store`）。
//! 规则（Rei 2026-11-21 定）：
//! - 用户写的字体要找得到。逗号分开的回退列表只要有一个找得到就行，一个都没有才算缺，
//!   报错时列出整条列表（Rei 2026-11-21 改，原先要求每一项都有）。列表里缺几个、但有能用的，
//!   什么都不报。中文名（宋体、微软雅黑……）按 [`CJK_NAMES`] 换成英文族名再找。
//! - 没写字体时用的内置默认（西文、中文、等宽各一组）是按平台挑着用的，一组里一个都找不到才算缺。
//! - 公式字体是 Typst 自带的，不查。

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;

use crate::{
    components::{Cell, Component, Run},
    doc::{Doc, Node},
    render::{Format, metrics::has_cjk},
    style::{FontFamily, Style},
};

/// 没写字体时的西文默认，按 macOS、Windows、Linux 的顺序。
pub const DEFAULT_LATIN: &[&str] = &["Helvetica Neue", "Arial", "Liberation Sans"];
/// 没写字体时的中文默认。
pub const DEFAULT_CJK: &[&str] = &[
    "PingFang SC",
    "Hiragino Sans GB",
    "Microsoft YaHei",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
];
/// Code、InlineCode 的默认等宽字体。
pub const DEFAULT_MONO: &[&str] = &["Menlo", "Consolas", "DejaVu Sans Mono"];

/// 没写字体时 docx 里写的默认西文字体（w:docDefaults 的 ascii、hAnsi、cs）。
/// parse 照原样读回，不跳过。
pub const DOCX_DEFAULT_LATIN: &str = "Arial";
/// 没写字体时 docx 里写的默认中文字体（w:docDefaults 的 eastAsia），parse 同样照原样读回。
pub const DOCX_DEFAULT_EAST_ASIA: &str = "PingFang SC";

/// 常见中日韩字体的本地名和英文族名。parse 读回时把本地名换成英文名，查字体时两种都认。
pub const CJK_NAMES: &[(&str, &str)] = &[
    ("宋体", "SimSun"),
    ("新宋体", "NSimSun"),
    ("黑体", "SimHei"),
    ("微软雅黑", "Microsoft YaHei"),
    ("微软雅黑 Light", "Microsoft YaHei Light"),
    ("楷体", "KaiTi"),
    ("楷体_GB2312", "KaiTi_GB2312"),
    ("仿宋", "FangSong"),
    ("仿宋_GB2312", "FangSong_GB2312"),
    ("等线", "DengXian"),
    ("等线 Light", "DengXian Light"),
    ("华文黑体", "STHeiti"),
    ("华文楷体", "STKaiti"),
    ("华文宋体", "STSong"),
    ("华文仿宋", "STFangsong"),
    ("华文中宋", "STZhongsong"),
    ("华文细黑", "STXihei"),
    ("华文行楷", "STXingkai"),
    ("华文新魏", "STXinwei"),
    ("华文隶书", "STLiti"),
    ("华文琥珀", "STHupo"),
    ("华文彩云", "STCaiyun"),
    ("幼圆", "YouYuan"),
    ("隶书", "LiSu"),
    ("方正舒体", "FZShuTi"),
    ("方正姚体", "FZYaoti"),
    ("方正小标宋简体", "FZXiaoBiaoSong-B05S"),
    ("方正小标宋_GBK", "FZXiaoBiaoSong-B05"),
    ("方正仿宋_GBK", "FZFangSong-Z02"),
    ("方正仿宋简体", "FZFangSong-Z02S"),
    ("方正黑体_GBK", "FZHei-B01"),
    ("方正黑体简体", "FZHei-B01S"),
    ("方正楷体_GBK", "FZKai-Z03"),
    ("方正楷体简体", "FZKai-Z03S"),
    ("方正书宋_GBK", "FZShuSong-Z01"),
    ("苹方", "PingFang SC"),
    ("苹方-简", "PingFang SC"),
    ("苹方-繁", "PingFang TC"),
    ("冬青黑体简体中文", "Hiragino Sans GB"),
    ("思源黑体", "Source Han Sans SC"),
    ("思源宋体", "Source Han Serif SC"),
    ("思源黑体 CN", "Source Han Sans CN"),
    ("思源宋体 CN", "Source Han Serif CN"),
    ("细明体", "MingLiU"),
    ("新细明体", "PMingLiU"),
    ("細明體", "MingLiU"),
    ("新細明體", "PMingLiU"),
    ("标楷体", "DFKai-SB"),
    ("標楷體", "DFKai-SB"),
    ("微軟正黑體", "Microsoft JhengHei"),
    ("微软正黑体", "Microsoft JhengHei"),
    ("ＭＳ 明朝", "MS Mincho"),
    ("ＭＳ ゴシック", "MS Gothic"),
    ("ＭＳ Ｐ明朝", "MS PMincho"),
    ("ＭＳ Ｐゴシック", "MS PGothic"),
    ("游ゴシック", "Yu Gothic"),
    ("游明朝", "Yu Mincho"),
    ("メイリオ", "Meiryo"),
    ("맑은 고딕", "Malgun Gothic"),
    ("바탕", "Batang"),
    ("굴림", "Gulim"),
    ("돋움", "Dotum"),
];

/// 本地名对应的英文族名，不认识是 None。
pub fn english_name(name: &str) -> Option<&'static str> {
    let n = name.trim();
    CJK_NAMES.iter().find(|(c, _)| *c == n).map(|(_, e)| *e)
}

/// 本机有没有这个字体。有的话返回 Typst 认的族名（中文名会换成英文名），没有是 None。
pub fn resolve(name: &str) -> Option<String> {
    let n = name.trim();
    if n.is_empty() {
        return None;
    }
    let book = crate::render::pdf::font_store().book();
    let has = |x: &str| book.contains_family(&x.to_lowercase());
    if has(n) {
        return Some(n.to_string());
    }
    if let Some(e) = english_name(n).filter(|e| has(e)) {
        return Some(e.to_string());
    }
    // 少数字体的族名表里第一条就是中文名，Typst 按它登记
    CJK_NAMES
        .iter()
        .find(|(c, e)| e.eq_ignore_ascii_case(n) && has(c))
        .map(|(c, _)| c.to_string())
}

/// 一个找不到的字体（回退列表一个都找不到时是整条列表），或者一组一个都找不到的内置默认字体。
#[derive(Debug, Clone)]
pub struct Missing {
    /// 用户写的族名，回退列表是整条（`A, B`）；内置默认时是这组的说法，比如「默认中文字体」
    pub name: String,
    /// 用户写的回退列表有几项
    pub count: usize,
    /// 内置默认时是这一组的字体
    pub defaults: Option<&'static [&'static str]>,
    /// 用到它的元素 id，按文档顺序
    pub ids: Vec<String>,
    /// 写在哪些样式键里
    pub sources: Vec<String>,
}

/// 最多列几个元素 id，多了写「等 N 处」。
const MAX_IDS: usize = 5;
const MAX_SOURCES: usize = 4;

impl Missing {
    /// 一行说明，不带前缀。
    pub fn describe(&self) -> String {
        let ids = list(&self.ids, MAX_IDS, "处");
        match self.defaults {
            Some(fs) => format!("{}（{}）一个都没有，用在 {ids}", self.name, fs.join("、")),
            None => {
                let src = if self.sources.is_empty() {
                    String::new()
                } else {
                    format!("，写在 {}", list(&self.sources, MAX_SOURCES, "个地方"))
                };
                let all = if self.count > 1 {
                    "（列表里一个都没有）"
                } else {
                    ""
                };
                format!("`{}`{all}，用在 {ids}{src}", self.name)
            }
        }
    }
}

fn list(xs: &[String], max: usize, unit: &str) -> String {
    let shown: Vec<String> = xs.iter().take(max).map(|x| format!("`{x}`")).collect();
    if xs.len() > max {
        format!("{} 等 {} {unit}", shown.join("、"), xs.len())
    } else {
        shown.join("、")
    }
}

/// 降级提示：命令行和 MCP 的参数名。
pub const ALLOW_HINT: &str =
    "命令行加 --allow-missing-fonts（MCP 设 allow_missing_fonts: true）可以降为警告，照常出文件";

/// 渲染 pdf、docx 时找不到字体的报错。
pub fn error_message(miss: &[Missing], format: Format) -> String {
    let fmt = format_name(format);
    let mut s = format!("本机找不到 {} 个字体，没有生成 {fmt}：", miss.len());
    for m in miss {
        s.push_str("\n- ");
        s.push_str(&m.describe());
    }
    s.push_str(&format!(
        "\n装上这些字体，或者换成本机有的字体；{ALLOW_HINT}。"
    ));
    s
}

/// 放行时每个缺的字体一条警告。
pub fn warnings(miss: &[Missing], format: Format) -> Vec<String> {
    let tail = match format {
        Format::Docx => "docx 里照写字体名，打开文件的电脑也没有时 Word 会自己换字体",
        _ => "pdf 里换成默认字体",
    };
    miss.iter()
        .map(|m| format!("本机找不到字体 {}；{tail}", m.describe()))
        .collect()
}

/// check 用：每个缺的字体一条，不带格式。
pub fn check_messages(miss: &[Missing]) -> Vec<String> {
    miss.iter()
        .map(|m| format!("本机找不到字体 {}", m.describe()))
        .collect()
}

fn format_name(f: Format) -> &'static str {
    match f {
        Format::Docx => "docx",
        Format::Pdf => "pdf",
        Format::Html => "html",
        Format::Markdown => "md",
    }
}

/// 找出文档用到、本机又没有的字体。
pub fn missing(doc: &Doc, tree: &Node) -> Vec<Missing> {
    missing_with(doc, tree, &|n| resolve(n).is_some())
}

/// [`missing`]，本机有没有某个字体由 `has` 判断（测试里用它模拟别的机器）。
pub fn missing_with(doc: &Doc, tree: &Node, has: &dyn Fn(&str) -> bool) -> Vec<Missing> {
    let mut c = Collector::default();
    c.walk(tree, &HashSet::new());
    let srcs = sources(doc);
    let mut out = Vec::new();
    for (names, ids) in c.user {
        if names.iter().any(|n| has(n)) {
            continue;
        }
        let sources = srcs
            .iter()
            .filter(|(_, v)| mentions(v, &names, false))
            .map(|(k, _)| k.clone())
            .collect();
        out.push(Missing {
            name: names.join(", "),
            count: names.len(),
            defaults: None,
            ids,
            sources,
        });
    }
    for (label, fs, ids) in [
        ("默认西文字体", DEFAULT_LATIN, c.latin),
        ("默认中文字体", DEFAULT_CJK, c.cjk),
        ("默认等宽字体", DEFAULT_MONO, c.mono),
    ] {
        if !ids.is_empty() && !fs.iter().any(|f| has(f)) {
            out.push(Missing {
                name: label.to_string(),
                count: fs.len(),
                defaults: Some(fs),
                ids,
                sources: Vec::new(),
            });
        }
    }
    out
}

/// 逗号分开的字体名拆开。
fn split(s: &str) -> impl Iterator<Item = &str> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty())
}

/// 这一项是不是内置的等宽字体列表（Code、InlineCode 的默认）。
fn is_default_mono(s: &str) -> bool {
    split(s).eq(DEFAULT_MONO.iter().copied())
}

/// 一条字体列表拆开以后的比较键（小写，逗号加空格连起来）。
fn key(names: &[String]) -> String {
    names.join(", ").to_lowercase()
}

/// 样式里用户写的字体列表：fontFamily 各文种（内置等宽列表除外）和 listMarkerFont，
/// 每个字段一条（逗号分开的回退列表拆成几项），一样的只留一条。
fn style_lists(s: &Style) -> Vec<Vec<String>> {
    let mut v: Vec<Vec<String>> = Vec::new();
    let mut push = |x: &str| {
        let l: Vec<String> = split(x).map(str::to_string).collect();
        if !l.is_empty() && !v.iter().any(|y| key(y) == key(&l)) {
            v.push(l);
        }
    };
    if let Some(f) = &s.font_family {
        for slot in slots(f) {
            if !is_default_mono(slot) {
                push(slot);
            }
        }
    }
    if let Some(m) = &s.list_marker_font {
        push(m);
    }
    v
}

fn slots(f: &FontFamily) -> impl Iterator<Item = &String> {
    [&f.latin, &f.east_asia, &f.complex].into_iter().flatten()
}

#[derive(Default)]
struct Collector {
    /// 用户写的字体列表：拆开的各项和用到它的元素，按第一次出现的顺序
    user: Vec<(Vec<String>, Vec<String>)>,
    latin: Vec<String>,
    cjk: Vec<String>,
    mono: Vec<String>,
}

fn add(v: &mut Vec<String>, id: &str) {
    if !v.iter().any(|x| x == id) {
        v.push(id.to_string());
    }
}

impl Collector {
    fn user_font(&mut self, names: Vec<String>, id: &str) {
        let k = key(&names);
        match self.user.iter_mut().find(|(n, _)| key(n) == k) {
            Some((_, ids)) => add(ids, id),
            None => self.user.push((names, vec![id.to_string()])),
        }
    }

    /// `s` 里有、`known` 里没有的用户字体列表记到 `id` 上。
    fn names(&mut self, s: &Style, known: &HashSet<String>, id: &str) {
        for l in style_lists(s) {
            if !known.contains(&key(&l)) {
                self.user_font(l, id);
            }
        }
    }

    /// 这段文字要不要用到内置默认字体。
    fn defaults(&mut self, s: &Style, text: &str, id: &str) {
        let f = s.font_family.as_ref();
        let latin = f.and_then(|f| f.latin.as_deref());
        if text
            .chars()
            .any(|c| !c.is_whitespace() && !has_cjk(c.encode_utf8(&mut [0; 4])))
        {
            match latin {
                Some(l) if is_default_mono(l) => add(&mut self.mono, id),
                Some(_) => {}
                None => add(&mut self.latin, id),
            }
        }
        if has_cjk(text) && f.and_then(|f| f.east_asia.as_ref()).is_none() {
            add(&mut self.cjk, id);
        }
    }

    fn run(&mut self, r: &Run, known: &HashSet<String>, id: &str) {
        self.names(&r.computed, known, id);
        if r.is_math() || r.image.is_some() {
            return;
        }
        let text = if r.field.is_some() {
            "1"
        } else {
            r.text.as_str()
        };
        self.defaults(&r.computed, text, id);
    }

    fn walk(&mut self, n: &Node, parent: &HashSet<String>) {
        let id = n.id.as_str();
        self.names(&n.style, parent, id);
        let known: HashSet<String> = style_lists(&n.style).iter().map(|l| key(l)).collect();
        for p in n.parts.values() {
            self.names(p, &known, id);
        }
        let mut comp = n.component.clone();
        for t in comp.inlines_mut() {
            t.normalize();
            for r in t.runs() {
                self.run(r, &known, id);
            }
        }
        match &n.component {
            Component::Table(t) => {
                for c in t.columns.iter().chain(t.rows.iter().flatten()) {
                    if let Cell::Full(c) = c {
                        self.names(&c.computed, &known, id);
                    }
                }
            }
            Component::Code(c) => self.defaults(&n.style, &c.code, id),
            Component::Image(i) => {
                if let Some(cap) = &i.caption {
                    self.defaults(&n.style, cap, id);
                }
            }
            Component::TableOfContents(t) => {
                if let Some(x) = &t.title {
                    self.defaults(&n.style, x, id);
                }
            }
            Component::Slide(s) => {
                if let Some(x) = &s.title {
                    self.defaults(&n.style, x, id);
                }
            }
            Component::Section(s) => {
                if let Some(l) = &s.layout {
                    for hf in [&l.header, &l.footer] {
                        let all = hf
                            .default
                            .iter()
                            .chain(hf.first.iter().flatten())
                            .chain(hf.even.iter().flatten());
                        for k in all {
                            self.walk(k, &known);
                        }
                    }
                }
            }
            _ => {}
        }
        for k in n.children.iter().chain(n.list_blocks()) {
            self.walk(k, &known);
        }
    }
}

/// 文档里写样式的地方：键的说法和原始 JSON。
fn sources(doc: &Doc) -> Vec<(String, Value)> {
    let mut v = Vec::new();
    for (k, x) in &doc.styles {
        v.push((format!("styles.{k}"), x.clone()));
    }
    let mut sets: BTreeMap<&String, _> = doc.external_style_sets.iter().collect();
    for (k, s) in &doc.style_sets {
        sets.insert(k, s);
    }
    for (name, s) in sets {
        if let Some(g) = &s.global {
            v.push((format!("styleSets.{name}.global"), g.clone()));
        }
        for (k, x) in &s.components {
            v.push((format!("styleSets.{name}.components.{k}"), x.clone()));
        }
        for (k, x) in &s.styles {
            v.push((format!("styleSets.{name}.styles.{k}"), x.clone()));
        }
    }
    for (id, el) in &doc.elements {
        if let Some(s) = &el.style {
            v.push((format!("元素 {id} 的 style"), s.clone()));
        }
        for (p, x) in &el.parts {
            v.push((format!("元素 {id} 的 parts.{p}"), x.clone()));
        }
        v.push((format!("元素 {id} 的 props"), el.props.clone()));
        for t in &el.template_styles {
            v.push((format!("元素 {id} 所属自定义组件的模板"), t.clone()));
        }
        for t in el.template_parts.iter().flat_map(|m| m.values()) {
            v.push((format!("元素 {id} 所属自定义组件的模板"), t.clone()));
        }
    }
    v.dedup_by(|a, b| a.0 == b.0);
    v
}

/// JSON 里有没有在字体字段（fontFamily、latin、eastAsia、complex、listMarkerFont，
/// 或者 `$变量`）里写到这条字体列表（整条一样，不分大小写）。`in_font` 表示当前已经在字体字段里面。
fn mentions(v: &Value, names: &[String], in_font: bool) -> bool {
    match v {
        Value::String(s) => {
            in_font && {
                let l: Vec<String> = split(s).map(str::to_string).collect();
                key(&l) == key(names)
            }
        }
        Value::Array(a) => a.iter().any(|x| mentions(x, names, in_font)),
        Value::Object(m) => m.iter().any(|(k, x)| {
            let font = in_font
                || matches!(
                    k.as_str(),
                    "fontFamily" | "latin" | "eastAsia" | "complex" | "listMarkerFont"
                )
                || k.starts_with('$');
            mentions(x, names, font)
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_default_list_is_the_three_groups() {
        let mut v: Vec<&str> = DEFAULT_LATIN.to_vec();
        v.extend(DEFAULT_CJK);
        v.push("Libertinus Serif");
        assert_eq!(v, crate::render::pdf::FONTS);
    }

    #[test]
    fn local_names_map_to_english() {
        assert_eq!(english_name("宋体"), Some("SimSun"));
        assert_eq!(english_name(" 微软雅黑 "), Some("Microsoft YaHei"));
        assert_eq!(english_name("Arial"), None);
    }

    fn l(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn mentions_only_font_fields() {
        let v = serde_json::json!({ "text": "Arial", "style": { "fontFamily": { "latin": "Inter,Arial" } } });
        assert!(mentions(&v, &l(&["inter", "arial"]), false));
        assert!(!mentions(&v, &l(&["Arial"]), false), "整条列表一样才算");
        assert!(!mentions(
            &serde_json::json!({ "text": "Arial" }),
            &l(&["Arial"]),
            false
        ));
        assert!(mentions(
            &serde_json::json!({ "$body": "Inter" }),
            &l(&["Inter"]),
            false
        ));
    }

    #[test]
    fn docx_defaults_are_in_the_default_groups() {
        assert!(DEFAULT_LATIN.contains(&DOCX_DEFAULT_LATIN));
        assert!(DEFAULT_CJK.contains(&DOCX_DEFAULT_EAST_ASIA));
    }

    #[test]
    fn id_list_truncates() {
        let ids: Vec<String> = (1..=12).map(|i| format!("p{i}")).collect();
        assert_eq!(list(&ids, 5, "处"), "`p1`、`p2`、`p3`、`p4`、`p5` 等 12 处");
    }
}
