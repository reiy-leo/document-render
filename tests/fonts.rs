//! 字体检查（src/fonts.rs）：渲染 pdf、docx 前，文档用到的字体本机找不到就报错，不出文件；
//! `allow_missing_fonts`（命令行 --allow-missing-fonts）降为警告。html、md 不查；check 也列出来。

use std::process::Command;

use document_render::{Env, fonts, render::Format};

const MISSING: &str = "DR Missing Font Zz";

fn lenient() -> Env {
    Env {
        allow_missing_fonts: true,
        ..Env::default()
    }
}

/// 七段正文都用命名样式 fancy，fancy 的西文字体本机没有。
fn seven_paragraphs() -> String {
    let ids: Vec<String> = (1..=7).map(|i| format!("\"t{i}\"")).collect();
    let els: Vec<String> = (1..=7)
        .map(|i| format!(r#""t{i}": {{ "type": "Text", "props": {{ "text": "第 {i} 段 abc" }}, "style": "fancy" }}"#))
        .collect();
    format!(
        r#"{{
          "styles": {{ "fancy": {{ "fontFamily": {{ "latin": "{MISSING}" }} }} }},
          "root": "doc",
          "elements": {{
            "doc": {{ "type": "Document", "children": [{}] }},
            {}
          }}
        }}"#,
        ids.join(", "),
        els.join(",\n")
    )
}

#[test]
fn missing_font_is_an_error_for_pdf_and_docx() {
    let json = seven_paragraphs();
    for f in [Format::Pdf, Format::Docx] {
        let e = match document_render::render_str(&json, f, None) {
            Ok(_) => panic!("{f:?}：缺字体应该报错"),
            Err(e) => format!("{e:#}"),
        };
        assert!(e.contains(MISSING), "{e}");
        assert!(e.contains("`t1`") && e.contains("等 7 处"), "{e}");
        assert!(!e.contains("`t6`"), "id 多了要截断：{e}");
        assert!(e.contains("styles.fancy"), "要写出来源样式键：{e}");
        assert!(e.contains("--allow-missing-fonts"), "{e}");
    }
    // html、md 交给浏览器回退，不报错
    for f in [Format::Html, Format::Markdown] {
        document_render::render_str(&json, f, None).unwrap();
    }
}

#[test]
fn allow_missing_fonts_turns_it_into_a_warning() {
    let json = seven_paragraphs();
    for f in [Format::Pdf, Format::Docx] {
        let out = document_render::render_with(&json, f, None, &lenient()).unwrap();
        let w: Vec<&String> = out
            .warnings
            .iter()
            .filter(|w| w.contains(MISSING))
            .collect();
        assert_eq!(w.len(), 1, "{f:?}：{:?}", out.warnings);
    }
}

fn fallback_doc(list: &str) -> String {
    format!(
        r#"{{ "root": "doc", "elements": {{
          "doc": {{ "type": "Document", "children": ["a"] }},
          "a": {{ "type": "Text", "props": {{ "text": "abc" }}, "style": {{ "fontFamily": "{list}" }} }}
        }} }}"#
    )
}

#[test]
fn fallback_list_passes_when_one_name_exists() {
    // Libertinus Serif 是 Typst 自带的，哪台机器都有；列表里另一个没有也不报错，也不给警告
    for list in [
        format!("Libertinus Serif, {MISSING}"),
        format!("{MISSING}, Libertinus Serif"),
    ] {
        let json = fallback_doc(&list);
        for f in [Format::Pdf, Format::Docx] {
            let out = document_render::render_str(&json, f, None)
                .unwrap_or_else(|e| panic!("{list} {f:?}：{e:#}"));
            assert!(
                !out.warnings.iter().any(|w| w.contains(MISSING)),
                "{list} {f:?}：{:?}",
                out.warnings
            );
        }
        let (d, tree) = document_render::prepare(&json, &Env::default()).unwrap();
        assert!(document_render::check_fonts(&d, &tree).is_empty(), "{list}");
    }
}

