import {
  packageName,
  TOOL_METADATA,
  platformPackageName,
  BINARY_DISTRIBUTION_PACKAGES,
} from "./const.mjs";

/**
 * @param {{ tool: string, os: string, arch: string, version: string }} opts
 * @returns {string}
 */
export function platformPackageJson({ tool, os, arch, version }) {
  return (
    JSON.stringify(
      {
        name: platformPackageName(tool, os, arch),
        version,
        description: `Platform-specific binary for ${packageName(tool)}`,
        license: "(MIT OR Apache-2.0)",
        os: [os],
        cpu: [arch],
        files: ["bin"],
        preferUnplugged: true,
        repository: {
          type: "git",
          directory: `crates/${tool}`,
          url: "https://github.com/tempoxyz/wallet",
        },
      },
      null,
      2,
    ) + "\n"
  );
}

/**
 * @param {{ tool: string, version: string }} opts
 * @returns {string}
 */
export function basePackageJson({ tool, version }) {
  const optionalDependencies = {};

  for (const [os, arches] of Object.entries(BINARY_DISTRIBUTION_PACKAGES)) {
    for (const arch of Object.keys(arches)) {
      optionalDependencies[platformPackageName(tool, os, arch)] = version;
    }
  }

  return (
    JSON.stringify(
      {
        name: packageName(tool),
        version,
        type: "module",
        description: TOOL_METADATA[tool].description,
        bin: {
          [tool]: "./bin.mjs",
        },
        files: ["bin.mjs", "const.mjs", "dist"],
        scripts: {
          postinstall: "node ./dist/postinstall.mjs",
        },
        optionalDependencies,
        publishConfig: {
          access: "public",
        },
        repository: {
          type: "git",
          url: "https://github.com/tempoxyz/wallet",
        },
        license: "(MIT OR Apache-2.0)",
      },
      null,
      2,
    ) + "\n"
  );
}
