// Minimal stdio MCP server: one tool "echo_secret_len" reporting the length of env DUMMY_SECRET.
import readline from "node:readline";
const rl = readline.createInterface({ input: process.stdin });
const send = (m) => process.stdout.write(JSON.stringify(m) + "\n");
rl.on("line", (l) => {
  let m; try { m = JSON.parse(l); } catch { return; }
  if (m.method === "initialize") send({ jsonrpc: "2.0", id: m.id, result: { protocolVersion: m.params?.protocolVersion ?? "2024-11-05", capabilities: { tools: {} }, serverInfo: { name: "mini", version: "0.0.1" } } });
  else if (m.method === "tools/list") send({ jsonrpc: "2.0", id: m.id, result: { tools: [{ name: "echo_secret_len", description: "length of DUMMY_SECRET", inputSchema: { type: "object", properties: {} } }] } });
  else if (m.method === "tools/call") send({ jsonrpc: "2.0", id: m.id, result: { content: [{ type: "text", text: String((process.env.DUMMY_SECRET || "").length) }] } });
  else if (m.id !== undefined) send({ jsonrpc: "2.0", id: m.id, result: {} });
});