#[test]
fn fallback_list_fails_when_no_name_exists() {
    let other = "DR Missing Font Yy";
    let json = fallback_doc(&format!("{MISSING},{other}"));
    let e = format!(
        "{:#}",
        document_render::render_str(&json, Format::Pdf, None)
            .err()
            .unwrap()
    );
    // 整条列表一行，带元素 id 和样式键
    assert!(e.contains("找不到 1 个字体"), "{e}");
    assert!(
        e.contains(&format!("`{MISSING}, {other}`（列表里一个都没有）")),
        "{e}"
    );
    assert!(e.contains("`a`") && e.contains("元素 a 的 style"), "{e}");
    // check 一样
    let (d, tree) = document_render::prepare(&json, &Env::default()).unwrap();
    let msgs = document_render::check_fonts(&d, &tree);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(
        msgs[0].contains(other) && msgs[0].contains("元素 a 的 style"),
        "{msgs:?}"
    );
    // 放行时一条警告
    let out = document_render::render_with(&json, Format::Docx, None, &lenient()).unwrap();
    let w: Vec<&String> = out
        .warnings
        .iter()
        .filter(|w| w.contains(MISSING))
        .collect();
    assert_eq!(w.len(), 1, "{:?}", out.warnings);
}

#[test]
fn marker_font_and_style_set_sources() {
    let json = format!(
        r#"{{
          "styleSet": "brand",
          "styleSets": {{ "brand": {{ "global": {{ "$body": "{MISSING}", "fontFamily": "${{body}}" }} }} }},
          "root": "doc",
          "elements": {{
            "doc": {{ "type": "Document", "children": ["l"] }},
            "l": {{ "type": "List", "props": {{ "items": ["一", "二"] }}, "style": {{ "listMarkerFont": "DR Missing Marker" }} }}
          }}
        }}"#
    );
    let (d, tree) = document_render::prepare(&json, &Env::default()).unwrap();
    let msgs = document_render::check_fonts(&d, &tree);
    assert_eq!(msgs.len(), 2, "{msgs:?}");
    assert!(
        msgs[0].contains(MISSING) && msgs[0].contains("styleSets.brand.global"),
        "{msgs:?}"
    );
    assert!(
        msgs[1].contains("DR Missing Marker")
            && msgs[1].contains("`l`")
            && msgs[1].contains("元素 l 的 style"),
        "{msgs:?}"
    );
}

#[test]
fn default_lists_fail_only_when_none_is_installed() {
    let json = r#"{ "root": "doc", "elements": {
      "doc": { "type": "Document", "children": ["a", "c"] },
      "a": { "type": "Text", "props": { "text": "中文 abc" } },
      "c": { "type": "Code", "props": { "code": "let x = 1;" } }
    } }"#;
    let (d, tree) = document_render::prepare(json, &Env::default()).unwrap();
    let names = |has: &dyn Fn(&str) -> bool| -> Vec<String> {
        fonts::missing_with(&d, &tree, has)
            .iter()
            .map(|m| m.describe())
            .collect()
    };
    // 一个字体都没有：西文、中文、等宽三组都报，带上用到的元素
    let all = names(&|_| false);
    assert_eq!(all.len(), 3, "{all:?}");
    assert!(
        all[0].starts_with("默认西文字体") && all[0].contains("`a`"),
        "{all:?}"
    );
    assert!(
        all[1].starts_with("默认中文字体") && all[1].contains("`a`"),
        "{all:?}"
    );
    assert!(
        all[2].starts_with("默认等宽字体") && all[2].contains("`c`"),
        "{all:?}"
    );
    // 每组有一个就行
    let some = names(&|n| matches!(n, "Liberation Sans" | "Noto Sans CJK SC" | "Consolas"));
    assert!(some.is_empty(), "{some:?}");
    // 只缺中文
    let no_cjk = names(&|n| matches!(n, "Arial" | "Menlo"));
    assert_eq!(no_cjk.len(), 1, "{no_cjk:?}");
    assert!(no_cjk[0].contains("PingFang SC"), "{no_cjk:?}");
}

#[test]
fn cli_check_and_render() {
    let dir = std::env::temp_dir().join("document-render-fonts-cli");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let doc = dir.join("doc.json");
    std::fs::write(&doc, seven_paragraphs()).unwrap();
    let bin = env!("CARGO_BIN_EXE_document-render");

    let out = Command::new(bin).arg("check").arg(&doc).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "{err}");
    assert!(
        err.contains(MISSING) && err.contains("--allow-missing-fonts"),
        "{err}"
    );

    let out = Command::new(bin)
        .args(["check", "--allow-missing-fonts"])
        .arg(&doc)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{err}");
    assert!(err.contains("警告") && err.contains(MISSING), "{err}");

    let pdf = dir.join("doc.pdf");
    let out = Command::new(bin)
        .arg("render")
        .arg(&doc)
        .arg("-o")
        .arg(&pdf)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!pdf.exists());
    let out = Command::new(bin)
        .args(["render", "--allow-missing-fonts"])
        .arg(&doc)
        .arg("-o")
        .arg(&pdf)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(pdf.exists());
}
