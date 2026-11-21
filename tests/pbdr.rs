//! 段落边框合框：相邻、四条边和左右缩进都一样的段落，像 Windows Word 那样画成一个框
//! （src/render/common.rs 的 pbdr_groups / pbdr_group）。pdf 看 Typst 源码，html 看结构，
//! docx 仍然每段照写 w:pBdr，parse 读回不受影响。

use std::path::Path;

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

fn text(t: &str, style: &str) -> String {
    format!(r#"{{ "type": "Text", "props": {{ "text": "{t}" }}, "style": {style} }}"#)
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

/// Typst 源码里画了几个带边框的块：参数里写了 stroke 的 #block（表格格子、下划线不算）。
fn boxes(src: &str) -> usize {
    let mut n = 0;
    for (i, _) in src.match_indices("#block(") {
        let args = &src[i + 7..];
        let mut depth = 1;
        let end = args
            .char_indices()
            .find(|(_, c)| {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                depth == 0
            })
            .map_or(args.len(), |(j, _)| j);
        if args[..end].contains("stroke: (") {
            n += 1;
        }
    }
    n
}

/// Text 内置段后 6pt，这里清零，好看清框里的间距。
const BOX: &str = r##"{ "border": "1pt solid #FF0000", "padding": 4, "spaceAfter": 0 }"##;

#[test]
fn same_borders_make_one_box() {
    let json = doc(&[
        ("p1", &text("一", BOX)),
        ("p2", &text("二", BOX)),
        ("p3", &text("三", BOX)),
    ]);
    let src = typst(&json);
    assert_eq!(boxes(&src), 1, "{src}");
    // 框的上下内边距只在第一段上面、最后一段下面
    assert!(
        src.contains("inset: (top: 4pt, right: 4pt, bottom: 0pt, left: 4pt)"),
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 0pt, right: 4pt, bottom: 0pt, left: 4pt)"),
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 0pt, right: 4pt, bottom: 4pt, left: 4pt)"),
        "{src}"
    );

    let h = html(&json);
    assert_eq!(
        h.matches("class=\"document-render-pbdr\"").count(),
        1,
        "{h}"
    );
    assert_eq!(h.matches("border-top:1pt solid").count(), 1, "{h}");
}

#[test]
fn between_line_and_spacing_follow_word() {
    let line = r##"{ "border": "1pt solid #FF0000", "padding": 4, "borderBetween": "0.5pt solid #FF8800", "spaceAfter": 12 }"##;
    let p2 = r##"{ "border": "1pt solid #FF0000", "padding": 4, "borderBetween": "0.5pt solid #FF8800", "spaceBefore": 18, "spaceAfter": 0 }"##;
    let json = doc(&[
        ("p1", &text("一", line)),
        ("p2", &text("二", p2)),
        ("p3", &text("三", BOX)),
    ]);
    let src = typst(&json);
    assert_eq!(boxes(&src), 1, "{src}");
    // 有线：上一段的段后在线上面，线下面空段前比段后多的 6pt
    assert!(
        src.contains("inset: (top: 4pt, right: 4pt, bottom: 12pt, left: 4pt)"),
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 6pt, right: 4pt, bottom: 0pt, left: 4pt)"),
        "{src}"
    );
    // 两条线：p1、p2 都写了 borderBetween（p3 的用不上，它也没写）
    assert_eq!(src.matches("height: 0.5pt").count(), 2, "{src}");
    assert!(
        src.contains("fill: rgb(\"#ff8800\")") || src.contains("fill: rgb(\"#FF8800\")"),
        "{src}"
    );
    let out = document_render::render_str(&json, Format::Pdf, None).unwrap();
    assert!(
        !out.warnings.iter().any(|w| w.contains("borderBetween")),
        "{:?}",
        out.warnings
    );

    let h = html(&json);
    assert_eq!(h.matches("document-render-pbdr-between").count(), 2, "{h}");
    let out = document_render::render_str(&json, Format::Html, None).unwrap();
    assert!(
        !out.warnings.iter().any(|w| w.contains("borderBetween")),
        "{:?}",
        out.warnings
    );
}

#[test]
fn without_line_the_gap_is_split_like_word() {
    let a = r##"{ "border": "1pt solid #FF0000", "spaceAfter": 6 }"##;
    let b = r##"{ "border": "1pt solid #FF0000", "spaceBefore": 12 }"##;
    let src = typst(&doc(&[("p1", &text("一", a)), ("p2", &text("二", b))]));
    assert_eq!(boxes(&src), 1, "{src}");
    // 两段之间空 12pt：上一段的段后 6pt 归上一段，多出来的 6pt 归下一段（各自铺各自的底色）
    assert!(
        src.contains("inset: (top: 0pt, right: 0pt, bottom: 6pt, left: 0pt)"),
        "{src}"
    );
    assert!(
        src.contains("inset: (top: 6pt, right: 0pt, bottom: 0pt, left: 0pt)"),
        "{src}"
    );
    // 第一段的段前、最后一段的段后在框外：这里都是 0
    assert!(src.contains("above: 0pt, below: 0pt"), "{src}");
}

