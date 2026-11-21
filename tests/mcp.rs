//! mcp：起子进程跑 `document-render mcp`，按 MCP 客户端的顺序发 JSON-RPC：
//! initialize → tools/list → resources/list、resources/read → tools/call（guide、list_components、render、check、parse）。

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
}

impl Client {
    fn start() -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_document-render"))
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("起不来 document-render mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Client {
            child,
            stdin,
            stdout,
            next_id: 1,
        }
    }

    fn send(&mut self, msg: &Value) {
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    /// 发请求，读到 id 对上的那条回复为止。标准输出每一行都必须是 JSON-RPC。
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            let n = self.stdout.read_line(&mut line).unwrap();
            assert!(n > 0, "server 提前关了标准输出");
            let msg: Value = serde_json::from_str(&line)
                .unwrap_or_else(|e| panic!("标准输出里有不是 JSON 的内容（{e}）：{line}"));
            assert_eq!(msg["jsonrpc"], "2.0", "{msg}");
            if msg["id"] == json!(id) {
                return msg;
            }
        }
    }

    /// 调工具，返回是否出错，和 structuredContent（出错时是错误文字）。
    fn call(&mut self, name: &str, args: Value) -> (bool, Value) {
        let r = self.request("tools/call", json!({ "name": name, "arguments": args }));
        assert!(r.get("error").is_none(), "{name} 返回了协议错误：{r}");
        let res = &r["result"];
        let is_error = res["isError"].as_bool().unwrap_or(false);
        let body = if is_error {
            res["content"][0]["text"].clone()
        } else {
            res["structuredContent"].clone()
        };
        (is_error, body)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tmp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("document-render-mcp-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn hello() -> Value {
    serde_json::from_str(&std::fs::read_to_string("examples/hello.json").unwrap()).unwrap()
}

fn handshake(c: &mut Client) -> Value {
    let r = c.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "document-render-test", "version": "0" }
        }),
    );
    c.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    r
}

