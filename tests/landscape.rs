//! 页面方向：orientation（portrait / landscape），纵横混排，四种格式、check、parse 往返。

use std::io::{Read, Write};

use document_render::render::Format;
use serde_json::Value;

const EXAMPLE: &str = include_str!("../examples/landscape.json");

/// A4 纵向的宽高，缇
const A4_W: &str = "11906";
const A4_H: &str = "16838";

fn render(json: &str, f: Format) -> Vec<u8> {
    let out = document_render::render_str(json, f, None).unwrap();
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    out.bytes
}

fn text(json: &str, f: Format) -> String {
    String::from_utf8_lossy(&render(json, f)).into_owned()
}

fn err(json: &str) -> String {
    match document_render::render_str(json, Format::Html, None) {
        Ok(_) => panic!("应该报错"),
        Err(e) => format!("{e:#}"),
    }
}

fn document_xml(docx: &[u8]) -> String {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
    let mut s = String::new();
    z.by_name("word/document.xml")
        .unwrap()
        .read_to_string(&mut s)
        .unwrap();
    s
}

/// 换掉 docx 里的 word/document.xml，别的原样拷过去。
fn with_document_xml(docx: &[u8], xml: &str) -> Vec<u8> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opt = zip::write::SimpleFileOptions::default();
    for i in 0..z.len() {
        let mut f = z.by_index(i).unwrap();
        let name = f.name().to_string();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        out.start_file(&name, opt).unwrap();
        if name == "word/document.xml" {
            out.write_all(xml.as_bytes()).unwrap();
        } else {
            out.write_all(&buf).unwrap();
        }
    }
    out.finish().unwrap().into_inner()
}

/// 每个 w:pgSz 的 (w, h, orient)，按文档顺序。
fn pg_sizes(xml: &str) -> Vec<(String, String, String)> {
    let attr = |tag: &str, a: &str| -> String {
        let k = format!("w:{a}=\"");
        tag.find(&k)
            .map(|i| {
                let rest = &tag[i + k.len()..];
                rest[..rest.find('"').unwrap()].to_string()
            })
            .unwrap_or_default()
    };
    xml.match_indices("<w:pgSz ")
        .map(|(i, _)| {
            let tag = &xml[i..i + xml[i..].find("/>").unwrap()];
            (attr(tag, "w"), attr(tag, "h"), attr(tag, "orient"))
        })
        .collect()
}

/// pdf 每页的 MediaBox 宽高，pt，按页的顺序。
fn pdf_pages(pdf: &[u8]) -> Vec<(f32, f32)> {
    let s = String::from_utf8_lossy(pdf);
    s.match_indices("/MediaBox")
        .map(|(i, _)| {
            let rest = &s[i..];
            let a = rest.find('[').unwrap();
            let b = rest.find(']').unwrap();
            let n: Vec<f32> = rest[a + 1..b]
                .split_whitespace()
                .map(|x| x.parse().unwrap())
                .collect();
            (n[2] - n[0], n[3] - n[1])
        })
        .collect()
}

fn section_json(props: &str) -> String {
    format!(
        r#"{{"root":"d","elements":{{"d":{{"type":"Document","children":["a","b"]}},
        "a":{{"type":"Section","children":["t1"]}},
        "b":{{"type":"Section","props":{props},"children":["t2"]}},
        "t1":{{"type":"Text","props":{{"text":"一"}}}},
        "t2":{{"type":"Text","props":{{"text":"二"}}}}}}}}"#
    )
}

