"use strict";

const fs = require("node:fs");
const fsp = require("node:fs/promises");
const http = require("node:http");
const https = require("node:https");
const path = require("node:path");
const tar = require("tar");
const { binaryName, normalizeTarget } = require("../lib/platform");

const pkg = require("../package.json");

async function mkdirp(dir) {
  await fsp.mkdir(dir, { recursive: true });
}

async function download(url, destination) {
  const client = url.startsWith("http://") ? http : https;

  await new Promise((resolve, reject) => {
    const file = fs.createWriteStream(destination);
    const request = client.get(url, (response) => {
      if (
        response.statusCode &&
        response.statusCode >= 300 &&
        response.statusCode < 400 &&
        response.headers.location
      ) {
        file.close();
        fs.unlink(destination, () => {
          download(response.headers.location, destination)
            .then(resolve)
            .catch(reject);
        });
        return;
      }

      if (response.statusCode !== 200) {
        reject(
          new Error(`download failed: ${response.statusCode} ${response.statusMessage}`)
        );
        return;
      }

      response.pipe(file);
      file.on("finish", () => file.close(resolve));
    });

    request.on("error", (error) => {
      file.close();
      fs.unlink(destination, () => reject(error));
    });

    file.on("error", (error) => {
      file.close();
      fs.unlink(destination, () => reject(error));
    });
  });
}

async function install() {
  if (process.env.SYNCMD_SKIP_DOWNLOAD === "1") {
    return;
  }

  const target = normalizeTarget();
  const vendorDir = path.join(__dirname, "..", "vendor", target);
  const archivePath = path.join(vendorDir, "syncmd.tar.gz");
  const releaseBase =
    process.env.SYNCMD_BINARY_MIRROR ||
    "https://github.com/langware-labs/syncmd/releases/download";
  const versionTag = `v${pkg.version}`;
  const archiveName = `syncmd-${target}.tar.gz`;
  const archiveUrl = `${releaseBase}/${versionTag}/${archiveName}`;

  await mkdirp(vendorDir);
  await download(archiveUrl, archivePath);
  await tar.x({
    cwd: vendorDir,
    file: archivePath,
    strict: true,
  });
  await fsp.unlink(archivePath);
  await fsp.chmod(path.join(vendorDir, binaryName()), 0o755);
}

install().catch((error) => {
  console.error(`syncmd install failed: ${error.message}`);
  process.exit(1);
});
