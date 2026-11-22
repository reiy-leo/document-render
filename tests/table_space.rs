//! 表格自己没有段前（Rei 2026-11-21 定）：Table 的 style、组件样式 `Table` 写了 `spaceBefore`，
//! check 给警告，docx、pdf、html 都不管，parse 也不输出。
//! 部位（`cell`、`header`、`oddRow` 这些）上的 `spaceBefore` 是格子里段落的段前，照旧（Rei 2026-11-22 定）。
//! 部位和格子 style 上的段前段后三种格式都画：docx 写在格子段落上，pdf、html 叠在格子上下内边距上
//! （2026-11-22 和 Windows Word 2024 比过：格子里的文字和行高差都在 0.2pt 内）。

use std::io::Read;

use document_render::render::{Format, pdf};
use serde_json::Value;

/// 一段正文后面跟一张表。`table` 是表格元素里 props 之后的部分（style、parts），`styles` 是顶层 styles。
fn doc(table: &str, styles: &str) -> String {
    format!(
        r#"{{
  "root": "d",
  "styles": {{ {styles} }},
  "elements": {{
    "d": {{ "type": "Document", "children": ["p", "t"] }},
    "p": {{ "type": "Text", "props": {{ "text": "表格前面的一段" }} }},
    "t": {{ "type": "Table", "props": {{ "columns": ["名称", "数量"], "rows": [["苹果", "3"]] }}{table} }}
  }}
}}"#
    )
}

