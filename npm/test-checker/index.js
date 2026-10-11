const { existsSync } = require("node:fs");
const { dirname, join } = require("node:path");

const PACKAGE_NAME = "@bacondroid/test-checker";

const SUPPORTED_PLATFORMS = [
  "darwin-arm64",
  "darwin-x64",
  "linux-arm64",
  "linux-x64",
  "win32-x64",
];

function getPlatformKey(platform = process.platform, arch = process.arch) {
  return `${platform}-${arch === "x64" ? "x64" : arch}`;
}

function getBinaryName(platform = process.platform) {
  return platform === "win32" ? "test-checker.exe" : "test-checker";
}

function getPlatformPackageName(platformKey) {
  return `${PACKAGE_NAME}-${platformKey}`;
}

function resolvePlatformPackageBinary(platformKey, binaryName, baseDir) {
  try {
    const manifestPath = require.resolve(`${getPlatformPackageName(platformKey)}/package.json`, {
      paths: [baseDir],
    });
    return join(dirname(manifestPath), "bin", binaryName);
  } catch (error) {
    if (error && error.code === "MODULE_NOT_FOUND") {
      return null;
    }
    throw error;
  }
}

function getBinaryPath(options = {}) {
  const platform = options.platform || process.platform;
  const arch = options.arch || process.arch;
  const platformKey = getPlatformKey(platform, arch);

  if (!SUPPORTED_PLATFORMS.includes(platformKey)) {
    throw new Error(
      `Unsupported platform: ${platform}-${arch}. ` +
        `Supported: ${SUPPORTED_PLATFORMS.join(", ")}`
    );
  }

  const binaryName = getBinaryName(platform);
  const baseDir = options.baseDir || __dirname;
  const platformPackageBinaryPath = resolvePlatformPackageBinary(platformKey, binaryName, baseDir);

  if (platformPackageBinaryPath && existsSync(platformPackageBinaryPath)) {
    return platformPackageBinaryPath;
  }

  const bundledBinaryPath = join(baseDir, "vendor", platformKey, binaryName);

  if (existsSync(bundledBinaryPath)) {
    return bundledBinaryPath;
  }

  const localBinaryPath = join(baseDir, "bin", binaryName);
  if (existsSync(localBinaryPath)) {
    return localBinaryPath;
  }

  throw new Error(
    `test-checker binary not found. ` +
      `Expected the ${getPlatformPackageName(platformKey)} package, a bundled binary at ${bundledBinaryPath}, ` +
      `or a postinstall binary at ${localBinaryPath}. ` +
      `Try reinstalling: npm install @bacondroid/test-checker`
  );
}

module.exports = {
  getBinaryName,
  getBinaryPath,
  getPlatformKey,
  getPlatformPackageName,
  resolvePlatformPackageBinary,
  SUPPORTED_PLATFORMS,
};
