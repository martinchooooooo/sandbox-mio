// Spike recorder: runs pi in RPC mode inside a container, sends one prompt, and writes every
// stdout record as `{ms, record}` (ms since the prompt was sent) until `agent_settled`.
// Usage: node record.mjs <container> <out.jsonl> "<prompt>" [timeoutSeconds]
import { spawn } from "node:child_process";
import fs from "node:fs";
import readline from "node:readline";

const [container, out, prompt, timeoutArg] = process.argv.slice(2);
if (!container || !out || !prompt) {
    console.error('usage: node record.mjs <container> <out.jsonl> "<prompt>" [timeoutSeconds]');
    process.exit(2);
}
const timeoutMs = Number(timeoutArg ?? 300) * 1000;
const connection = process.env.PODMAN_CONNECTION ?? "podman-machine-default";

const pi = spawn(
    "podman",
    ["--connection", connection, "exec", "-i", "-w", "/root/work", container,
        "mise", "exec", "--", "pi", "--mode", "rpc", "--no-session"],
    { stdio: ["pipe", "pipe", "inherit"] },
);
const sink = fs.createWriteStream(out);
const t0 = Date.now();
let settled = false;

readline.createInterface({ input: pi.stdout }).on("line", (line) => {
    if (!line.trim()) return;
    let record;
    try {
        record = JSON.parse(line);
    } catch {
        record = { type: "unparsed", line };
    }
    sink.write(JSON.stringify({ ms: Date.now() - t0, record }) + "\n");
    if (record.type !== "message_update") console.error(`${Date.now() - t0}ms ${record.type}`);
    if (record.type === "agent_settled" && !settled) {
        settled = true;
        pi.stdin.end();
    }
});

pi.stdin.write(JSON.stringify({ id: "p1", type: "prompt", message: prompt }) + "\n");

const timer = setTimeout(() => {
    console.error("timeout: killing pi");
    pi.kill();
}, timeoutMs);

pi.on("exit", (code) => {
    clearTimeout(timer);
    sink.end(() => process.exit(settled ? 0 : 1 + (code ?? 0)));
});
