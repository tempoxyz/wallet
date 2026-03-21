#!/usr/bin/env node

/**
 * Build and link npm packages locally for development.
 */

import * as NodePath from "node:path";
import * as NodeProcess from "node:process";
import * as NodeChildProcess from "node:child_process";

import { TOOLS, platformPackageName } from "../src/const.mjs";

const ROOT = new URL("..", import.meta.url).pathname;
const REPO_ROOT = NodePath.resolve(ROOT, "..");
const PLATFORM = NodeProcess.platform;
const ARCH = NodeProcess.arch;
const VERSION = NodeProcess.env.RELEASE_VERSION || "0.0.0-dev";

/**
 * @param {string} cmd
 * @param {string} [cwd]
 */
function run(cmd, cwd = ROOT) {
  console.info(`$ ${cmd}`);
  NodeChildProcess.execSync(cmd, { cwd, stdio: "inherit" });
}

console.info("\n-> Building Rust binaries...");
run(`cargo build ${TOOLS.map((tool) => `--package ${tool}`).join(" ")}`, REPO_ROOT);

for (const tool of TOOLS) {
  const platformDir = `${tool}-${PLATFORM}-${ARCH}`;
  const platformPkg = platformPackageName(tool, PLATFORM, ARCH);

  console.info(`\n-> Assembling ${tool} platform package...`);
  run(
    `DRY_RUN=1 RELEASE_VERSION=${VERSION} node scripts/publish.mjs platform ` +
      `--tool ${tool} --os ${PLATFORM} --arch ${ARCH} ` +
      `--binary ${NodePath.join(REPO_ROOT, "target/debug", tool)}`,
    ROOT,
  );

  console.info(`\n-> Assembling ${tool} base package...`);
  run(`DRY_RUN=1 RELEASE_VERSION=${VERSION} node scripts/publish.mjs base --tool ${tool}`, ROOT);

  console.info(`\n-> Linking ${tool} platform package...`);
  run("npm link", NodePath.join(ROOT, platformDir));

  console.info(`\n-> Linking ${tool} base package...`);
  run(`npm link ${platformPkg}`, NodePath.join(ROOT, tool));
  run("npm link", NodePath.join(ROOT, tool));
}

console.info("\nOK linked. Run: tempo-wallet --help");
