//! `line` 单位照 Word 没有文档网格时的算法：1 行 = 12pt，跟字号无关（Rei 2026-11-22 定）。
//! docx 的段前段后照写 beforeLines、afterLines，Word 自己算；pdf、html 按 12pt 换算。
//! 多倍行距（`lineHeight` 写数字或 `{ multiple }`）不受影响，还是看字体。

use std::io::Read;

use document_render::render::{Format, pdf};
use document_render::style::{LINE_PT, Len};

fn document_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

/// 三种格式的输出：docx 的 document.xml、pdf 的 Typst 源码、html。
fn outputs(json: &str) -> [String; 3] {
    let (d, tree) = document_render::prepare(json, &document_render::Env::default()).unwrap();
    let (typ, _, _) = pdf::markup(&d, &tree, &Default::default());
    let r = |f| document_render::render_str(json, f, None).unwrap().bytes;
    [
        document_xml(&r(Format::Docx)),
        typ,
        String::from_utf8(r(Format::Html)).unwrap(),
    ]
}

/// 一段正文，`style` 是它的样式。
fn para(style: &str) -> String {
    format!(
        r#"{{
  "root": "d",
  "elements": {{
    "d": {{ "type": "Document", "children": ["p", "q"] }},
    "p": {{ "type": "Text", "props": {{ "text": "第一段" }} }},
    "q": {{ "type": "Text", "props": {{ "text": "第二段" }}, "style": {{ {style} }} }}
  }}
}}"#
    )
}

#[test]
fn one_line_is_12pt_at_any_font_size() {
    assert_eq!(LINE_PT, 12.0);
    for fs in [5.0, 10.5, 12.0, 14.0, 24.0, 72.0] {
        assert_eq!(Len::Line(1.0).pt(fs, 0.0), 12.0, "字号 {fs}");
        assert_eq!(Len::Line(0.5).pt(fs, 0.0), 6.0, "字号 {fs}");
        assert_eq!(Len::Line(2.5).pt(fs, 0.0), 30.0, "字号 {fs}");
    }
    assert_eq!(Len::Line(1.0).abs_pt(), 12.0);
}

#[test]
fn space_before_after_in_lines() {
    for fs in [10.5, 12.0, 14.0, 24.0] {
        let json = para(&format!(
            r#""fontSize": {fs}, "spaceBefore": "1line", "spaceAfter": "0.5line""#
        ));
        let [docx, typ, html] = outputs(&json);
        assert!(docx.contains(r#"w:beforeLines="100""#), "{docx}");
        assert!(docx.contains(r#"w:afterLines="50""#), "{docx}");
        assert!(typ.contains("above: 12pt"), "字号 {fs}：{typ}");
        assert!(typ.contains("below: 6pt"), "字号 {fs}：{typ}");
        assert!(html.contains("margin-top:12pt"), "字号 {fs}：{html}");
        assert!(html.contains("margin-bottom:6pt"), "字号 {fs}：{html}");
    }
}

#[test]
fn line_unit_elsewhere_is_12pt_too() {
    // 固定行高写成行数：Word 那边写的是算好的 twip，也按 12pt 一行
    let [docx, _, html] = outputs(&para(r#""fontSize": 20, "lineHeight": "2line""#));
    assert!(
        docx.contains(r#"w:line="480""#) && docx.contains(r#"w:lineRule="exact""#),
        "{docx}"
    );
    assert!(html.contains("line-height:24pt"), "{html}");
    // 缩进写成行数（少见，但 schema 收）
    let [docx, typ, html] = outputs(&para(r#""fontSize": 20, "marginLeft": "1line""#));
    assert!(docx.contains(r#"w:left="240""#), "{docx}");
    assert!(typ.contains("12pt"), "{typ}");
    assert!(html.contains("margin-left:12pt"), "{html}");
}

#[test]
fn multiple_line_spacing_still_follows_font() {
    // 多倍行距不受 line 单位影响：数字照旧是「至少 字号 × 倍数」，multiple 照旧写 auto
    let [docx, _, _] = outputs(&para(r#""fontSize": 20, "lineHeight": 1.5"#));
    assert!(
        docx.contains(r#"w:line="600""#) && docx.contains(r#"w:lineRule="atLeast""#),
        "{docx}"
    );
    let [docx, _, _] = outputs(&para(
        r#""fontSize": 20, "lineHeight": { "multiple": 1.5 }"#,
    ));
    assert!(
        docx.contains(r#"w:line="360""#) && docx.contains(r#"w:lineRule="auto""#),
        "{docx}"
    );
}

#[test]
fn table_space_after_in_lines() {
    // 表格后的间距靠一个 1pt 高的空段落，line 单位也换成 pt 再扣掉这 1pt：1 行 → 11pt
    let json = r#"{
  "root": "d",
  "elements": {
    "d": { "type": "Document", "children": ["t", "p"] },
    "t": { "type": "Table", "props": { "columns": ["名称"], "rows": [["苹果"]] },
           "style": { "fontSize": 24, "spaceAfter": "1line" } },
    "p": { "type": "Text", "props": { "text": "后面一段" } }
  }
}"#;
    let [docx, typ, html] = outputs(json);
    assert!(docx.contains(r#"w:after="220""#), "{docx}");
    assert!(!docx.contains("w:afterLines"), "{docx}");
    // pdf 另加 Word 表格后的 0.04pt（见 metrics::WORD_TABLE_TAIL）
    assert!(typ.contains("below: 12.04pt"), "{typ}");
    assert!(html.contains("margin-bottom:12pt"), "{html}");
}
