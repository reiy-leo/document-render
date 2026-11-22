//! Divider 照 Windows Word 2024（2026-11-22 实测）：Word 里它是一个空段落，
//! 高度 = 段前 + 一行（默认固定 1pt）+ 下内边距 + 下边框线宽 + 段后。
//! 相邻两条 Divider 边框一样，Word 合成一个框，中间只隔段后。
//!
//! pdf 看 Typst 源码：空段落里放一个零宽字符撑出这一行，线宽一半算进里面块的内边距，
//! 另一半放在外层块上。html 看 hr 的样式：content-box 加一行的高度，边框、内边距另算。

use document_render::render::{Format, pdf};

fn doc(children: &[(&str, &str)]) -> String {
    let ids: Vec<String> = children.iter().map(|(id, _)| format!("\"{id}\"")).collect();
    let els: Vec<String> = children
        .iter()
        .map(|(id, body)| format!("\"{id}\": {body}"))
        .collect();
    format!(
        r#"{{ "root": "d", "elements": {{ "d": {{ "type": "Document", "children": [{}] }}, {} }} }}"#,
        ids.join(", "),
        els.join(", ")
    )
}

fn typst(json: &str) -> String {
    let d = document_render::doc::parse(json).unwrap();
    let tree = d.tree().unwrap();
    pdf::markup(&d, &tree, &Default::default()).0
}

fn html(json: &str) -> String {
    let out = document_render::render_str(json, Format::Html, None).unwrap();
    String::from_utf8(out.bytes).unwrap()
}

const PLAIN: &str = r#"{ "type": "Divider" }"#;
const THICK: &str = r##"{ "type": "Divider", "style": { "borderBottom": "3pt solid #000000", "paddingBottom": 4 } }"##;

#[test]
fn pdf_divider_is_an_empty_paragraph_with_its_line_counted() {
    let src = typst(&doc(&[("a", PLAIN)]));
    // 一行：空段落里的零宽字符
    assert!(src.contains("#par[#sym.zws]"), "{src}");
    // 默认 0.5pt 线：Word 画成 0.48pt，贴着里侧，里面留 0.24pt，外层补到 0.5pt
    assert!(
        src.contains("#block(above: 6pt, below: 12pt, inset: (top: 0pt, bottom: 0.26pt), width: 100%)[#block(stroke: (bottom: (paint: rgb(\"#D0D7DE\"), thickness: 0.48pt)), inset: (bottom: 0.24pt), width: 100%)"),
        "{src}"
    );
}

#[test]
fn pdf_thick_line_and_padding_add_up() {
    let src = typst(&doc(&[("b", THICK)]));
    // 3pt 线：外层 1.5pt，里面 4pt 内边距 + 1.5pt
    assert!(
        src.contains("inset: (top: 0pt, bottom: 1.5pt), width: 100%)[#block(stroke: (bottom: (paint: rgb(\"#000000\"), thickness: 3pt)), inset: (bottom: 5.5pt)"),
        "{src}"
    );
}

#[test]
fn pdf_two_dividers_share_one_box() {
    let src = typst(&doc(&[("x", PLAIN), ("y", PLAIN)]));
    assert_eq!(src.matches("thickness: 0.48pt").count(), 1, "{src}");
    assert_eq!(src.matches("#par[#sym.zws]").count(), 2, "{src}");
    // 两段之间隔第一段的段后
    assert!(
        src.contains("inset: (top: 0pt, right: 0pt, bottom: 12pt, left: 0pt)"),
        "{src}"
    );
}

#[test]
fn html_hr_has_one_line_and_content_box() {
    let h = html(&doc(&[("a", PLAIN), ("b", THICK)]));
    assert!(
        h.contains(r#"<hr id="a" style="box-sizing:content-box;height:1.000pt;margin-top:6pt;margin-bottom:12pt;"#),
        "{h}"
    );
    // 边框和内边距不挤进 height
    assert!(
        h.contains(r#"<hr id="b" style="box-sizing:content-box;height:1.000pt;"#)
            && h.contains("border-bottom:3pt solid #000000;padding-bottom:4pt;"),
        "{h}"
    );
}

#[test]
fn html_line_height_sets_the_hr_height() {
    let h = html(&doc(&[(
        "c",
        r#"{ "type": "Divider", "style": { "lineHeight": "6pt" } }"#,
    )]));
    assert!(h.contains("box-sizing:content-box;height:6.000pt;"), "{h}");
}

#[test]
fn html_two_dividers_share_one_box() {
    let h = html(&doc(&[("x", PLAIN), ("y", PLAIN)]));
    assert_eq!(h.matches("document-render-pbdr").count(), 1, "{h}");
    assert_eq!(
        h.matches("box-sizing:content-box;height:1.000pt;").count(),
        2,
        "{h}"
    );
}
