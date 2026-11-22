//! 表格行高照 Windows Word 2024（2026-11-22 实测）：
//! - 最小行高 H 不含格子的上下边距和横线，含段前段后：行高 = max(段前 + 文字 + 段后, H) + 上下边距 + 横线。
//! - 固定行高画出来是 H + 下边距，上边距、横线都不加；最后一行再加下框线。
//!
//! docx 照写 w:trHeight，不变；pdf、html 按上面算。

use document_render::render::{Format, pdf};

/// 两行两列的表，0.5pt 框线；`extra` 接在 props 的 rows 后面，`parts` 是部位样式。
fn doc(heights: &str, parts: &str) -> String {
    format!(
        r##"{{
  "root": "d",
  "elements": {{
    "d": {{ "type": "Document", "children": ["t"] }},
    "t": {{ "type": "Table",
           "props": {{ "rows": [["a 甲", "b 乙"], ["c 丙", "d 丁"]], "rowHeights": {heights} }},
           "style": {{ "border": "0.5pt solid #000000" }},
           "parts": {{ {parts} }} }}
  }}
}}"##
    )
}

fn typst(json: &str) -> String {
    let (d, tree) = document_render::prepare(json, &document_render::Env::default()).unwrap();
    pdf::markup(&d, &tree, &Default::default()).0
}

fn html(json: &str) -> String {
    String::from_utf8(
        document_render::render_str(json, Format::Html, None)
            .unwrap()
            .bytes,
    )
    .unwrap()
}

fn docx_xml(json: &str) -> String {
    use std::io::Read;
    let b = document_render::render_str(json, Format::Docx, None)
        .unwrap()
        .bytes;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(b)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

#[test]
fn at_least_excludes_padding_and_lines() {
    let json = doc(
        r#"["40pt", "40pt"]"#,
        r#""cell": { "spaceBefore": 6, "spaceAfter": 4 }"#,
    );
    // pdf：文字块至少 H − 段前 − 段后 = 30pt，内边距、段前段后、线另外加在 inset 上
    let t = typst(&json);
    assert_eq!(t.matches("calc.max(h, 30pt)").count(), 4, "{t}");
    // html：tr 的 height = H + 上下边距 8，第一行再加上框线 0.5
    let h = html(&json);
    assert!(h.contains(r#"<tr style="height:48.5pt;">"#), "{h}");
    assert!(h.contains(r#"<tr style="height:48pt;">"#), "{h}");
    // docx 照写 H
    let x = docx_xml(&json);
    assert_eq!(
        x.matches(r#"<w:trHeight w:val="800" w:hRule="atLeast" />"#)
            .count(),
        2,
        "{x}"
    );
}

#[test]
fn thick_inner_line_beyond_top_padding_adds_to_row() {
    // 3pt 线、1pt 边距：行间的线先占上边距，多出来的 2pt 加进行高
    let json = doc(r#"["40pt", "40pt"]"#, "").replace(
        r#""style": { "border": "0.5pt solid #000000" }"#,
        r#""style": { "border": "3pt solid #000000", "cellPadding": 1 }"#,
    );
    let h = html(&json);
    assert!(h.contains(r#"<tr style="height:45pt;">"#), "{h}");
    assert!(h.contains(r#"<tr style="height:44pt;">"#), "{h}");
}

#[test]
fn exact_adds_bottom_padding() {
    let json = doc(
        r#"[{ "height": 30, "rule": "exact" }, { "height": 30, "rule": "exact" }]"#,
        r#""cell": { "paddingTop": 1, "paddingBottom": 9 }"#,
    );
    // pdf：H + 下边距 9；最后一行再加下框线 0.5
    let t = typst(&json);
    assert!(t.contains("rows: (39pt, 39.5pt,)"), "{t}");
    // html：tr 不写高度，定高 div = H + 下边距 − 上下边距 − 上下框线各一半 = 30 + 9 − 10 − 0.5
    let h = html(&json);
    assert!(!h.contains("<tr style=\"height"), "{h}");
    assert_eq!(
        h.matches(r#"<div style="height:28.5pt">"#).count(),
        4,
        "{h}"
    );
    let x = docx_xml(&json);
    assert_eq!(
        x.matches(r#"<w:trHeight w:val="600" w:hRule="exact" />"#)
            .count(),
        2,
        "{x}"
    );
}

#[test]
fn exact_with_default_padding_is_h_plus_4() {
    let json = doc(r#"[{ "height": 30, "rule": "exact" }, null]"#, "");
    let t = typst(&json);
    assert!(t.contains("rows: (34pt, auto,)"), "{t}");
}
