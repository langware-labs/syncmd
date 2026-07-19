"use strict";

const path = require("node:path");

function normalizeTarget() {
  const platformMap = {
    darwin: "apple-darwin",
    linux: "unknown-linux-gnu",
    win32: "pc-windows-msvc",
  };
  const archMap = {
    arm64: "aarch64",
    x64: "x86_64",
  };

  const platform = platformMap[process.platform];
  const arch = archMap[process.arch];

  if (!platform || !arch) {
    throw new Error(
      `syncmd does not publish binaries for ${process.platform}/${process.arch}`
    );
  }

  return `${arch}-${platform}`;
}

function binaryName() {
  return process.platform === "win32" ? "syncmd.exe" : "syncmd";
}

function binaryPath() {
  if (process.env.SYNCMD_BINARY_PATH) {
    return process.env.SYNCMD_BINARY_PATH;
  }

  return path.join(__dirname, "..", "vendor", normalizeTarget(), binaryName());
}

module.exports = {
  binaryName,
  binaryPath,
  normalizeTarget,
};