#[test]
fn mcp_end_to_end() {
    let dir = tmp_dir("e2e");
    let mut c = Client::start();

    // initialize
    let init = handshake(&mut c);
    let res = &init["result"];
    assert_eq!(res["serverInfo"]["name"], "document-render", "{init}");
    assert_eq!(res["protocolVersion"], "2025-06-18", "{init}");
    assert!(res["capabilities"]["tools"].is_object(), "{init}");
    assert!(res["capabilities"]["resources"].is_object(), "{init}");

    // tools/list
    let list = c.request("tools/list", json!({}));
    let tools = list["result"]["tools"].as_array().unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    for n in ["render", "check", "list_components", "parse", "guide"] {
        assert!(names.contains(&n), "缺工具 {n}：{names:?}");
    }
    let render = tools.iter().find(|t| t["name"] == "render").unwrap();
    let props = &render["inputSchema"]["properties"];
    for k in [
        "document",
        "output",
        "format",
        "paged",
        "style_set",
        "catalog",
        "base_dir",
        "allow_missing_fonts",
    ] {
        assert!(props.get(k).is_some(), "render 缺参数 {k}：{render}");
    }
    assert_eq!(render["inputSchema"]["required"], json!(["document"]));
    assert!(!render["description"].as_str().unwrap().is_empty());

    let lc = tools
        .iter()
        .find(|t| t["name"] == "list_components")
        .unwrap();
    assert!(
        lc["description"].as_str().unwrap().contains("agent-guide"),
        "{lc}"
    );

    // 写文档指南：资源和 guide 工具都返回 docs/AGENT.md 全文
    let guide = std::fs::read_to_string("docs/AGENT.md").unwrap();
    let r = c.request("resources/list", json!({}));
    let res = r["result"]["resources"].as_array().unwrap();
    let g = res
        .iter()
        .find(|x| x["uri"] == "document-render://agent-guide")
        .unwrap_or_else(|| panic!("资源列表里没有指南：{r}"));
    assert_eq!(g["mimeType"], "text/markdown", "{g}");
    let r = c.request(
        "resources/read",
        json!({ "uri": "document-render://agent-guide" }),
    );
    let contents = &r["result"]["contents"][0];
    assert_eq!(contents["uri"], "document-render://agent-guide", "{r}");
    assert_eq!(contents["mimeType"], "text/markdown", "{r}");
    assert_eq!(contents["text"].as_str().unwrap(), guide);
    let r = c.request("resources/read", json!({ "uri": "document-render://nope" }));
    assert!(r.get("error").is_some(), "读不存在的资源应该报错：{r}");
    let r = c.request("tools/call", json!({ "name": "guide", "arguments": {} }));
    assert!(!r["result"]["isError"].as_bool().unwrap_or(false), "{r}");
    assert_eq!(r["result"]["content"][0]["text"].as_str().unwrap(), guide);

    // list_components：内置加自定义
    let (err, comps) = c.call("list_components", json!({ "catalog": "examples/catalog" }));
    assert!(!err, "{comps}");
    assert!(comps["Heading"]["props"].is_object(), "{comps}");
    assert_eq!(comps["Table"]["container"], false);
    assert!(comps["Table"]["parts"].as_array().unwrap().len() > 3);
    assert_eq!(comps["Notice"]["custom"], true, "{comps}");

    // render：对象形式的文档出 html
    let html = dir.join("hello.html");
    let (err, out) = c.call(
        "render",
        json!({ "document": hello(), "output": html, "base_dir": "examples" }),
    );
    assert!(!err, "{out}");
    assert_eq!(out["format"], "html");
    assert_eq!(Path::new(out["path"].as_str().unwrap()), html);
    assert!(out["warnings"].is_array());
    let text = std::fs::read_to_string(&html).unwrap();
    assert!(text.contains("季度报告"));

    // render：字符串形式的文档，不给 output，直接拿回 Markdown
    let (err, out) = c.call(
        "render",
        json!({ "document": hello().to_string(), "format": "md" }),
    );
    assert!(!err, "{out}");
    assert!(
        out["content"].as_str().unwrap().contains("# 季度报告"),
        "{out}"
    );

    // render：docx 不给 output 是工具错误
    let (err, msg) = c.call("render", json!({ "document": hello(), "format": "docx" }));
    assert!(err, "{msg}");

    // render：少了 document 也是工具错误，不是协议错误
    let (err, msg) = c.call("render", json!({ "output": dir.join("x.html") }));
    assert!(err, "{msg}");

    // render：错的文档返回工具错误，server 不退出
    let mut bad = hello();
    bad["elements"]["title"]["type"] = json!("Headline");
    let (err, msg) = c.call(
        "render",
        json!({ "document": bad, "output": dir.join("bad.html") }),
    );
    assert!(err, "{msg}");
    assert!(msg.as_str().unwrap().contains("title"), "{msg}");
    assert!(!dir.join("bad.html").exists());

    // check：错的文档
    let (err, out) = c.call("check", json!({ "document": bad }));
    assert!(!err, "{out}");
    assert_eq!(out["ok"], false, "{out}");
    let e = out["errors"][0].as_str().unwrap();
    assert!(e.contains("title") && e.contains("Headline"), "{e}");

    // check：多写 prop
    let mut extra = hello();
    extra["elements"]["title"]["props"]["size"] = json!(3);
    let (_, out) = c.call("check", json!({ "document": extra }));
    assert_eq!(out["ok"], false, "{out}");

    // check：好的文档
    let (err, out) = c.call("check", json!({ "document": hello() }));
    assert!(!err, "{out}");
    assert_eq!(out["ok"], true, "{out}");
    assert!(out["elements"].as_u64().unwrap() >= 5);

    // parse：先用 render 出一份 docx，再解析回来
    let docx = dir.join("sub/hello.docx");
    let (err, out) = c.call("render", json!({ "document": hello(), "output": docx }));
    assert!(!err, "{out}");
    assert_eq!(out["format"], "docx");
    assert!(docx.exists(), "render 应该自动建 sub/ 目录");

    let (err, out) = c.call("parse", json!({ "input": docx }));
    assert!(!err, "{out}");
    let parsed = &out["document"];
    let root = parsed["root"].as_str().unwrap();
    assert!(parsed["elements"][root].is_object(), "{out}");
    let s = parsed.to_string();
    assert!(s.contains("季度报告") && s.contains("Table"), "{s}");

    let json_out = dir.join("hello.json");
    let (err, out) = c.call("parse", json!({ "input": docx, "output": json_out }));
    assert!(!err, "{out}");
    assert_eq!(Path::new(out["path"].as_str().unwrap()), json_out);
    let back: Value = serde_json::from_str(&std::fs::read_to_string(&json_out).unwrap()).unwrap();
    assert_eq!(&back, parsed);

    // 解析出来的 JSON 能再 check 通过（字体照 docx 原样读回 Arial、PingFang SC，本机不一定有，缺字体降为警告）
    let (_, out) = c.call(
        "check",
        json!({ "document": back, "allow_missing_fonts": true }),
    );
    assert_eq!(out["ok"], true, "{out}");

    // 字体缺了：check 算错，render 出 docx 报错不出文件；allow_missing_fonts 降为警告
    let mut nofont = hello();
    nofont["elements"]["title"]["style"] = json!({ "fontFamily": "DR Missing Font Zz" });
    let (_, out) = c.call("check", json!({ "document": nofont }));
    assert_eq!(out["ok"], false, "{out}");
    let e = out["errors"][0].as_str().unwrap();
    assert!(
        e.contains("DR Missing Font Zz") && e.contains("title"),
        "{e}"
    );
    let (_, out) = c.call(
        "check",
        json!({ "document": nofont, "allow_missing_fonts": true }),
    );
    assert_eq!(out["ok"], true, "{out}");
    assert!(
        out["warnings"].to_string().contains("DR Missing Font Zz"),
        "{out}"
    );
    let nf = dir.join("nofont.docx");
    let (err, msg) = c.call("render", json!({ "document": nofont, "output": nf }));
    assert!(err, "{msg}");
    assert!(
        msg.as_str().unwrap().contains("allow_missing_fonts"),
        "{msg}"
    );
    assert!(!nf.exists());
    let (err, out) = c.call(
        "render",
        json!({ "document": nofont, "output": nf, "allow_missing_fonts": true }),
    );
    assert!(!err, "{out}");
    assert!(
        out["warnings"].to_string().contains("DR Missing Font Zz"),
        "{out}"
    );
    assert!(nf.exists());

    // parse：文件不存在是工具错误
    let (err, msg) = c.call("parse", json!({ "input": dir.join("nope.docx") }));
    assert!(err, "{msg}");

    // 出过错以后 server 还活着
    let ping = c.request("ping", json!({}));
    assert!(ping.get("result").is_some(), "{ping}");
}
