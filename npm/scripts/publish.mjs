#!/usr/bin/env node

/**
 * Assemble and publish a tool-specific npm package.
 *
 * Usage:
 *   node scripts/publish.mjs platform --tool tempo-wallet --os darwin --arch arm64 --binary path
 *   node scripts/publish.mjs base --tool tempo-wallet
 *
 * Environment:
 *   RELEASE_VERSION - version to stamp (required)
 *   DRY_RUN=1       - assemble and stamp but skip npm publish
 */

import * as NodeFS from "node:fs";
import * as NodePath from "node:path";
import * as NodeChildProcess from "node:child_process";

import { assertTool, binaryName } from "../src/const.mjs";
import { basePackageJson, platformPackageJson } from "../src/package.json.mjs";

const ROOT = new URL("..", import.meta.url).pathname;

function parseArgs() {
  const args = process.argv.slice(2);
  const [mode] = args;

  if (mode === "platform") {
    /** @type {{ tool?: string; os?: string; arch?: string; binary?: string }} */
    const flags = {};

    for (let index = 1; index < args.length; index++) {
      if (args[index] === "--tool") flags.tool = args[++index];
      else if (args[index] === "--os") flags.os = args[++index];
      else if (args[index] === "--arch") flags.arch = args[++index];
      else if (args[index] === "--binary") flags.binary = args[++index];
    }

    if (!flags.tool || !flags.os || !flags.arch || !flags.binary) {
      console.error(
        "Usage: publish.mjs platform --tool <tool> --os <os> --arch <arch> --binary <path>",
      );
      process.exit(1);
    }

    return { mode, ...flags };
  }

  if (mode === "base") {
    const toolIndex = args.indexOf("--tool");
    const tool = toolIndex >= 0 ? args[toolIndex + 1] : undefined;
    if (!tool) {
      console.error("Usage: publish.mjs base --tool <tool>");
      process.exit(1);
    }
    return { mode, tool };
  }

  console.error("Usage: publish.mjs <platform|base> [options]");
  process.exit(1);
}

function getVersion() {
  const version = process.env.RELEASE_VERSION;
  if (!version) {
    console.error("RELEASE_VERSION environment variable is required");
    process.exit(1);
  }
  return version;
}

/**
 * @param {{ tool?: string; os?: string; arch?: string; binary?: string }} options
 */
function publishPlatform(options) {
  const version = getVersion();
  const tool = assertTool(options.tool ?? "");
  const dir = NodePath.join(ROOT, `${tool}-${options.os}-${options.arch}`);
  const binDir = NodePath.join(dir, "bin");

  NodeFS.rmSync(dir, { recursive: true, force: true });
  NodeFS.mkdirSync(binDir, { recursive: true });
  NodeFS.writeFileSync(
    NodePath.join(dir, "package.json"),
    platformPackageJson({
      tool,
      os: options.os ?? "",
      arch: options.arch ?? "",
      version,
    }),
  );

  const destination = NodePath.join(binDir, binaryName(tool));
  NodeFS.copyFileSync(options.binary ?? "", destination);
  NodeFS.chmodSync(destination, 0o755);
  console.info(`Copied: ${options.binary} -> ${destination}`);

  publish(dir);
}

/**
 * @param {{ tool?: string }} options
 */
function publishBase(options) {
  const version = getVersion();
  const tool = assertTool(options.tool ?? "");
  const packageDir = NodePath.join(ROOT, tool);
  const distDir = NodePath.join(packageDir, "dist");

  NodeFS.rmSync(packageDir, { recursive: true, force: true });
  NodeFS.mkdirSync(distDir, { recursive: true });
  NodeFS.writeFileSync(
    NodePath.join(packageDir, "package.json"),
    basePackageJson({ tool, version }),
  );
  NodeFS.copyFileSync(NodePath.join(ROOT, "src", "bin.mjs"), NodePath.join(packageDir, "bin.mjs"));
  NodeFS.copyFileSync(
    NodePath.join(ROOT, "src", "const.mjs"),
    NodePath.join(packageDir, "const.mjs"),
  );
  NodeFS.copyFileSync(
    NodePath.join(ROOT, "src", "install.mjs"),
    NodePath.join(distDir, "postinstall.mjs"),
  );
  NodeFS.copyFileSync(NodePath.join(ROOT, "src", "const.mjs"), NodePath.join(distDir, "const.mjs"));

  console.info(`Assembled base package for ${tool}`);
  publish(packageDir);
}

/**
 * @param {NodeFS.PathLike} dir
 */
function publish(dir) {
  const dryRun = process.env.DRY_RUN === "1";
  const pkg = JSON.parse(
    NodeFS.readFileSync(NodePath.join(dir.toString(), "package.json"), "utf8"),
  );
  const tag = pkg.version.includes("-") ? "next" : "latest";

  if (dryRun) {
    console.info(`DRY RUN: would publish ${pkg.name}@${pkg.version} --tag=${tag}`);
    console.info(`  dir: ${dir}`);
    console.info(`  files: ${NodeFS.readdirSync(dir, { recursive: true }).join(", ")}`);
    return;
  }

  console.info(`Publishing ${pkg.name}@${pkg.version} --tag=${tag}`);
  NodeChildProcess.execSync(`npm publish --access public --tag ${tag}`, {
    stdio: "inherit",
    cwd: dir.toString(),
  });
}

const opts = parseArgs();
if (opts.mode === "platform") {
  publishPlatform(opts);
} else {
  publishBase(opts);
}
