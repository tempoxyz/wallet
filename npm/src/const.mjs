/** @type {readonly string[]} */
export const TOOLS = Object.freeze(["tempo-wallet", "tempo-request"]);

export const TOOL_METADATA = Object.freeze({
  "tempo-wallet": {
    description: "Manage your Tempo Wallet",
  },
  "tempo-request": {
    description: "Make an HTTP request",
  },
});

/** @type {(tool: string) => string} */
export const binaryName = (tool) => (process.platform === "win32" ? `${tool}.exe` : tool);

export const BINARY_DISTRIBUTION_PACKAGES = Object.freeze({
  darwin: Object.freeze({
    x64: "x64",
    arm64: "arm64",
  }),
  linux: Object.freeze({
    x64: "x64",
    arm64: "arm64",
  }),
});

export function assertTool(tool) {
  if (!TOOLS.includes(tool)) {
    throw new Error(`Unsupported tool: ${tool}. Supported: ${TOOLS.join(", ")}`);
  }
  return tool;
}

export function packageName(tool) {
  return `@tempoxyz/${assertTool(tool)}`;
}

export function toolFromPackageName(name) {
  if (!name.startsWith("@tempoxyz/")) {
    return null;
  }

  const tool = name.slice("@tempoxyz/".length);
  return TOOLS.includes(tool) ? tool : null;
}

export function platformPackageName(tool, os, arch) {
  assertTool(tool);

  if (!BINARY_DISTRIBUTION_PACKAGES[os]?.[arch]) {
    throw new Error(
      `Unsupported platform: ${os}-${arch}. ` +
        `Supported: ${Object.entries(BINARY_DISTRIBUTION_PACKAGES)
          .flatMap(([platform, archs]) => Object.keys(archs).map((cpu) => `${platform}-${cpu}`))
          .join(", ")}`,
    );
  }

  return `${packageName(tool)}-${os}-${arch}`;
}

/**
 * Resolves the platform-specific package name for the current OS + arch.
 *
 * @param {string} tool
 * @returns {string}
 */
export function platformPackage(tool) {
  return platformPackageName(tool, process.platform, process.arch);
}