#[test]
fn docx_each_section_has_own_pgsz() {
    let xml = document_xml(&render(EXAMPLE, Format::Docx));
    assert_eq!(
        pg_sizes(&xml),
        vec![
            (A4_W.into(), A4_H.into(), "portrait".into()),
            (A4_H.into(), A4_W.into(), "landscape".into()),
            (A4_W.into(), A4_H.into(), "portrait".into()),
        ]
    );
    // 横向那一节的页边距是 15mm（850 缇），按横过来的纸写
    assert!(xml.contains(r#"w:top="850""#), "{xml}");
    // 三节都挂着页眉页脚
    assert_eq!(xml.matches("<w:sectPr><w:headerReference").count(), 3);
}

#[test]
fn pdf_pages_follow_sections() {
    let pages = pdf_pages(&render(EXAMPLE, Format::Pdf));
    assert_eq!(pages.len(), 3, "{pages:?}");
    let portrait = |p: (f32, f32)| (p.0 - 595.28).abs() < 0.1 && (p.1 - 841.89).abs() < 0.1;
    let landscape = |p: (f32, f32)| (p.0 - 841.89).abs() < 0.1 && (p.1 - 595.28).abs() < 0.1;
    assert!(portrait(pages[0]), "{pages:?}");
    assert!(landscape(pages[1]), "{pages:?}");
    assert!(portrait(pages[2]), "{pages:?}");
}

#[test]
fn html_named_pages_and_section_width() {
    let x = text(EXAMPLE, Format::Html);
    assert!(x.contains("@page document-render-s0 { size: 595.2756pt 841.8898pt;"));
    assert!(x.contains("@page document-render-s1 { size: 841.8898pt 595.2756pt;"));
    assert!(
        x.contains(".document-render-section[data-section=\"1\"] { page: document-render-s1; break-before: page; }")
    );
    // 直接打开：横向节按自己的版心限宽（841.89 − 2×15mm），正文限宽取最宽那节
    let num_after = |key: &str| -> f32 {
        let i = x.find(key).unwrap_or_else(|| panic!("没有 {key}")) + key.len();
        let rest = &x[i..];
        rest[..rest.find("pt").unwrap()].parse().unwrap()
    };
    let s0 = num_after(".document-render-section[data-section=\"0\"] { max-width: ");
    let s1 = num_after(".document-render-section[data-section=\"1\"] { max-width: ");
    assert!((s0 - (595.2756 - 2.0 * 56.6929)).abs() < 0.01, "{s0}");
    assert!((s1 - (841.8898 - 2.0 * 42.5197)).abs() < 0.01, "{s1}");
    let body = num_after("color: #1a1a1a; max-width: ");
    assert!((body - s1).abs() < 0.001, "{body}");
    // 不加 --paged 时没有打印用的具名页面
    assert!(!x.contains("document-render-print-s"));
}

#[test]
fn paged_html_prints_each_section_on_its_paper() {
    let (d, tree) = document_render::prepare(EXAMPLE, &Default::default()).unwrap();
    let opts = document_render::render::Options {
        paged: true,
        ..Default::default()
    };
    let out = document_render::render::render(&d, &tree, Format::Html, &opts).unwrap();
    let x = String::from_utf8_lossy(&out.bytes);
    assert!(x.contains(
        "@page document-render-print-s1 { size: 841.8898pt 595.2756pt !important; margin: 0; }"
    ));
    assert!(x.contains(".pagedjs_document-render-s1_page { page: document-render-print-s1; }"));
    // 改名时把 JS 函数名换成了带减号的 document-renderNum，整段脚本语法错误，页眉页脚出不来
    assert!(x.contains("function documentRenderNum("));
    assert!(!x.contains("document-renderNum"));
    // 屏幕上不另加节宽，由 Paged.js 的页面决定
    assert!(!x.contains("margin-inline: auto"));
}

#[test]
fn markdown_ignores_orientation() {
    let out = document_render::render_str(EXAMPLE, Format::Markdown, None).unwrap();
    // 只有页眉页脚那条警告，方向不报
    assert!(
        out.warnings.iter().all(|w| w.contains("页眉页脚")),
        "{:?}",
        out.warnings
    );
    let md = String::from_utf8(out.bytes).unwrap();
    assert!(md.contains("各区域月度销量（横向）"));
    assert!(!md.contains("landscape"));
}

#[test]
fn orientation_is_inherited_and_switched_back() {
    // 中间一节横向，第三节没写方向，沿用横向
    let json = r#"{"root":"d","elements":{"d":{"type":"Document","children":["a","b","c"]},
        "a":{"type":"Section","children":["t"]},
        "b":{"type":"Section","props":{"orientation":"landscape"},"children":["u"]},
        "c":{"type":"Section","props":{"size":"A5"},"children":["v"]},
        "t":{"type":"Text","props":{"text":"一"}},"u":{"type":"Text","props":{"text":"二"}},"v":{"type":"Text","props":{"text":"三"}}}}"#;
    let xml = document_xml(&render(json, Format::Docx));
    let s = pg_sizes(&xml);
    assert_eq!(s[1].2, "landscape");
    // A5 横向：宽 210mm，高 148mm
    assert_eq!(s[2], ("11906".into(), "8391".into(), "landscape".into()));
}

#[test]
fn custom_size_is_turned_by_orientation() {
    // 自定义尺寸写成横的，又写了 portrait：按纵向摆，长边做高
    let json = r#"{"meta":{"page":{"size":{"width":"297mm","height":"210mm"},"orientation":"portrait"}},
        "root":"d","elements":{"d":{"type":"Document","children":["t"]},"t":{"type":"Text","props":{"text":"一"}}}}"#;
    let s = pg_sizes(&document_xml(&render(json, Format::Docx)));
    assert_eq!(s[0], (A4_W.into(), A4_H.into(), "portrait".into()));
    // 没写方向：照 size 写的宽高，宽大于高就写成横向
    let json = json.replace(r#","orientation":"portrait""#, "");
    let s = pg_sizes(&document_xml(&render(&json, Format::Docx)));
    assert_eq!(s[0], (A4_H.into(), A4_W.into(), "landscape".into()));
    // 横向写在 meta.page 上
    let json = json.replace(
        r#""size":{"width":"297mm","height":"210mm"}"#,
        r#""size":"A4","orientation":"landscape""#,
    );
    let s = pg_sizes(&document_xml(&render(&json, Format::Docx)));
    assert_eq!(s[0], (A4_H.into(), A4_W.into(), "landscape".into()));
}