#[test]
fn what_breaks_a_group() {
    let indented = r##"{ "border": "1pt solid #FF0000", "padding": 4, "marginLeft": 20 }"##;
    let blue = r##"{ "border": "1pt solid #0000FF", "padding": 4 }"##;
    let pad8 = r##"{ "border": "1pt solid #FF0000", "padding": 8 }"##;
    let table = r#"{ "type": "Table", "props": { "rows": [["x"]] } }"#;
    let plain = r#"{ "type": "Text", "props": { "text": "" } }"#;
    for (what, mid) in [
        ("左缩进不同", text("中", indented)),
        ("边框颜色不同", text("中", blue)),
        ("内边距不同", text("中", pad8)),
        ("中间夹表格", table.to_string()),
        ("中间夹没边框的空段落", plain.to_string()),
    ] {
        let src = typst(&doc(&[
            ("p1", &text("一", BOX)),
            ("m", &mid),
            ("p3", &text("三", BOX)),
        ]));
        let want = if mid.contains("\"border\"") { 3 } else { 2 };
        assert_eq!(boxes(&src), want, "{what}：{src}");
    }
}

#[test]
fn background_alignment_and_first_line_indent_do_not_break_it() {
    let a = r##"{ "border": "1pt solid #FF0000", "background": "#E8F0FE" }"##;
    let b = r##"{ "border": "1pt solid #FF0000", "background": "#FEF7E0", "align": "center", "textIndent": "2em" }"##;
    let src = typst(&doc(&[("p1", &text("一", a)), ("p2", &text("二", b))]));
    assert_eq!(boxes(&src), 1, "{src}");
    assert_eq!(
        src.matches("fill: rgb(").count(),
        2,
        "两段各铺自己的底色：{src}"
    );
}

#[test]
fn quotes_callouts_and_dividers_merge_too() {
    let q = r#"{ "type": "Quote", "props": { "text": "引" } }"#;
    let c = r#"{ "type": "Callout", "props": { "text": "提" } }"#;
    let dv = r#"{ "type": "Divider" }"#;
    for (what, x) in [("Quote", q), ("Callout", c), ("Divider", dv)] {
        let json = doc(&[("a", x), ("b", x)]);
        let src = typst(&json);
        assert_eq!(boxes(&src), 1, "{what}：{src}");
        let h = html(&json);
        assert_eq!(
            h.matches("class=\"document-render-pbdr\"").count(),
            1,
            "{what}：{h}"
        );
    }
    // Quote 和带同样左边框的 Text 不合：docx 里 Quote 的左缩进加了线宽和内边距
    let t = r##"{ "type": "Text", "props": { "text": "t" }, "style": { "borderLeft": "3pt solid #D0D7DE", "paddingLeft": 10 } }"##;
    let src = typst(&doc(&[("a", q), ("b", t)]));
    assert_eq!(boxes(&src), 2, "{src}");
}

#[test]
fn numbered_headings_stay_alone() {
    let json = r##"{
      "styles": { "Heading.1": { "listStyleType": "decimal", "border": "1pt solid #FF0000" } },
      "root": "d",
      "elements": {
        "d": { "type": "Document", "children": ["a", "b"] },
        "a": { "type": "Heading", "props": { "level": 1, "text": "一" } },
        "b": { "type": "Heading", "props": { "level": 1, "text": "二" } }
      }
    }"##;
    assert_eq!(boxes(&typst(json)), 2);
}

#[test]
fn docx_keeps_pbdr_on_every_paragraph_and_parse_reads_it_back() {
    let line = r##"{ "border": "1pt solid #FF0000", "padding": 4, "borderBetween": "0.5pt solid #FF8800" }"##;
    let json = doc(&[("p1", &text("一", line)), ("p2", &text("二", line))]);
    let out = document_render::render_str(&json, Format::Docx, None).unwrap();
    let parsed = document_render::parse::docx(
        &out.bytes,
        &document_render::parse::Options {
            assets_dir: "a".into(),
        },
    )
    .unwrap();
    let s = parsed.json.to_string();
    // 两段都带四边和 between，读回来仍是两段，样式一样
    assert_eq!(s.matches("一").count() + s.matches("二").count(), 2, "{s}");
    assert!(s.contains("borderBetween"), "{s}");
    let back = serde_json::to_string(&parsed.json).unwrap();
    // 读回的 JSON 带着 docx 的默认字体 Arial、PingFang SC，本机不一定有，放行缺字体
    let allow = document_render::Env {
        allow_missing_fonts: true,
        ..Default::default()
    };
    let again =
        document_render::render_with(&back, Format::Pdf, Some(Path::new(".")), &allow).unwrap();
    assert!(again.bytes.starts_with(b"%PDF-"));
}

#[test]
fn example_renders() {
    let json = include_str!("../examples/paragraph-borders.json");
    for f in [Format::Html, Format::Pdf, Format::Docx] {
        let out = document_render::render_str(json, f, Some(Path::new("examples"))).unwrap();
        assert!(
            !out.warnings.iter().any(|w| w.contains("borderBetween")),
            "{f:?}：{:?}",
            out.warnings
        );
    }
    let src = typst(json);
    // a1–a3 一框、b1–b2 一框、b3、c1–c2、q1–q2、d1–d2、e1、e3
    assert_eq!(boxes(&src), 8, "{src}");
}
