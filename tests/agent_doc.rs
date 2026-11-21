//! docs/AGENT.md 是给 AI agent 的写文档指南，要跟代码对得上：
//! - catalog 里每个内置组件名、每个组件的每个 prop 名和部位、每个样式字段都要在文档里出现，
//!   加了组件、prop 或样式字段忘了写文档这里会挂；
//! - 紧跟在 `<!-- check -->` 注释下一行的 ```json 代码块是完整文档，要能通过 check，
//!   没有公式警告，而且每个元素都挂在树上（children、页眉页脚或列表项的 children 引用到）。

use std::collections::BTreeSet;

use serde_json::Value;

const GUIDE: &str = include_str!("../docs/AGENT.md");
const MARK: &str = "<!-- check -->";

/// 标了 `<!-- check -->` 的代码块，带它在文档里的行号（从 1 起）。
fn checked_blocks() -> Vec<(usize, String)> {
    let lines: Vec<&str> = GUIDE.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if l.trim() != MARK {
            continue;
        }
        assert_eq!(
            lines.get(i + 1).map(|x| x.trim()),
            Some("```json"),
            "第 {} 行的 {MARK} 下一行要是 ```json",
            i + 1
        );
        let body: Vec<&str> = lines[i + 2..]
            .iter()
            .take_while(|x| x.trim() != "```")
            .copied()
            .collect();
        assert!(
            i + 2 + body.len() < lines.len(),
            "第 {} 行的代码块没有结束的 ```",
            i + 1
        );
        out.push((i + 2, body.join("\n")));
    }
    out
}

/// 列表项对象 `children` 里引用的元素 id（含子列表的项）。
fn item_children(items: &Value, out: &mut Vec<String>) {
    for it in items.as_array().into_iter().flatten() {
        if let Some(ch) = it["children"].as_array() {
            out.extend(ch.iter().filter_map(|x| x.as_str().map(String::from)));
        }
        item_children(&it["items"], out);
    }
}

/// 从 root 出发能走到的元素 id，加上页眉页脚、列表项 children 引用的。
fn reachable(doc: &Value) -> BTreeSet<String> {
    let els = &doc["elements"];
    let mut seen = BTreeSet::new();
    let mut stack: Vec<String> = vec![doc["root"].as_str().unwrap().to_string()];
    let hf = |v: &Value, stack: &mut Vec<String>| match v {
        Value::String(s) => stack.push(s.clone()),
        Value::Object(m) => stack.extend(m.values().filter_map(|x| x.as_str().map(String::from))),
        _ => {}
    };
    let page = &doc["meta"]["page"];
    hf(&page["header"], &mut stack);
    hf(&page["footer"], &mut stack);
    while let Some(id) = stack.pop() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let el = &els[&id];
        if let Some(ch) = el["children"].as_array() {
            stack.extend(ch.iter().filter_map(|x| x.as_str().map(String::from)));
        }
        if el["type"] == "List" {
            item_children(&el["props"]["items"], &mut stack);
        }
        if el["type"] == "Section" {
            hf(&el["props"]["header"], &mut stack);
            hf(&el["props"]["footer"], &mut stack);
        }
    }
    seen
}

#[test]
fn guide_mentions_every_component_and_prop() {
    let cat = document_render::components_json(&Default::default());
    let mut missing = Vec::new();
    for (name, spec) in cat.as_object().unwrap() {
        if !GUIDE.contains(&format!("`{name}`")) && !GUIDE.contains(&format!("| {name} |")) {
            missing.push(format!("组件 {name}"));
        }
        if let Some(props) = spec["props"]["properties"].as_object() {
            for p in props.keys() {
                if !GUIDE.contains(&format!("`{p}`")) {
                    missing.push(format!("{name}.{p}"));
                }
            }
        }
        if let Some(parts) = spec["parts"].as_array() {
            for p in parts {
                let p = p.as_str().unwrap();
                if !GUIDE.contains(&format!("`{p}`")) {
                    missing.push(format!("{name} 的部位 {p}"));
                }
            }
        }
    }
    assert!(
        missing.is_empty(),
        "docs/AGENT.md 没写到这些（加组件或 prop 时要同步改指南）：{missing:?}"
    );
}

#[test]
fn guide_mentions_every_style_field() {
    let schema = schemars::schema_for!(document_render::style::Style);
    let fields = schema
        .as_value()
        .get("properties")
        .and_then(Value::as_object)
        .expect("Style 的 schema 应该有 properties");
    let missing: Vec<_> = fields
        .keys()
        .filter(|f| !GUIDE.contains(&format!("`{f}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/AGENT.md 第 4 节没写到这些样式字段（加样式字段时要同步改指南）：{missing:?}"
    );
}

#[test]
fn guide_has_a_section_per_component() {
    // 第 3 节每个组件一个小节，标题写成 `### 3.x 组件名`，几个组件合一节时用 / 分开
    let heads: Vec<&str> = GUIDE.lines().filter(|l| l.starts_with("### 3.")).collect();
    let cat = document_render::components_json(&Default::default());
    for name in cat.as_object().unwrap().keys() {
        assert!(
            heads.iter().any(|h| h
                .split(|c: char| c.is_whitespace() || c == '/')
                .any(|w| w == name)),
            "docs/AGENT.md 第 3 节没有 {name} 的小节（标题写成 `### 3.x {name}`）"
        );
    }
}

#[test]
fn guide_examples_pass_check() {
    let blocks = checked_blocks();
    assert!(
        blocks.len() >= 3,
        "标了 {MARK} 的完整示例太少：{}",
        blocks.len()
    );
    for (line, src) in blocks {
        let doc: Value = serde_json::from_str(&src)
            .unwrap_or_else(|e| panic!("第 {line} 行的示例不是合法 JSON：{e}"));
        let (_, tree) = document_render::prepare(&src, &document_render::Env::default())
            .unwrap_or_else(|e| panic!("第 {line} 行的示例没通过 check：{e:#}"));
        let warnings = document_render::check_math(&tree);
        assert!(
            warnings.is_empty(),
            "第 {line} 行的示例有警告：{warnings:?}"
        );
        let seen = reachable(&doc);
        let orphans: Vec<_> = doc["elements"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|k| !seen.contains(*k))
            .collect();
        assert!(
            orphans.is_empty(),
            "第 {line} 行的示例有没被引用的元素：{orphans:?}"
        );
    }
}

#[test]
fn guide_full_example_renders() {
    // 最大的那个完整示例（第 6 节）要能出 html 和 md；docx、pdf 在 tests/ 以外手动验过，太慢不放这里
    let (_, src) = checked_blocks()
        .into_iter()
        .max_by_key(|(_, s)| s.len())
        .unwrap();
    for f in [
        document_render::render::Format::Html,
        document_render::render::Format::Markdown,
    ] {
        let out = document_render::render_str(&src, f, Some(std::path::Path::new("docs")))
            .unwrap_or_else(|e| panic!("完整示例渲染失败：{e:#}"));
        assert!(!out.bytes.is_empty());
    }
}

#[test]
fn guide_is_what_the_binary_serves() {
    assert_eq!(document_render::AGENT_GUIDE, GUIDE);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_document-render"))
        .arg("guide")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), GUIDE);
}
