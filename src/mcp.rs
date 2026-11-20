//! `document-render mcp`：stdio 上的 MCP server。
//!
//! 工具：render、check、list_components、parse，做的事跟同名命令行一样，渲染都走库里的同一套函数。
//! 标准输出只走 MCP 协议；警告放进工具结果里返回，别的提示打到标准错误。
//! 出错时返回 isError 的工具结果，不让 server 退出。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use document_render::{Env, render};
use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerConfig},
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

/// 启动 server，读写标准输入输出，直到客户端断开。
pub fn run() -> Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        // 渲染、解析放在阻塞线程池里跑；Typst 排版递归深，给足栈
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;
    rt.block_on(async {
        let service = Server::new()
            .serve(rmcp::transport::stdio())
            .await
            .map_err(|e| anyhow!("MCP 握手失败：{e}"))?;
        service
            .waiting()
            .await
            .map_err(|e| anyhow!("MCP server 异常退出：{e}"))?;
        Ok(())
    })
}

const INSTRUCTIONS: &str = "document-render 把一份 JSON 文档渲染成 docx、pdf、html 或 Markdown，也能把 docx 解析回 JSON。\
先调 list_components 看能用哪些组件和每个组件的 props，写好文档后可以先 check，再 render。\
文件路径都建议写绝对路径：server 的当前目录由客户端决定，不一定是你以为的目录。";

const DOCUMENT_DESC: &str = "文档 JSON。写成对象，也可以写成 JSON 字符串。结构：\
{ \"version\": \"0.1\", \"meta\": { \"title\", \"lang\", \"page\": { \"size\": \"A4\", \"margin\": \"20mm\" } }, \
\"styles\": { 样式名: 样式对象 }, \"root\": \"doc\", \"elements\": { id: { \"type\": 组件名, \"props\": {...}, \"style\": 样式名或样式对象, \"children\": [子元素 id] } } }。\
所有元素平铺在 elements 里，用 id 互相引用，root 指向根元素（一般是 Document）；每个元素只能被引用一次，只有容器组件能写 children。\
组件名和 props 以 list_components 为准，多写、少写、类型不对都会报错。\
文字类 props 可以写纯字符串，也可以写 runs 数组，例如 [\"普通\", { \"text\": \"加粗\", \"bold\": true }, { \"type\": \"InlineMath\", \"text\": \"E=mc^2\" }]。\
自定义组件可以写在文档的 components 数组里一起传。";

fn document_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": ["object", "string"],
        "description": DOCUMENT_DESC,
    })
}

/// 一个路径或路径列表。
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
enum Paths {
    One(PathBuf),
    Many(Vec<PathBuf>),
}

