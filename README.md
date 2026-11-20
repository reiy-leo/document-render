<p align="center"><img src="docs/logo.png" width="128" alt="document-render"></p>

# document-render

一份 JSON，出 Word、PDF 和网页。

JSON 里只引用注册好的组件（标题、正文、列表、表格、图片……），再带一点样式。document-render 负责把它排成 docx、pdf 或 html。AI 也能通过 MCP 直接调用。

> 还在开发，第一版没发布。
>
> 以前叫 fmtagno，更早叫 pewp，2026-11-18 改成现在的名字。旧的 GitHub 地址会自动跳到这里。

```sh
document-render render report.json -o report.docx
document-render render report.json -o report.pdf
document-render render report.json -o report.md     # Markdown，只留结构
document-render render report.json -o report.html --paged   # 屏幕上按页显示
document-render check report.json
document-render parse report.docx -o report.json     # 反过来，从 Word 解析出 JSON，图片在 report.assets/
document-render render report.json -o report.html --catalog my-components/ --style-sets themes/ --style-set formal
document-render guide                                 # 打印给 AI 看的写文档指南（docs/AGENT.md）
document-render mcp                                   # MCP server（stdio），给 AI 客户端用
```

## 给 AI 用（MCP）

在 Claude Desktop、Cursor 的 MCP 配置里加上：

```json
{
  "mcpServers": {
    "document-render": { "command": "document-render", "args": ["mcp"] }
  }
}
```

Claude Code 用 `claude mcp add document-render -- document-render mcp`。

AI 能调五个工具：`guide` 拿写文档指南，`list_components` 看有哪些组件，`check` 校验文档，`render` 出 docx、pdf、html、md，`parse` 把 docx 读成 JSON。指南也能当 MCP 资源 `document-render://agent-guide` 读。参数和更多客户端的写法见 [docs/PRODUCT.md](docs/PRODUCT.md) 的「MCP」一节。

不走 MCP 的 agent，直接把 [docs/AGENT.md](docs/AGENT.md) 给它读（或者 `document-render guide` 的输出）：文档结构、全部组件和样式字段、能通过 check 的完整示例、常见错误都在里面，读完就能写。

## 文档

文档格式和计划见 [docs/PRODUCT.md](docs/PRODUCT.md)，样式模型见 [docs/STYLE.md](docs/STYLE.md)，给 AI agent 的速查指南见 [docs/AGENT.md](docs/AGENT.md)。

## 许可证

MIT
