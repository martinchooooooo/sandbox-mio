import fs from "fs";
import os from "os";
import path from "path";
import assert from "node:assert/strict";
import { spawnSync } from "child_process";
import { after, before, describe, it } from "node:test";

import {
  connectionInfo,
  provisionMachine,
  removeMachine,
  stopMachine,
} from "../lib/podman-machine.js";
import {
  ensureMutagen,
  removeSshAlias,
  syncCreate,
  syncFlush,
  syncTerminate,
  writeSshAlias,
} from "../lib/mutagen.js";

// The remote stand-in. Dedicated and ephemeral: brought up for this suite and stopped (or
// removed) afterwards, so it never collides with the dev machine or the app's mio-tests machine.
const REMOTE_MACHINE = "mio-tests-remote";
const SSH_ALIAS = "mio-tests-remote";
const SESSION = "mio-e2e-remote-sync";
const CONTAINER = "mio-e2e-remote-sync";

// Honoured for fast local iteration: KEEP leaves the machine running, REMOVE deletes its disk
// image. Default is to stop the machine (down, but image kept so the next run skips init).
const KEEP = process.env.MIO_REMOTE_KEEP === "1";
const REMOVE = process.env.MIO_REMOTE_REMOVE === "1";

// shell:false throughout — these commands carry shell metacharacters (`>`, `&&`) inside their
// remote-shell arguments, and routing through Windows cmd.exe (shell:true) would let cmd eat
// them before podman runs. The same reason startPodmanSocket spawns podman directly.
function podmanRemote(args, { check = true } = {}) {
  const out = spawnSync("podman", ["--connection", REMOTE_MACHINE, ...args], {
    encoding: "utf8",
    shell: false,
  });
  if (check && out.status !== 0) {
    throw new Error(`podman ${args.join(" ")} failed: ${out.stderr}`);
  }
  return out.stdout?.trim() ?? "";
}

function machineSsh(command) {
  const out = spawnSync("podman", ["machine", "ssh", REMOTE_MACHINE, command], {
    encoding: "utf8",
    shell: false,
  });
  if (out.status !== 0) {
    throw new Error(`machine ssh "${command}" failed: ${out.stderr}`);
  }
  return out.stdout?.trim() ?? "";
}

describe("remote workspace sync (two-machine)", () => {
  let laptopDir;
  let remoteDir;

  before(() => {
    ensureMutagen();
    provisionMachine(REMOTE_MACHINE);

    const info = connectionInfo(REMOTE_MACHINE);
    writeSshAlias(SSH_ALIAS, info);

    // The remote workspace lives inside the remote VM filesystem — created over the connection,
    // the way the Phase 2 backend must create it for a remote host (not with a laptop-side
    // mkdir). The laptop directory is a throwaway temp dir standing in for the user's profile.
    remoteDir = `/home/${info.user}/work/${SESSION}`;
    laptopDir = fs.mkdtempSync(path.join(os.tmpdir(), "mio-remote-sync-"));

    machineSsh(`rm -rf ${remoteDir} && mkdir -p ${remoteDir}`);

    // A container on the remote connection bind-mounts the remote workspace dir at /root/work.
    podmanRemote(["rm", "-f", CONTAINER], { check: false });
    podmanRemote([
      "run", "-d", "--name", CONTAINER,
      "--volume", `${remoteDir}:/root/work`, "--workdir", "/root/work",
      "alpine:latest", "sh", "-c", "while true; do sleep 3600; done",
    ]);

    syncCreate(SESSION, laptopDir, `${SSH_ALIAS}:${remoteDir}`);
    syncFlush(SESSION);
  });

  after(() => {
    syncTerminate(SESSION);
    removeSshAlias(SSH_ALIAS);
    podmanRemote(["rm", "-f", CONTAINER], { check: false });
    if (REMOVE) removeMachine(REMOTE_MACHINE);
    else if (!KEEP) stopMachine(REMOTE_MACHINE);
    if (laptopDir) fs.rmSync(laptopDir, { recursive: true, force: true });
  });

  it("propagates a container-written file down to the laptop directory", () => {
    podmanRemote(["exec", CONTAINER, "sh", "-c", "echo round-trip > /root/work/from-container.txt"]);
    syncFlush(SESSION);

    const landed = path.join(laptopDir, "from-container.txt");
    assert.ok(fs.existsSync(landed), "container file should appear in the laptop directory");
    assert.equal(fs.readFileSync(landed, "utf8").trim(), "round-trip");
  });

  it("propagates a laptop-written file up into the running container", () => {
    fs.writeFileSync(path.join(laptopDir, "from-laptop.txt"), "edited on laptop");
    syncFlush(SESSION);

    const seen = podmanRemote(["exec", CONTAINER, "cat", "/root/work/from-laptop.txt"]);
    assert.equal(seen, "edited on laptop");
  });
});