impl Paths {
    fn list(p: &Option<Paths>) -> Vec<PathBuf> {
        match p {
            None => Vec::new(),
            Some(Paths::One(x)) => vec![x.clone()],
            Some(Paths::Many(v)) => v.clone(),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RenderParams {
    // 说明写在 document_schema 里
    #[schemars(schema_with = "document_schema")]
    document: Value,
    /// 输出文件路径，按扩展名选格式（.docx、.pdf、.html、.md），建议写绝对路径；
    /// 相对路径按 base_dir 算，没给 base_dir 就按 server 的当前目录算。目录不存在会自动建。
    /// 不写时只能出 html 或 md，内容直接放在结果的 content 里返回，这时要写 format。
    output: Option<PathBuf>,
    /// 格式：html、docx、pdf、md。写了就不看 output 的扩展名
    format: Option<String>,
    /// 只对 html 有用：内嵌 Paged.js，浏览器里按页显示
    #[serde(default)]
    paged: bool,
    /// 用哪个样式集，优先于文档里的 styleSet（同命令行 --style-set）
    style_set: Option<String>,
    /// 样式集文件或目录（JSON），一个路径或路径数组（同命令行 --style-sets）
    style_sets: Option<Paths>,
    /// 自定义组件的定义文件或目录（JSON），一个路径或路径数组（同命令行 --catalog）
    catalog: Option<Paths>,
    /// 文档所在目录：文档里相对路径的图片按它找，它下面的 catalog/、styles/ 会自动读，跟命令行读文件时一样。
    /// 不写时图片按 output 所在目录找（没有 output 就按 server 的当前目录），也不自动读 catalog/、styles/。
    base_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CheckParams {
    #[schemars(schema_with = "document_schema")]
    document: Value,
    /// 自定义组件的定义文件或目录（JSON），一个路径或路径数组
    catalog: Option<Paths>,
    /// 样式集文件或目录（JSON），一个路径或路径数组
    style_sets: Option<Paths>,
    /// 用哪个样式集，优先于文档里的 styleSet
    style_set: Option<String>,
    /// 文档所在目录，它下面的 catalog/、styles/ 会自动读
    base_dir: Option<PathBuf>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListParams {
    /// 一起列出这些定义文件或目录里的自定义组件，一个路径或路径数组
    catalog: Option<Paths>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ParseParams {
    /// 要解析的 .docx 文件路径，建议写绝对路径
    input: PathBuf,
    /// 输出的 JSON 文件路径。不写时 JSON 直接放在结果的 document 里返回
    output: Option<PathBuf>,
    /// 图片导到哪个目录，相对 JSON 所在目录（不写 output 时相对 docx 所在目录）；默认是「文件名.assets」
    assets: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct Server {
    tool_router: ToolRouter<Self>,
}

impl Server {
    fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl Server {
    #[tool(
        name = "render",
        description = "把 document-render 的 JSON 文档渲染成文件：docx、pdf、html 或 md（Markdown，只留结构）。\
格式按 output 的扩展名选，也可以用 format 指定。成功时返回写出的绝对路径 path、格式 format 和警告 warnings（某个格式表达不了的样式会给警告，不算错）。\
不写 output 时只能出 html 或 md，返回的 content 就是文本内容。文档有错时返回错误，错误信息里带出错的元素 id。"
    )]
    async fn render(&self, Parameters(p): Parameters<RenderParams>) -> CallToolResult {
        blocking(move || render_tool(p)).await
    }

    #[tool(
        name = "check",
        description = "只校验文档，不出文件：检查组件名、props、样式、children 引用和自定义组件展开，再看公式能不能解析。\
返回 ok（是否通过）、errors（错误，带元素 id）、warnings（警告，比如解析不了的公式会显示成 LaTeX 原文）和 elements（展开后的元素个数）。"
    )]
    async fn check(&self, Parameters(p): Parameters<CheckParams>) -> CallToolResult {
        blocking(move || check_tool(p)).await
    }

    #[tool(
        name = "list_components",
        description = "列出能用的组件：内置组件加 catalog 里的自定义组件。返回一个对象，键是组件名，值里有 description（说明）、\
container（能不能写 children）、parts（能单独设样式的部位，写在元素的 parts 里）、props（props 的 JSON Schema）；自定义组件带 custom: true。写文档前先看这个。"
    )]
    async fn list_components(&self, Parameters(p): Parameters<ListParams>) -> CallToolResult {
        blocking(move || list_tool(p)).await
    }

    #[tool(
        name = "parse",
        description = "把一份 .docx 解析成 document-render 的 JSON 文档（结构和 render 的输入一样），图片导到 JSON 旁边的目录，JSON 里写相对路径。\
写了 output 就把 JSON 写到这个文件，返回 path；不写就把 JSON 放在结果的 document 里返回。\
认不出、表达不了的内容放在 warnings 里，不会中断。拿到 JSON 后可以改内容再 render；渲染时把 base_dir 设成 JSON 所在目录（没写 output 时是 docx 所在目录），图片才找得到。"
    )]
    async fn parse(&self, Parameters(p): Parameters<ParseParams>) -> CallToolResult {
        blocking(move || parse_tool(p)).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "document-render",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(INSTRUCTIONS)
    }
}

/// 在阻塞线程池里跑，出错或崩了都变成 isError 的工具结果。
async fn blocking<F>(f: F) -> CallToolResult
where
    F: FnOnce() -> Result<Value> + Send + 'static,
{
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(v)) => CallToolResult::structured(v),
        Ok(Err(e)) => tool_error(format!("{e:#}")),
        Err(e) => tool_error(format!("内部错误，处理时程序崩了：{e}")),
    }
}

fn tool_error(msg: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(msg)])
}

/// document 参数：对象转成文本，字符串原样用。
fn document_text(v: &Value) -> Result<String> {
    match v {
        Value::String(s) => Ok(s.clone()),
        Value::Object(_) => Ok(v.to_string()),
        _ => bail!("document 要写成 JSON 对象，或者 JSON 字符串"),
    }
}

fn cwd() -> PathBuf {
    std::env::current_dir().unwrap_or_default()
}