fn document_xml(bytes: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

fn render(json: &str, f: Format) -> document_render::render::Output {
    document_render::render_str(json, f, None).unwrap()
}

fn warnings(json: &str) -> Vec<String> {
    let (d, _) = document_render::prepare(json, &document_render::Env::default()).unwrap();
    document_render::check_styles(&d)
}

/// 三种格式的输出：docx 的 document.xml、pdf 的 Typst 源码、html。
fn outputs(json: &str) -> [String; 3] {
    let (d, tree) = document_render::prepare(json, &document_render::Env::default()).unwrap();
    let (typ, _, _) = pdf::markup(&d, &tree, &Default::default());
    [
        document_xml(&render(json, Format::Docx).bytes),
        typ,
        String::from_utf8(render(json, Format::Html).bytes).unwrap(),
    ]
}

#[test]
fn own_space_before_warns_and_changes_nothing() {
    let plain = doc("", "");
    let with = doc(r#", "style": { "spaceBefore": 30 }"#, "");
    assert!(warnings(&plain).is_empty());
    let w = warnings(&with);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(
        w[0].contains("`t`") && w[0].contains("spaceBefore"),
        "{w:?}"
    );
    // 三种格式和没写一模一样
    let [a, b, c] = outputs(&plain);
    let [x, y, z] = outputs(&with);
    assert_eq!(a, x, "docx 不该变");
    assert_eq!(b, y, "pdf 不该变");
    assert_eq!(c, z, "html 不该变");
    assert!(!z.contains("margin-top:30pt"));
    // 渲染也把警告带出来
    for f in [Format::Docx, Format::Pdf, Format::Html, Format::Markdown] {
        let out = render(&with, f);
        assert!(
            out.warnings.iter().any(|x| x.contains("spaceBefore")),
            "{f:?}：{:?}",
            out.warnings
        );
    }
}

#[test]
fn component_style_warns_once() {
    // styles 里的 Table 写了：两张表也只报一次，说清是组件样式
    let json = r#"{
  "root": "d",
  "styles": { "Table": { "spaceBefore": "1line" } },
  "elements": {
    "d": { "type": "Document", "children": ["t1", "t2"] },
    "t1": { "type": "Table", "props": { "rows": [["a"]] } },
    "t2": { "type": "Table", "props": { "rows": [["b"]] } }
  }
}"#;
    let w = warnings(json);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].contains("组件样式 `Table`"), "{w:?}");
    let [x, y, z] = outputs(json);
    let [a, b, c] = outputs(&json.replace(r#"{ "spaceBefore": "1line" }"#, "{}"));
    assert_eq!((a, b, c), (x, y, z));
}

#[test]
fn named_style_on_the_element_counts_as_its_own() {
    let json = doc(
        r#", "style": "gap""#,
        r#""gap": { "spaceBefore": 12, "spaceAfter": 20 }"#,
    );
    let w = warnings(&json);
    assert_eq!(w.len(), 1, "{w:?}");
    assert!(w[0].starts_with("元素 `t` 的 style"), "{w:?}");
    // 段后照样起作用
    let h = String::from_utf8(render(&json, Format::Html).bytes).unwrap();
    assert!(h.contains("margin-bottom:20pt"), "{h}");
}

#[test]
fn part_space_before_is_the_cell_paragraph_spacing() {
    // 部位上的 spaceBefore 不报警告，docx 写在格子段落的段前上
    let json = doc(r#", "parts": { "cell": { "spaceBefore": 9 } }"#, "");
    assert!(warnings(&json).is_empty());
    let x = document_xml(&render(&json, Format::Docx).bytes);
    let cells: Vec<&str> = x.split("<w:tc>").skip(1).collect();
    assert_eq!(cells.len(), 4, "{x}");
    for c in cells {
        assert!(
            c.contains(r#"w:before="180""#),
            "格子段落该有 9pt 段前：{c}"
        );
    }
}

#[test]
fn parse_never_writes_table_space_before() {
    // 前一段段后 20pt、格子段落段前 9pt：读回来表格自己没有 spaceBefore，格子的段前还在
    let json = doc(r#", "parts": { "cell": { "spaceBefore": 9 } }"#, "");
    let json = json.replace(
        r#""props": { "text": "表格前面的一段" }"#,
        r#""props": { "text": "表格前面的一段" }, "style": { "spaceAfter": 20 }"#,
    );
    let docx = render(&json, Format::Docx).bytes;
    let parsed = document_render::parse::docx(
        &docx,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap();
    let els = parsed.json["elements"].as_object().unwrap();
    let (_, t) = els
        .iter()
        .find(|(_, e)| e["type"] == "Table")
        .expect("读回来该有表格");
    assert!(t["style"].get("spaceBefore").is_none(), "{t}");
    assert!(
        parsed.json["styles"]
            .get("Table")
            .and_then(|s| s.get("spaceBefore"))
            .is_none()
    );
    let text = serde_json::to_string(t).unwrap();
    assert!(text.contains(r#""spaceBefore":9"#), "格子段前该读回来：{t}");
    // 前一段的段后还在前一段上
    let (_, p) = els
        .iter()
        .find(|(_, e)| e["type"] == "Text")
        .expect("读回来该有段落");
    let after = p["style"]
        .get("spaceAfter")
        .or_else(|| parsed.json["styles"]["Text"].get("spaceAfter"));
    assert_eq!(after, Some(&Value::from(20)), "{p}");
}

#[test]
fn check_command_prints_the_warning() {
    let dir = std::env::temp_dir().join(format!("dr-table-space-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("t.json");
    std::fs::write(&f, doc(r#", "style": { "spaceBefore": 6 }"#, "")).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_document-render"))
        .arg("check")
        .arg(&f)
        .arg("--allow-missing-fonts")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(
        err.contains("警告：元素 `t` 的 style 写了 `spaceBefore`"),
        "{err}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// 两行的表，表格 style 和 parts 由调用方给。
fn table_doc(rest: &str) -> String {
    format!(
        r#"{{
  "root": "d",
  "elements": {{
    "d": {{ "type": "Document", "children": ["t"] }},
    "t": {{ "type": "Table", "props": {{ "columns": ["名称", "数量"], "rows": [["苹果", {{ "text": "3", "style": {{ "spaceBefore": 10, "spaceAfter": 2 }} }}], ["梨", "4"]] }}{rest} }}
  }}
}}"#
    )
}

#[test]
fn pdf_adds_cell_spacing_to_insets() {
    // 默认 cellPadding 4pt、0.5pt 边框：表头段前 6 → 上 4 + 0.5（上框线）+ 6；cell 段后 5 → 下 9；
    // 格子自己的 style 盖掉部位的
    let json =
        table_doc(r#", "parts": { "header": { "spaceBefore": 6 }, "cell": { "spaceAfter": 5 } }"#);
    let [_, typ, _] = outputs(&json);
    assert!(
        typ.contains("inset: (top: 10.5pt, right: 4pt, bottom: 9pt, left: 4pt)"),
        "表头：{typ}"
    );
    assert!(
        typ.contains("inset: (top: 14pt, right: 4pt, bottom: 6pt, left: 4pt)"),
        "格子自己的段前 10、段后 2：{typ}"
    );
    assert!(
        typ.contains("inset: (top: 4pt, right: 4pt, bottom: 9pt, left: 4pt)"),
        "普通格子只有段后：{typ}"
    );
}

#[test]
fn pdf_spacing_stacks_on_part_padding_and_inner_lines() {
    // 部位 paddingTop 2 + 段前 7；3pt 的行间线先吃上边距（2pt），多的 1pt 加进去，不吃段前
    let json = table_doc(
        r#", "style": { "border": "3pt solid #000", "cellPadding": 8 }, "parts": { "cell": { "paddingTop": 2, "spaceBefore": 7 } }"#,
    )
    .replace(r#""style": { "spaceBefore": 10, "spaceAfter": 2 }"#, r#""style": {}"#);
    let [_, typ, _] = outputs(&json);
    assert!(
        typ.contains("inset: (top: 10pt, right: 8pt, bottom: 8pt, left: 8pt)"),
        "中间一行：2 + 1 + 7：{typ}"
    );
}

#[test]
fn html_puts_cell_spacing_into_padding() {
    let json = table_doc(r#", "parts": { "cell": { "spaceBefore": 6, "spaceAfter": "1em" } }"#);
    let [_, _, h] = outputs(&json);
    // td 上不再写浏览器不认的 margin
    for td in h.split("<td").skip(1) {
        let td = &td[..td.find('>').unwrap()];
        assert!(
            !td.contains("margin-top") && !td.contains("margin-bottom"),
            "{td}"
        );
    }
    assert!(
        h.contains("padding-top:10pt;padding-bottom:14.5pt;"),
        "段前 6、段后 1em（10.5pt）叠在 4pt 内边距上：{h}"
    );
    assert!(
        h.contains("padding-top:14pt;padding-bottom:6pt;"),
        "格子自己的段前 10、段后 2：{h}"
    );
}

#[test]
fn html_exact_row_keeps_its_height() {
    // 固定 30pt 的行：上下内边距 4 + 段前段后 5 → 里面 12pt 高
    let json = table_doc(r#", "parts": { "cell": { "spaceBefore": 5, "spaceAfter": 5 } }"#)
        .replace(
            r#""rows": [["#,
            r#""rowHeights": [null, { "height": 30, "rule": "exact" }], "rows": [["#,
        )
        .replace(
            r#""style": { "spaceBefore": 10, "spaceAfter": 2 }"#,
            r#""style": {}"#,
        );
    let [_, _, h] = outputs(&json);
    assert!(h.contains(r#"<div style="height:12pt">苹果</div>"#), "{h}");
    assert_eq!(h.matches(r#"<div style="height:12pt">"#).count(), 2, "{h}");
}

#[test]
fn line_breaks_inside_a_cell_get_no_spacing() {
    // 格子里的换行是段内换行（docx 的 w:br）：段前段后只在整格上下各一次
    let json = table_doc(r#", "parts": { "cell": { "spaceBefore": 6, "spaceAfter": 6 } }"#)
        .replace(r#""梨""#, r#""梨\n第二行""#);
    let [x, typ, _] = outputs(&json);
    assert!(
        x.contains(r#"<w:br w:type="textWrapping" />"#) || x.contains("<w:br"),
        "{x}"
    );
    let tbl = &x[x.find("<w:tbl>").unwrap()..x.find("</w:tbl>").unwrap()];
    assert_eq!(
        tbl.matches("<w:p ").count() + tbl.matches("<w:p>").count(),
        6,
        "一格一段：{tbl}"
    );
    assert!(
        typ.contains("inset: (top: 10pt, right: 4pt, bottom: 10pt, left: 4pt)"),
        "{typ}"
    );
}

#[test]
fn styles_example_total_row_spacing_shows_in_every_format() {
    // examples/styles.json 合计行 lastRow 写了 spaceBefore: 3：三种格式都要看得出来
    let json = include_str!("../examples/styles.json");
    let [x, typ, h] = outputs(json);
    assert!(x.contains(r#"w:before="60""#), "docx 合计行段前 3pt");
    let total = &h[h.rfind("<tr>").unwrap()..];
    assert!(
        total.contains("合计") && total.contains("padding-top:7pt;"),
        "{total}"
    );
    let i = typ.find("#\"合计\"").unwrap();
    let cell = &typ[typ[..i].rfind("table.cell(").unwrap()..i];
    // 上内边距 4 + 1.5pt 双线比边距细、不加 + 段前 3
    assert!(cell.contains("inset: (top: 7pt,"), "{cell}");
}
