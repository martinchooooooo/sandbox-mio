// Spike harness: times each stage of the current Terminal-tab chain (docs/features/native-terminal/01_ideas.md,
// "Current State") against a throwaway container, from a script.
//
// Usage: node current_chain.mjs <container> <warm|cold> <iterations> <out.json>
// Required env: PODMAN_EXE, MUTAGEN_EXE, PODMAN_CONNECTION (the private MUTAGEN_DATA_DIRECTORY,
// MUTAGEN_SSH_PATH and SPIKE_SSH_CONFIG must already be exported; see run_all.sh).
import { spawn } from "node:child_process";
import { writeFileSync } from "node:fs";

const [name, mode, iterStr, outFile] = process.argv.slice(2);
const iters = Number(iterStr);
const PODMAN = process.env.PODMAN_EXE;
const MUTAGEN = process.env.MUTAGEN_EXE;
const CONN = process.env.PODMAN_CONNECTION;
const PORT = 18090;
const FWD = `spike-fwd-${name}`;

function run(exe, args) {
  return new Promise((resolve) => {
    const t0 = performance.now();
    const p = spawn(exe, args, { stdio: ["ignore", "pipe", "pipe"], windowsHide: true });
    let out = "";
    p.stdout.on("data", (d) => (out += d));
    p.stderr.on("data", (d) => (out += d));
    p.on("close", (code) => resolve({ ms: performance.now() - t0, code, out }));
  });
}
const podman = (...a) => run(PODMAN, ["--connection", CONN, ...a]);
const mutagen = (...a) => run(MUTAGEN, a);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function pollOk(url) {
  const t0 = performance.now();
  for (;;) {
    try {
      const r = await fetch(url);
      if (r.status === 200) return { ms: performance.now() - t0, html: await r.text() };
    } catch {}
    if (performance.now() - t0 > 30000) throw new Error("timeout waiting for 200");
    await sleep(25);
  }
}

const results = [];
for (let i = 0; i < iters; i++) {
  if (mode === "cold") {
    await podman("stop", "-t", "0", name);
    await mutagen("forward", "terminate", FWD);
  }
  const row = {};
  const total0 = performance.now();

  row.start = (await podman("start", name)).ms;
  const status = await podman("exec", name, "zellij", "web", "--status");
  row.status = status.ms;
  if (!(status.out).includes("online")) {
    const d = await podman("exec", name, "zellij", "web", "--daemonize");
    if (d.code !== 0) throw new Error("daemonize failed: " + d.out);
    row.daemonize = d.ms;
  } else row.daemonize = 0;
  const tok = await podman("exec", name, "zellij", "web", "--create-token");
  row.token = tok.ms;
  const token = /token_\d+:\s*([0-9a-f-]+)/.exec(tok.out)?.[1];
  if (!token) throw new Error("no token: " + tok.out);

  // The app best-effort terminates the named forward first, then creates it.
  const f0 = performance.now();
  await mutagen("forward", "terminate", FWD);
  const fc = await mutagen("forward", "create", `--name=${FWD}`, `tcp:127.0.0.1:${PORT}`, `${name}:tcp:127.0.0.1:8082`);
  if (fc.code !== 0) throw new Error("forward create failed: " + fc.out);
  row.forward = performance.now() - f0;

  const base = `http://127.0.0.1:${PORT}`;
  const first = await pollOk(`${base}/${name}`);
  row.first200 = first.ms;
  row.chainToFirst200 = performance.now() - total0;

  // Headless estimate of the double page load: html + all assets, login POST, html again (assets cached).
  const l0 = performance.now();
  const assets = [...first.html.matchAll(/(?:src|href)="(assets\/[^"]+)"/g)].map((m) => m[1]);
  const bodies = await Promise.all(assets.map((a) => fetch(`${base}/${a}`).then((r) => r.arrayBuffer())));
  row.load1 = performance.now() - l0;
  row.assetCount = assets.length;
  row.assetBytes = bodies.reduce((s, b) => s + b.byteLength, 0);
  const lg0 = performance.now();
  const lr = await fetch(`${base}/command/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ auth_token: token, remember_me: true }),
  });
  row.loginStatus = lr.status;
  row.login = performance.now() - lg0;
  const l2 = performance.now();
  await (await fetch(`${base}/${name}`)).text();
  row.load2htmlOnly = performance.now() - l2;
  row.total = performance.now() - total0;
  results.push(row);
  console.log(i, JSON.stringify(Object.fromEntries(Object.entries(row).map(([k, v]) => [k, Math.round(v)]))));
  await mutagen("forward", "terminate", FWD);
}
writeFileSync(outFile, JSON.stringify(results, null, 1));
