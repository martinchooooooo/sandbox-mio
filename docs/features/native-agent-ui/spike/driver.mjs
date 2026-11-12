// Spike host driver: attaches over `ssh <alias>` stdio, sends JSON-line commands, records every
// journal record with its byte offset, and persists the cursor for the next attach.
// usage: node driver.mjs --config <ssh_config> --alias <alias> --state <cursor.json> --trace <out.jsonl>
//          [--fresh] [--send '<json>']...
//          [--send-file <file with one json line>]... [--until <type>[:count]] [--timeout <s>] [--drop hard|clean]
import { spawn } from "node:child_process";
import { readFileSync, writeFileSync, appendFileSync, existsSync } from "node:fs";

const argv = process.argv.slice(2);
const opt = { send: [], until: "agent_settled", timeout: 240, drop: "clean" };
for (let i = 0; i < argv.length; i++) {
  const k = argv[i].replace(/^--/, "");
  if (k === "fresh") opt.fresh = true;
  else if (k === "send") opt.send.push(argv[++i]);
  else if (k === "send-file") opt.send.push(readFileSync(argv[++i], "utf8").trimEnd());
  else opt[k] = argv[++i];
}

const SSH = process.env.SPIKE_SSH ?? "C:\\Windows\\System32\\OpenSSH\\ssh.exe";
let cursor = { generation: "-", offset: "end" };
if (!opt.fresh && opt.state && existsSync(opt.state)) cursor = JSON.parse(readFileSync(opt.state, "utf8"));

const [untilType, untilCountStr] = opt.until.split(":");
let untilCount = Number(untilCountStr ?? 1);
const remote = `sh /usr/local/lib/mio/pi-attach.sh ${cursor.generation} ${cursor.offset}`;
const child = spawn(SSH, ["-F", opt.config, "-T", opt.alias, remote], { stdio: ["pipe", "pipe", "pipe"] });
const t0 = Date.now();

let buf = Buffer.alloc(0);
let header = null;
let offset = 0; // journal byte offset of the next unread byte
let stats = { records: 0, bytes: 0, byType: {} };

function finish(reason) {
  if (opt.state && header) writeFileSync(opt.state, JSON.stringify({ generation: header.generation, offset }));
  console.log(JSON.stringify({ reason, ms: Date.now() - t0, header, endOffset: offset, ...stats }));
  if (opt.drop === "hard") child.kill("SIGKILL");
  else child.stdin.end();
  setTimeout(() => process.exit(0), 1500);
}

child.stderr.on("data", (d) => process.stderr.write(`[ssh] ${d}`));
child.on("exit", (code) => { if (!finished) { finished = true; finish(`ssh exited ${code}`); } });
let finished = false;

child.stdout.on("data", (chunk) => {
  // The persisted cursor is the last consumed record; anything read after finishing is left for the next attach.
  if (finished) return;
  buf = Buffer.concat([buf, chunk]);
  let nl;
  // Split on LF only: pi's JSON strings may legally contain U+2028/U+2029.
  while ((nl = buf.indexOf(0x0a)) !== -1) {
    const line = buf.subarray(0, nl);
    buf = buf.subarray(nl + 1);
    const rec = JSON.parse(line.toString("utf8"));
    if (finished) break;
    if (!header) {
      header = rec;
      offset = rec.offset;
      if (rec.type !== "mio_attach") { finished = true; return finish("attach failed"); }
      for (const s of opt.send) child.stdin.write(s + "\n");
      continue;
    }
    const at = offset;
    offset += nl + 1;
    stats.records++; stats.bytes += nl + 1;
    const key = rec.type === "message_update" ? `message_update/${rec.assistantMessageEvent?.type}` : rec.type === "extension_ui_request" ? `extension_ui_request/${rec.method}` : rec.type;
    stats.byType[key] = (stats.byType[key] ?? 0) + 1;
    if (opt.trace) appendFileSync(opt.trace, JSON.stringify({ offset: at, bytes: nl + 1, ms: Date.now() - t0, record: rec }) + "\n");
    const matchKey = untilType.includes("/") ? key : rec.type;
    if (!finished && matchKey === untilType && --untilCount <= 0) { finished = true; finish(`until ${opt.until}`); }
  }
});

setTimeout(() => { if (!finished) { finished = true; finish("timeout"); } }, Number(opt.timeout) * 1000);