/// 相对路径按 `base` 补成绝对路径。
fn absolute(p: &Path, base: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

fn load_env(
    base_dir: Option<&Path>,
    catalog: &Option<Paths>,
    style_sets: &Option<Paths>,
    style_set: Option<String>,
) -> Result<Env> {
    let here = cwd();
    let abs = |v: Vec<PathBuf>| v.iter().map(|p| absolute(p, &here)).collect::<Vec<_>>();
    document_render::load_env(
        base_dir,
        &abs(Paths::list(catalog)),
        &abs(Paths::list(style_sets)),
        style_set,
    )
}

fn render_tool(p: RenderParams) -> Result<Value> {
    let here = cwd();
    let base_dir = p.base_dir.as_deref().map(|d| absolute(d, &here));
    let output = p
        .output
        .as_deref()
        .map(|o| absolute(o, base_dir.as_deref().unwrap_or(&here)));
    let fmt = match (&p.format, &output) {
        (Some(f), _) => render::Format::from_name(&f.to_ascii_lowercase())?,
        (None, Some(o)) => render::Format::from_path(o)?,
        (None, None) => bail!("没写 output 时要用 format 指定格式（html 或 md）"),
    };
    if output.is_none() && !matches!(fmt, render::Format::Html | render::Format::Markdown) {
        bail!("docx、pdf 是二进制文件，要写 output 指定输出路径");
    }
    let mut warnings = Vec::new();
    if p.paged && !matches!(fmt, render::Format::Html) {
        warnings.push("paged 只对 HTML 有用，已忽略".to_string());
    }
    let text = document_text(&p.document)?;
    let env = load_env(base_dir.as_deref(), &p.catalog, &p.style_sets, p.style_set)?;
    // 图片的基准目录：base_dir，其次 output 所在目录，最后是当前目录
    let image_dir = base_dir
        .clone()
        .or_else(|| {
            output
                .as_deref()
                .and_then(Path::parent)
                .map(Path::to_path_buf)
        })
        .unwrap_or(here);
    let (d, tree) = document_render::prepare(&text, &env)
        .context("document 不是合法的 document-render 文档")?;
    let out = render::render(
        &d,
        &tree,
        fmt,
        &render::Options {
            base_dir: Some(image_dir),
            paged: p.paged,
        },
    )?;
    warnings.extend(out.warnings);
    let format = format_name(fmt);
    match output {
        Some(o) => {
            if let Some(dir) = o.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("建不了目录 {}", dir.display()))?;
            }
            std::fs::write(&o, &out.bytes).with_context(|| format!("写不了 {}", o.display()))?;
            Ok(json!({
                "path": o.to_string_lossy(),
                "format": format,
                "bytes": out.bytes.len(),
                "warnings": warnings,
            }))
        }
        None => Ok(json!({
            "format": format,
            "content": String::from_utf8(out.bytes).context("渲染结果不是 UTF-8 文本")?,
            "warnings": warnings,
        })),
    }
}

fn format_name(f: render::Format) -> &'static str {
    match f {
        render::Format::Html => "html",
        render::Format::Docx => "docx",
        render::Format::Pdf => "pdf",
        render::Format::Markdown => "md",
    }
}

fn check_tool(p: CheckParams) -> Result<Value> {
    let here = cwd();
    let base_dir = p.base_dir.as_deref().map(|d| absolute(d, &here));
    let text = document_text(&p.document)?;
    let env = load_env(base_dir.as_deref(), &p.catalog, &p.style_sets, p.style_set)?;
    Ok(match document_render::prepare(&text, &env) {
        Ok((d, tree)) => json!({
            "ok": true,
            "errors": [],
            "warnings": document_render::check_math(&tree),
            "elements": d.elements.len(),
        }),
        Err(e) => json!({
            "ok": false,
            "errors": [format!("{e:#}")],
            "warnings": [],
        }),
    })
}

fn list_tool(p: ListParams) -> Result<Value> {
    let env = load_env(None, &p.catalog, &None, None)?;
    Ok(document_render::components_json(&env.catalog))
}

fn parse_tool(p: ParseParams) -> Result<Value> {
    let here = cwd();
    let input = absolute(&p.input, &here);
    let bytes = std::fs::read(&input).with_context(|| format!("读不了 {}", input.display()))?;
    let output = p.output.as_deref().map(|o| absolute(o, &here));
    // 图片目录相对 JSON 所在目录；不写 output 时相对 docx 所在目录
    let json_dir = match &output {
        Some(o) => o
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| here.clone()),
        None => input.parent().map(Path::to_path_buf).unwrap_or(here),
    };
    let stem = output
        .as_deref()
        .unwrap_or(&input)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_string();
    let parsed =
        document_render::parse_docx_with_assets(&bytes, &stem, &json_dir, p.assets.as_deref())
            .with_context(|| format!("{} 解析不了", input.display()))?;
    let assets = parsed
        .assets_dir
        .as_ref()
        .map(|d| json!({ "dir": d.to_string_lossy(), "count": parsed.asset_count }));
    match output {
        Some(o) => {
            if let Some(dir) = o.parent() {
                std::fs::create_dir_all(dir)
                    .with_context(|| format!("建不了目录 {}", dir.display()))?;
            }
            std::fs::write(&o, &parsed.json).with_context(|| format!("写不了 {}", o.display()))?;
            Ok(json!({
                "path": o.to_string_lossy(),
                "assets": assets,
                "warnings": parsed.warnings,
            }))
        }
        None => Ok(json!({
            "document": serde_json::from_str::<Value>(&parsed.json)?,
            "base_dir": json_dir.to_string_lossy(),
            "assets": assets,
            "warnings": parsed.warnings,
        })),
    }
}
