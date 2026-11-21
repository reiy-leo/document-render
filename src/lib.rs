//! document-render：一份 JSON，按组件目录渲染成 docx / pdf / html / Markdown。

pub mod assets;
pub mod cascade;
pub mod components;
pub mod custom;
pub mod doc;
pub mod math;
pub mod parse;
pub mod render;
pub mod style;
pub mod units;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

/// 给 AI agent 看的写文档指南（docs/AGENT.md），编进二进制。
/// 命令行 `document-render guide`、MCP 的 `guide` 工具和资源 [`AGENT_GUIDE_URI`] 都返回它。
pub const AGENT_GUIDE: &str = include_str!("../docs/AGENT.md");

/// MCP 资源里这份指南的 URI。
pub const AGENT_GUIDE_URI: &str = "document-render://agent-guide";

/// 文档以外的东西：自定义组件、样式集文件、命令行选的样式集。
#[derive(Debug, Default, Clone)]
pub struct Env {
    pub catalog: custom::Catalog,
    pub style_sets: BTreeMap<String, cascade::StyleSetDef>,
    /// 命令行 --style-set，优先于文档的 styleSet
    pub style_set: Option<String>,
}

/// 解析、展开自定义组件、算样式、校验，得到可以渲染的文档和元素树。
/// 文档自己 `components` 里写的组件会合进 `env.catalog`。
pub fn prepare(json: &str, env: &Env) -> Result<(doc::Doc, doc::Node)> {
    let mut d = doc::parse(json)?;
    let inline = std::mem::take(&mut d.components);
    let mut all = env.catalog.clone();
    for def in inline {
        let name = def.name.clone();
        all.add(def)
            .with_context(|| format!("文档里的组件定义 `{name}` 有问题"))?;
    }
    all.expand(&mut d)?;
    d.custom_names = all.defs().map(|x| x.name.clone()).collect();
    let sheet = cascade::Sheet::build(&d, &env.style_sets, env.style_set.as_deref())?;
    let tree = d.tree_with(&sheet)?;
    Ok((d, tree))
}

/// 检查文档里的公式（Math 组件和 runs 里的 InlineMath）能不能解析，返回警告。
/// 解析不了的公式渲染时会改成显示 LaTeX 原文，不算错误。
pub fn check_math(tree: &doc::Node) -> Vec<String> {
    fn walk(n: &doc::Node, out: &mut Vec<String>) {
        let mut c = n.component.clone();
        let mut all: Vec<(String, bool)> = Vec::new();
        if let components::Component::Math(p) = &c {
            all.push((p.latex.clone(), true));
        }
        for t in c.inlines_mut() {
            all.extend(
                t.runs()
                    .filter(|r| r.is_math())
                    .map(|r| (r.text.clone(), false)),
            );
        }
        for (latex, block) in all {
            if let Err(e) = math::to_mathml(&latex, block) {
                out.push(format!(
                    "元素 `{}` 里的公式 `{latex}` 解析不了（{e}），渲染时会显示 LaTeX 原文",
                    n.id
                ));
            }
        }
        for k in n.children.iter().chain(n.list_blocks()) {
            walk(k, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut out);
    out
}

/// 从 JSON 文本直接渲染，命令行和 MCP 都走这里。
/// `base_dir` 是文档所在目录，用来找相对路径的图片；没有就按当前目录找。
pub fn render_with(
    json: &str,
    format: render::Format,
    base_dir: Option<&Path>,
    env: &Env,
) -> Result<render::Output> {
    let (d, tree) = prepare(json, env)?;
    render::render(
        &d,
        &tree,
        format,
        &render::Options {
            base_dir: base_dir.map(Into::into),
            ..Default::default()
        },
    )
}

/// 不带外部组件和样式集的 [`render_with`]。
pub fn render_str(
    json: &str,
    format: render::Format,
    base_dir: Option<&Path>,
) -> Result<render::Output> {
    render_with(json, format, base_dir, &Env::default())
}

/// 读文档以外的定义：文档目录下的 `catalog/`、`styles/`（有的话），再读命令行指定的。
pub fn load_env(
    doc_dir: Option<&Path>,
    catalogs: &[PathBuf],
    style_sets: &[PathBuf],
    style_set: Option<String>,
) -> Result<Env> {
    let mut env = Env {
        style_set,
        ..Env::default()
    };
    if let Some(dir) = doc_dir {
        let c = dir.join("catalog");
        if c.is_dir() {
            env.catalog.load_path(&c)?;
        }
        let s = dir.join("styles");
        if s.is_dir() {
            cascade::load_style_sets(&s, &mut env.style_sets)?;
        }
    }
    for p in catalogs {
        env.catalog.load_path(p)?;
    }
    for p in style_sets {
        cascade::load_style_sets(p, &mut env.style_sets)?;
    }
    Ok(env)
}

/// 组件目录的 JSON：内置组件加 `catalog` 里的自定义组件，每个带说明、是不是容器、部位和 props 的 JSON Schema。
/// `catalog --json` 和 MCP 的 list_components 共用。
pub fn components_json(catalog: &custom::Catalog) -> serde_json::Value {
    let mut m = serde_json::Map::new();
    for s in components::BUILTINS {
        let mut v = serde_json::json!({
            "description": s.description,
            "container": s.container,
            "props": (s.schema)(),
        });
        if !s.parts.is_empty() {
            v["parts"] = serde_json::json!(s.parts);
        }
        m.insert(s.name.into(), v);
    }
    for d in catalog.defs() {
        m.insert(
            d.name.clone(),
            serde_json::json!({
                "description": d.description,
                "container": d.container,
                "custom": true,
                "props": d.props_schema(),
            }),
        );
    }
    serde_json::Value::Object(m)
}

/// [`parse_docx_with_assets`] 的结果。
pub struct ParsedFiles {
    /// 排好版的 JSON 文本，末尾带换行
    pub json: String,
    /// JSON 里图片路径的前缀（相对 JSON 所在目录，或者绝对路径）
    pub assets_prefix: String,
    /// 图片实际写到的目录；没有图片时是 None
    pub assets_dir: Option<PathBuf>,
    pub asset_count: usize,
    pub warnings: Vec<String>,
}

/// 解析 docx，把图片写到 `json_dir` 下的图片目录，返回 JSON 文本。命令行 parse 和 MCP 共用。
/// `stem` 决定默认的图片目录名「stem.assets」；`assets` 给了就用它（相对 `json_dir`）。
pub fn parse_docx_with_assets(
    bytes: &[u8],
    stem: &str,
    json_dir: &Path,
    assets: Option<&Path>,
) -> Result<ParsedFiles> {
    let rel = assets
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("{stem}.assets")));
    let prefix = rel.to_string_lossy().replace('\\', "/");
    let parsed = parse::docx(
        bytes,
        &parse::Options {
            assets_dir: prefix.clone(),
        },
    )?;
    let mut assets_dir = None;
    if !parsed.assets.is_empty() {
        let dir = if rel.is_absolute() {
            rel.clone()
        } else {
            json_dir.join(&rel)
        };
        std::fs::create_dir_all(&dir).with_context(|| format!("建不了目录 {}", dir.display()))?;
        for (name, data) in &parsed.assets {
            let p = dir.join(name);
            std::fs::write(&p, data).with_context(|| format!("写不了 {}", p.display()))?;
        }
        assets_dir = Some(dir);
    }
    Ok(ParsedFiles {
        json: serde_json::to_string_pretty(&parsed.json)? + "\n",
        assets_prefix: prefix,
        assets_dir,
        asset_count: parsed.assets.len(),
        warnings: parsed.warnings,
    })
}
