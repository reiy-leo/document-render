use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use document_render::{components, render};

mod mcp;

#[derive(Parser)]
#[command(
    name = "document-render",
    version,
    about = "把一份 JSON 渲染成 docx / pdf / html / Markdown"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

/// 文档以外的定义。
#[derive(clap::Args)]
struct Defs {
    /// 自定义组件的定义文件或目录（JSON），可写多次；文档目录下的 catalog/ 会自动读
    #[arg(long)]
    catalog: Vec<PathBuf>,
    /// 样式集文件或目录（JSON），可写多次；文档目录下的 styles/ 会自动读
    #[arg(long)]
    style_sets: Vec<PathBuf>,
    /// 用哪个样式集，优先于文档里的 styleSet
    #[arg(long)]
    style_set: Option<String>,
}

impl Defs {
    fn env(self, input: &Path) -> Result<document_render::Env> {
        document_render::load_env(
            doc_dir(input).as_deref(),
            &self.catalog,
            &self.style_sets,
            self.style_set,
        )
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// 渲染文档：document-render render doc.json -o out.html（输入写 - 表示从标准输入读）
    Render {
        input: PathBuf,
        /// 输出文件，按扩展名选格式；写 - 表示输出到标准输出，这时要配 --format
        #[arg(short, long)]
        output: PathBuf,
        /// 指定格式（html、docx、pdf、md），不写就看输出文件的扩展名
        #[arg(short, long)]
        format: Option<String>,
        /// 只对 HTML 有用：内嵌 Paged.js，打开就按页显示（分页、表头重复不用打印也看得到）
        #[arg(long)]
        paged: bool,
        /// pdf、docx：本机找不到文档用到的字体时照常出文件，只给警告（默认报错）
        #[arg(long)]
        allow_missing_fonts: bool,
        #[command(flatten)]
        defs: Defs,
    },
    /// 只校验，不出文件（本机找不到的字体也算错）
    Check {
        input: PathBuf,
        /// 本机找不到字体时只给警告，不算校验没通过
        #[arg(long)]
        allow_missing_fonts: bool,
        #[command(flatten)]
        defs: Defs,
    },
    /// 列出可用组件；加 --json 输出每个组件 props 的 JSON Schema
    Catalog {
        #[arg(long)]
        json: bool,
        /// 一起列出这些定义文件或目录里的自定义组件
        #[arg(long)]
        catalog: Vec<PathBuf>,
    },
    /// 从 Word 文档解析出 JSON：document-render parse doc.docx -o doc.json（输入写 - 表示从标准输入读）
    Parse {
        input: PathBuf,
        /// 输出的 JSON 文件；不写或写 - 时输出到标准输出
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// 图片导出到哪个目录，相对 JSON 所在的目录；默认是「JSON 文件名.assets」
        #[arg(long)]
        assets: Option<PathBuf>,
    },
    /// 打印给 AI agent 看的写文档指南（docs/AGENT.md）：结构、全部组件、样式字段、完整示例、常见错误
    Guide,
    /// 以 MCP server 方式运行（stdio），给 Claude Desktop、Cursor 这类 AI 客户端调用
    Mcp,
}

fn read_input(p: &Path) -> Result<String> {
    if p == Path::new("-") {
        let mut s = String::new();
        std::io::stdin().read_to_string(&mut s)?;
        Ok(s)
    } else {
        std::fs::read_to_string(p).with_context(|| format!("读不了 {}", p.display()))
    }
}

/// 文档所在目录；从标准输入读时没有。
fn doc_dir(input: &Path) -> Option<PathBuf> {
    if input == Path::new("-") {
        None
    } else {
        input.parent().map(Path::to_path_buf)
    }
}

fn main() -> Result<()> {
    match Cli::parse().cmd {
        Cmd::Render {
            input,
            output,
            format,
            paged,
            allow_missing_fonts,
            defs,
        } => {
            let fmt = match &format {
                Some(f) => render::Format::from_name(&f.to_ascii_lowercase())?,
                None if output == Path::new("-") => bail!("输出到标准输出时要用 --format 指定格式"),
                None => render::Format::from_path(&output)?,
            };
            if paged && !matches!(fmt, render::Format::Html) {
                eprintln!("警告：--paged 只对 HTML 有用，已忽略");
            }
            let text = read_input(&input)?;
            let base_dir = doc_dir(&input);
            let env = defs.env(&input)?;
            let (d, tree) = document_render::prepare(&text, &env)
                .with_context(|| format!("{} 不是合法的 document-render 文档", input.display()))?;
            let out = render::render(
                &d,
                &tree,
                fmt,
                &render::Options {
                    base_dir,
                    paged,
                    allow_missing_fonts,
                },
            )?;
            for w in &out.warnings {
                eprintln!("警告：{w}");
            }
            if output == Path::new("-") {
                std::io::stdout().write_all(&out.bytes)?;
            } else {
                std::fs::write(&output, &out.bytes)
                    .with_context(|| format!("写不了 {}", output.display()))?;
                eprintln!("已生成 {}", output.display());
            }
            Ok(())
        }
        Cmd::Check {
            input,
            allow_missing_fonts,
            defs,
        } => {
            let text = read_input(&input)?;
            let env = defs.env(&input)?;
            let (d, tree) = document_render::prepare(&text, &env)
                .with_context(|| format!("{} 校验没通过", input.display()))?;
            for w in document_render::check_styles(&d)
                .into_iter()
                .chain(document_render::check_math(&tree))
            {
                eprintln!("警告：{w}");
            }
            let fonts = document_render::check_fonts(&d, &tree);
            if allow_missing_fonts {
                for w in &fonts {
                    eprintln!("警告：{w}");
                }
            } else if !fonts.is_empty() {
                for e in &fonts {
                    eprintln!("错误：{e}");
                }
                bail!(
                    "{} 校验没通过：本机找不到 {} 个字体（加 --allow-missing-fonts 降为警告）",
                    input.display(),
                    fonts.len()
                );
            }
            println!("ok：展开后 {} 个元素", d.elements.len());
            Ok(())
        }
        Cmd::Catalog { json, catalog } => {
            let cat = document_render::load_env(None, &catalog, &[], None)?.catalog;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&document_render::components_json(&cat))?
                );
            } else {
                for s in components::BUILTINS {
                    println!(
                        "{:<10} {}{}",
                        s.name,
                        s.description,
                        if s.container { "（容器）" } else { "" }
                    );
                }
                for d in cat.defs() {
                    println!(
                        "{:<10} {}（自定义{}）",
                        d.name,
                        d.description,
                        if d.container { "，容器" } else { "" }
                    );
                }
            }
            Ok(())
        }
        Cmd::Parse {
            input,
            output,
            assets,
        } => parse_cmd(&input, output.as_deref(), assets.as_deref()),
        Cmd::Guide => {
            std::io::stdout().write_all(document_render::AGENT_GUIDE.as_bytes())?;
            Ok(())
        }
        Cmd::Mcp => mcp::run(),
    }
}

