#!/usr/bin/env node

/**
 * Full npm install smoke test.
 *
 * Builds binaries, packs tarballs, installs each tool in a temp directory,
 * and verifies the bin shim works.
 */

import * as NodeFS from "node:fs";
import * as NodeOS from "node:os";
import * as NodePath from "node:path";
import * as NodeProcess from "node:process";
import * as NodeChildProcess from "node:child_process";

import { TOOLS } from "../src/const.mjs";

const ROOT = new URL("..", import.meta.url).pathname;
const REPO_ROOT = NodePath.resolve(ROOT, "..");
const PLATFORM = NodeProcess.platform;
const ARCH = NodeProcess.arch;
const VERSION = NodeProcess.env.RELEASE_VERSION || "0.0.0-check";

/**
 * @param {string} cmd
 * @param {string} [cwd]
 */
function run(cmd, cwd = ROOT) {
  console.info(`$ ${cmd}`);
  NodeChildProcess.execSync(cmd, { cwd, stdio: "inherit" });
}

/**
 * @param {string} cmd
 * @param {string} [cwd]
 * @returns {string}
 */
function runCapture(cmd, cwd = ROOT) {
  return NodeChildProcess.execSync(cmd, { cwd, encoding: "utf8" }).trim();
}

console.info("\n-> Building Rust binaries...");
run(`cargo build ${TOOLS.map((tool) => `--package ${tool}`).join(" ")}`, REPO_ROOT);

for (const tool of TOOLS) {
  const platformDir = `${tool}-${PLATFORM}-${ARCH}`;
  const tmpDir = NodeFS.mkdtempSync(NodePath.join(NodeOS.tmpdir(), `${tool}-npm-check-`));

  console.info(`\n-> Assembling ${tool} platform package...`);
  run(
    `DRY_RUN=1 RELEASE_VERSION=${VERSION} node scripts/publish.mjs platform ` +
      `--tool ${tool} --os ${PLATFORM} --arch ${ARCH} ` +
      `--binary ${NodePath.join(REPO_ROOT, "target/debug", tool)}`,
    ROOT,
  );

  console.info(`\n-> Assembling ${tool} base package...`);
  run(`DRY_RUN=1 RELEASE_VERSION=${VERSION} node scripts/publish.mjs base --tool ${tool}`, ROOT);

  console.info(`\n-> Packing ${tool} into ${tmpDir}...`);
  const platformTar = runCapture(
    `npm pack --pack-destination ${tmpDir}`,
    NodePath.join(ROOT, platformDir),
  );
  const baseTar = runCapture(`npm pack --pack-destination ${tmpDir}`, NodePath.join(ROOT, tool));

  console.info(`\n-> Installing ${tool} from tarballs...`);
  run("npm init -y", tmpDir);
  run(
    `npm install ${NodePath.join(tmpDir, platformTar)} ${NodePath.join(tmpDir, baseTar)}`,
    tmpDir,
  );

  console.info(`\n-> Running ${tool} smoke test...`);
  const binDir = NodePath.join(tmpDir, "node_modules", ".bin");
  run(`${NodePath.join(binDir, tool)} --help`);

  console.info(`\n-> Cleaning up ${tool} temp dir...`);
  NodeFS.rmSync(tmpDir, { recursive: true, force: true });
}

console.info("\nOK npm-check passed");
