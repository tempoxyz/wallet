#!/usr/bin/env node

import * as NodeFS from "node:fs";
import * as NodePath from "node:path";
import * as NodeModule from "node:module";
import * as NodeChildProcess from "node:child_process";

import { TOOLS, binaryName, platformPackage, toolFromPackageName } from "./const.mjs";

const require = NodeModule.createRequire(import.meta.url);

function resolveTool() {
  const invoked = NodePath.basename(process.argv[1]).replace(/\.mjs$/, "");
  if (TOOLS.includes(invoked)) {
    return invoked;
  }

  const self = JSON.parse(NodeFS.readFileSync(new URL("./package.json", import.meta.url), "utf8"));
  const tool = toolFromPackageName(self.name);
  if (tool) {
    return tool;
  }

  console.error(`Unable to resolve tool from invocation: ${invoked}`);
  process.exit(1);
}

/**
 * Resolve the binary path. Tries:
 * 1. Platform-specific optionalDependency package
 * 2. Local `dist/` fallback (written by postinstall)
 *
 * @param {string} tool
 * @returns {string}
 */
function resolveBinary(tool) {
  const bin = binaryName(tool);
  const pkg = platformPackage(tool);

  try {
    return require.resolve(`${pkg}/bin/${bin}`);
  } catch {}

  const fallback = new URL(`./dist/${bin}`, import.meta.url).pathname;
  if (NodeFS.existsSync(fallback)) {
    return fallback;
  }

  throw new Error(
    `Could not find the ${tool} binary. ` +
      `The platform package ${pkg} was not installed and the postinstall fallback failed. ` +
      `Try reinstalling @tempoxyz/${tool}.`,
  );
}

function run() {
  const tool = resolveTool();
  const args = process.argv.slice(2);
  const binary = resolveBinary(tool);

  try {
    NodeChildProcess.execFileSync(binary, args, { stdio: "inherit" });
  } catch (/** @type {any} */ error) {
    if (error.status !== null) {
      process.exit(error.status);
    }
    throw error;
  }
}

run();
