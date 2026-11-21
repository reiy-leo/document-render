use std::path::Path;

use document_render::render::Format;

fn has(bytes: &[u8], needle: &str) -> bool {
    bytes.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn showcase_renders_to_docx() {
    let out = document_render::render_str(
        include_str!("../examples/showcase.json"),
        Format::Docx,
        Some(Path::new("examples")),
    )
    .unwrap();
    assert!(out.bytes.starts_with(b"PK"));
    assert!(has(&out.bytes, "word/document.xml"));
    // logo.svg 放不进 docx，应该给警告而不是报错
    assert!(out.warnings.iter().any(|w| w.contains("logo")));
}

#[test]
fn png_image_is_embedded() {
    let dir = std::env::temp_dir().join("document-render-docx-test");
    std::fs::create_dir_all(&dir).unwrap();
    image::RgbImage::from_pixel(4, 2, image::Rgb([200, 30, 30]))
        .save(dir.join("dot.png"))
        .unwrap();
    let json = r#"{"root":"d","elements":{"d":{"type":"Document","children":["i"]},"i":{"type":"Image","props":{"src":"dot.png","width":40}}}}"#;
    let out = document_render::render_str(json, Format::Docx, Some(&dir)).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    assert!(has(&out.bytes, "word/media/"));
}

/// Word 实机里看到的两个问题：合计行的上边框画了两条、二级编号出成「二.1」。
#[test]
fn styles_example_borders_and_legal_numbering() {
    // 样例写的是 Times New Roman、宋体这类字体，本机不一定都有
    let env = document_render::Env {
        allow_missing_fonts: true,
        ..Default::default()
    };
    let out = document_render::render_with(
        include_str!("../examples/styles.json"),
        Format::Docx,
        Some(Path::new("examples")),
        &env,
    )
    .unwrap();
    let b = &out.bytes;
    // 格子之间的线只写在上面一格的下边，格子里的段落不带边框
    assert!(has(b, r#"<w:bottom w:val="double""#));
    assert!(!has(b, r#"<w:top w:val="double""#));
    // 二级是 decimal，上级编号用阿拉伯数字
    assert!(has(b, "<w:isLgl"));
}

/// 表头默认跨页重复：docx-rs 不支持 w:tblHeader，渲染后补进 XML。
#[test]
fn table_header_repeats_and_page_break() {
    let json = include_str!("../examples/pages.json");
    let out = document_render::render_str(json, Format::Docx, None).unwrap();
    assert!(has(&out.bytes, "<w:trPr><w:tblHeader /></w:trPr>"));
    assert!(has(&out.bytes, r#"<w:br w:type="page""#));
    // repeatHeader: false 时不写
    let off = json.replacen(
        r#""type": "Table","#,
        r#""type": "Table", "style": { "repeatHeader": false },"#,
        1,
    );
    let out = document_render::render_str(&off, Format::Docx, None).unwrap();
    assert!(!has(&out.bytes, "<w:tblHeader"));
}

/// 倍数行距按 CSS 算（字号 × 倍数），docx 写成「至少」，不用 Word 的 auto。
#[test]
fn line_height_multiple_is_at_least_font_size_times() {
    let json = r#"{
      "version": "0.1",
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["a", "b"] },
        "a": { "type": "Text", "props": { "text": "倍数" }, "style": { "fontSize": 10, "lineHeight": 1.5 } },
        "b": { "type": "Text", "props": { "text": "固定" }, "style": { "fontSize": 10, "lineHeight": "20pt" } }
      }
    }"#;
    let out = document_render::render_str(json, Format::Docx, None).unwrap();
    let b = &out.bytes;
    assert!(has(b, r#"w:line="300" w:lineRule="atLeast""#));
    assert!(has(b, r#"w:line="400" w:lineRule="exact""#));
    assert!(!has(b, r#"w:lineRule="auto""#));
}

/// 固定列 + auto 列：表格不撑满，auto 列写 tcW auto、表格 autofit；部位 padding 落成 tcMar。
#[test]
fn auto_columns_and_part_padding() {
    let json = r##"{
      "version": "0.1",
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["t"] },
        "t": {
          "type": "Table",
          "props": { "rows": [["", "基本信息"]], "widths": ["5.65pt", "auto"], "look": { "firstRow": false, "firstColumn": true } },
          "style": { "fontSize": 10 },
          "parts": { "lastColumn": { "paddingLeft": 12, "paddingRight": 18 } }
        }
      }
    }"##;
    let json = json.replace(
        r#""firstColumn": true }"#,
        r#""firstColumn": true, "lastColumn": true }"#,
    );
    let out = document_render::render_str(&json, Format::Docx, None).unwrap();
    let b = &out.bytes;
    // 1460 = 4 个全角字 × 10pt + 左右 padding 12 + 18 + 3pt 余量
    assert!(has(
        b,
        r#"<w:gridCol w:w="113" w:type="dxa" /><w:gridCol w:w="1460""#
    ));
    assert!(has(b, r#"<w:tcW w:w="0" w:type="auto""#));
    assert!(has(b, r#"<w:tblLayout w:type="autofit""#));
    assert!(has(b, r#"<w:left w:w="240" w:type="dxa""#));
    assert!(has(b, r#"<w:right w:w="360" w:type="dxa""#));
    assert!(
        has(b, r#"<w:tblW w:w="0" w:type="auto""#),
        "auto 表格不该撑满版心"
    );
}

/// em 缩进同时写 Word 的字符单位（leftChars、rightChars、firstLineChars、hangingChars）。
#[test]
fn em_indent_writes_chars() {
    let json = r##"{
      "root": "d",
      "elements": {
        "d": { "type": "Document", "children": ["a", "b", "c"] },
        "a": { "type": "Text", "props": { "text": "a" },
               "style": { "fontSize": 12, "marginLeft": "2em", "marginRight": "1em", "textIndent": "2em" } },
        "b": { "type": "Text", "props": { "text": "b" },
               "style": { "fontSize": 12, "marginLeft": "1em", "hangingIndent": "2em" } },
        "c": { "type": "Text", "props": { "text": "c" },
               "style": { "fontSize": 12, "marginLeft": "20pt", "marginRight": 10 } }
      }
    }"##;
    let out = document_render::render_str(json, Format::Docx, None).unwrap();
    let b = &out.bytes;
    assert!(has(
        b,
        r#"<w:ind w:left="480" w:right="240" w:firstLine="480" w:leftChars="200" w:rightChars="100" w:firstLineChars="200" />"#
    ));
    assert!(has(
        b,
        r#"<w:ind w:left="720" w:right="0" w:hanging="480" w:leftChars="300" w:hangingChars="200" />"#
    ));
    assert!(has(b, r#"<w:ind w:left="400" w:right="200" />"#));
    assert!(!has(b, "w:right=\"100000"));
}

#[test]
fn word_border_styles_map_through() {
    let json = include_str!("../examples/borders.json");
    let out =
        document_render::render_str(json, document_render::render::Format::Docx, None).unwrap();
    for v in [
        "thinThickThinLargeGap",
        "doubleWave",
        "threeDEngrave",
        "dashDotStroked",
        "dotDotDash",
    ] {
        assert!(has(&out.bytes, &format!("w:val=\"{v}\"")), "缺 {v}");
    }
}

/// `{ "multiple": x }` 是 Word 的多倍行距：docx 原样写 line = x × 240、lineRule auto；
/// html、pdf 按 Windows Word 的规则（metrics）换算。数字 lineHeight 还是写成 atLeast。
#[test]
fn line_height_font_multiple_is_word_auto() {
    use document_render::render::metrics;
    let json = r#"{
      "version": "0.1",
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["a", "b"] },
        "a": { "type": "Text", "props": { "text": "多倍" }, "style": { "fontSize": 10, "lineHeight": { "multiple": 1.5 } } },
        "b": { "type": "Text", "props": { "text": "倍数" }, "style": { "fontSize": 10, "lineHeight": 1.5 } }
      }
    }"#;
    let out = document_render::render_str(json, Format::Docx, None).unwrap();
    let b = &out.bytes;
    assert!(has(b, r#"w:line="360" w:lineRule="auto""#));
    assert!(has(b, r#"w:line="300" w:lineRule="atLeast""#));
    // html：行盒高度 ÷ 字号
    let s = document_render::style::Style {
        font_size: Some(document_render::style::Len::Pt(10.0)),
        ..Default::default()
    };
    let m = metrics::style_line(&s, true, None);
    let k = |l| metrics::line_box(&m, Some(l), 10.0).height(10.0) / 10.0;
    let multi = k(document_render::style::LineHeight::FontMultiple(1.5));
    let css = k(document_render::style::LineHeight::Multiple(1.5));
    assert!(multi > 1.5, "单倍行高应该比字号大：{multi}");
    assert!(css >= 1.5 - 1e-6);
    let h = document_render::render_str(json, Format::Html, None).unwrap();
    let h = String::from_utf8(h.bytes).unwrap();
    for want in [multi, css] {
        let want = format!("line-height:{};", (want * 1000.0).round() / 1000.0);
        assert!(h.contains(&want), "{want}");
    }
    // 写错了要报错
    let bad = json.replace(r#"{ "multiple": 1.5 }"#, r#"{ "multiple": 0 }"#);
    assert!(document_render::render_str(&bad, Format::Docx, None).is_err());
}

/// fontTable 给用到的字体写 charset：认得出的简中字体写 86，Word 才会乘 1.3。
#[test]
fn font_table_charset_for_cjk_fonts() {
    let json = r#"{
      "version": "0.1",
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["a"], "style": { "fontFamily": { "latin": "Calibri", "eastAsia": "Microsoft YaHei" } } },
        "a": { "type": "Text", "props": { "text": "中文 abc" }, "style": { "fontFamily": { "eastAsia": "SimSun" } } }
      }
    }"#;
    // Calibri、雅黑、宋体本机不一定有：照常出文件，看 fontTable
    let env = document_render::Env {
        allow_missing_fonts: true,
        ..Default::default()
    };
    let out = document_render::render_with(json, Format::Docx, None, &env).unwrap();
    let ft = unzip(&out.bytes, "word/fontTable.xml");
    for name in ["Microsoft YaHei", "SimSun"] {
        let want = format!(r#"<w:font w:name="{name}"><w:charset w:val="86" />"#);
        assert!(ft.contains(&want), "{name}：{ft}");
    }
    assert!(ft.contains(r#"<w:font w:name="Symbol"><w:charset w:val="02" />"#));
}

fn unzip(bytes: &[u8], name: &str) -> String {
    use std::io::Read;
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut f = z.by_name(name).unwrap();
    let mut s = String::new();
    f.read_to_string(&mut s).unwrap();
    s
}

/// 表格后面的空段落固定 1pt 高，段后扣掉这 1pt，表格后的间距才是 spaceAfter（和 pdf 一样）。
#[test]
fn table_gap_paragraph_counts_its_own_height() {
    let json = r#"{
      "root": "doc",
      "elements": {
        "doc": { "type": "Document", "children": ["t"] },
        "t": { "type": "Table", "props": { "rows": [["a"]] }, "style": { "spaceAfter": 8 } }
      }
    }"#;
    let out = document_render::render_str(json, Format::Docx, None).unwrap();
    assert!(has(
        &out.bytes,
        r#"w:after="140" w:line="20" w:lineRule="exact""#
    ));
}
