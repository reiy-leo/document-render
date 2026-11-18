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
document-render render report.json -o report.html --catalog my-components/ --style-sets themes/ --style-set formal
document-render mcp
```

文档格式和计划见 [docs/PRODUCT.md](docs/PRODUCT.md)。

## 许可证

MIT