#[test]
fn legacy_landscape_flag_still_works() {
    let xml = document_xml(&render(
        &section_json(r#"{"landscape":true}"#),
        Format::Docx,
    ));
    assert_eq!(pg_sizes(&xml)[1].2, "landscape");
    // 写法一致时两个一起写也行
    let xml = document_xml(&render(
        &section_json(r#"{"landscape":true,"orientation":"landscape"}"#),
        Format::Docx,
    ));
    assert_eq!(pg_sizes(&xml)[1].2, "landscape");
}

#[test]
fn check_rejects_bad_orientation() {
    let e = err(&section_json(r#"{"orientation":"sideways"}"#));
    assert!(
        e.contains("元素 `b`") && e.contains("orientation 只认") && e.contains("sideways"),
        "{e}"
    );
    let e = err(&section_json(r#"{"orientation":true}"#));
    assert!(e.contains("orientation 只认") && e.contains("true"), "{e}");
    let e = err(&section_json(
        r#"{"orientation":"portrait","landscape":true}"#,
    ));
    assert!(e.contains("元素 `b`") && e.contains("对不上"), "{e}");
    let e = err(
        r#"{"meta":{"page":{"orientation":"Landscape"}},"root":"d","elements":{"d":{"type":"Document","children":[]}}}"#,
    );
    assert!(
        e.contains("orientation 只认") && e.contains("Landscape"),
        "{e}"
    );
    let e = err(
        r#"{"meta":{"page":{"orientation":"landscape","landscape":false}},"root":"d","elements":{"d":{"type":"Document","children":[]}}}"#,
    );
    assert!(e.contains("meta.page") && e.contains("对不上"), "{e}");
}

fn parse(bytes: &[u8]) -> Value {
    document_render::parse::docx(
        bytes,
        &document_render::parse::Options {
            assets_dir: "x.assets".into(),
        },
    )
    .unwrap()
    .json
}

fn sections(json: &Value) -> Vec<Value> {
    let els = json["elements"].as_object().unwrap();
    let root = &els[json["root"].as_str().unwrap()];
    root["children"]
        .as_array()
        .unwrap()
        .iter()
        .map(|id| els[id.as_str().unwrap()].clone())
        .filter(|e| e["type"] == "Section")
        .collect()
}

#[test]
fn parse_round_trip_keeps_orientation() {
    let docx = render(EXAMPLE, Format::Docx);
    let back = parse(&docx);
    assert!(back["meta"]["page"].get("orientation").is_none());
    assert_eq!(back["meta"]["page"]["size"], "A4");
    let secs = sections(&back);
    assert_eq!(secs.len(), 2, "{back}");
    assert_eq!(secs[0]["props"]["orientation"], "landscape");
    assert!(secs[0]["props"].get("size").is_none());
    assert_eq!(secs[1]["props"]["orientation"], "portrait");
    assert!(!back.to_string().contains("\"landscape\":"));
    // 读回来的 JSON 再出 docx，每节纸张一样
    let again = render(&back.to_string(), Format::Docx);
    assert_eq!(
        pg_sizes(&document_xml(&again)),
        pg_sizes(&document_xml(&docx))
    );
}

#[test]
fn parse_reads_orientation_from_width_and_height() {
    let docx = render(EXAMPLE, Format::Docx);
    let xml = document_xml(&docx);
    // 横向那节去掉 w:orient，只剩宽大于高：仍读成横向
    let no_orient = xml.replace(r#" w:orient="landscape""#, "");
    let back = parse(&with_document_xml(&docx, &no_orient));
    assert_eq!(sections(&back)[0]["props"]["orientation"], "landscape");
    // 第三节写 orient="landscape" 但高大于宽：Word 按宽高排，读成纵向
    let mut n = 0;
    let odd = xml
        .split(r#"w:orient="portrait""#)
        .enumerate()
        .map(|(i, part)| {
            n += 1;
            if i == 0 {
                part.to_string()
            } else if i == 2 {
                format!("w:orient=\"landscape\"{part}")
            } else {
                format!("w:orient=\"portrait\"{part}")
            }
        })
        .collect::<String>();
    assert_eq!(n, 3, "两个纵向节");
    let back = parse(&with_document_xml(&docx, &odd));
    assert_eq!(sections(&back)[1]["props"]["orientation"], "portrait");
}