/// parse 子命令：读 docx，写 JSON 和图片目录。
fn parse_cmd(input: &Path, output: Option<&Path>, assets: Option<&Path>) -> Result<()> {
    let bytes = if input == Path::new("-") {
        let mut b = Vec::new();
        std::io::stdin().read_to_end(&mut b)?;
        b
    } else {
        std::fs::read(input).with_context(|| format!("读不了 {}", input.display()))?
    };
    let output = output.filter(|o| *o != Path::new("-"));
    // JSON 所在目录：图片目录相对它
    let json_dir = match output {
        Some(o) => o.parent().map(Path::to_path_buf).unwrap_or_default(),
        None => PathBuf::new(),
    };
    let stem = output
        .or((input != Path::new("-")).then_some(input))
        .and_then(|p| p.file_stem())
        .and_then(|s| s.to_str())
        .unwrap_or("document")
        .to_string();
    let parsed = document_render::parse_docx_with_assets(&bytes, &stem, &json_dir, assets)
        .with_context(|| format!("{} 解析不了", input.display()))?;
    for w in &parsed.warnings {
        eprintln!("警告：{w}");
    }
    match output {
        Some(o) => {
            std::fs::write(o, &parsed.json).with_context(|| format!("写不了 {}", o.display()))?;
            if parsed.asset_count == 0 {
                eprintln!("已生成 {}", o.display());
            } else {
                eprintln!(
                    "已生成 {}，图片 {} 张在 {}/",
                    o.display(),
                    parsed.asset_count,
                    parsed.assets_prefix
                );
            }
        }
        None => std::io::stdout().write_all(parsed.json.as_bytes())?,
    }
    Ok(())
}
