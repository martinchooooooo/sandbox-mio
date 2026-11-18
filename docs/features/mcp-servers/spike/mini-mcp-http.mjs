// Minimal streamable-HTTP MCP server that answers only when header X-Dummy equals the dummy secret.
import http from "node:http";
const port = Number(process.argv[2] || 8799);
http.createServer((req, res) => {
  let b = ""; req.on("data", (c) => (b += c));
  req.on("end", () => {
    console.error(req.method, req.headers["x-dummy"] ?? "<none>");
    if (req.headers["x-dummy"] !== "hunter2") { res.writeHead(401).end(); return; }
    if (req.method !== "POST") { res.writeHead(405).end(); return; }
    const m = JSON.parse(b || "{}"); if (m.id === undefined) { res.writeHead(202).end(); return; }
    const r = m.method === "initialize" ? { protocolVersion: m.params?.protocolVersion ?? "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "mini-http", version: "0.0.1" } } : m.method === "tools/list" ? { tools: [] } : {};
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({ jsonrpc: "2.0", id: m.id, result: r }));
  });
}).listen(port, "127.0.0.1");
