#!/usr/bin/env node
import { mkdir, access, copyFile, rm, readdir, readFile, writeFile } from "node:fs/promises";
import { createWriteStream, createReadStream } from "node:fs";
import { pipeline } from "node:stream/promises";
import { createGunzip } from "node:zlib";
import { extract } from "tar";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.join(__dirname, "..");
const RESOURCES = path.join(ROOT, "src-tauri", "resources", "pdfium");
// Must match pdfium-render 0.8.37's pdfium_latest / pdfium_7543 bindings.
const PDFIUM_VERSION = "chromium/7543";
const VERSION_MARKER = path.join(RESOURCES, "VERSION");

const PLATFORM_MAP = {
  "darwin-arm64": "mac-arm64",
  "darwin-x64": "mac-x64",
  "linux-x64": "linux-x64",
  "win32-x64": "win-x64",
};

const LIB_NAMES = {
  "darwin-arm64": "libpdfium.dylib",
  "darwin-x64": "libpdfium.dylib",
  "linux-x64": "libpdfium.so",
  "win32-x64": "pdfium.dll",
};

const key = `${process.platform}-${process.arch}`;
const libName = LIB_NAMES[key];
const platformKey = PLATFORM_MAP[key];

if (!libName || !platformKey) {
  console.warn(`[fetch-pdfium] Unsupported platform ${key}, skipping.`);
  process.exit(0);
}

const destPath = path.join(RESOURCES, libName);

async function exists(p) {
  try {
    await access(p);
    return true;
  } catch {
    return false;
  }
}

async function findLib(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const found = await findLib(full);
      if (found) return found;
    } else if (
      entry.name === libName ||
      entry.name.endsWith(".dylib") ||
      entry.name.endsWith(".so") ||
      entry.name.endsWith(".dll")
    ) {
      return full;
    }
  }
  return null;
}

async function installedVersion() {
  try {
    return (await readFile(VERSION_MARKER, "utf8")).trim();
  } catch {
    return "";
  }
}

async function main() {
  await mkdir(RESOURCES, { recursive: true });
  const current = await installedVersion();
  if ((await exists(destPath)) && current === PDFIUM_VERSION) {
    console.log(`[fetch-pdfium] Already present: ${destPath} (${PDFIUM_VERSION})`);
    return;
  }
  if (await exists(destPath) && current !== PDFIUM_VERSION) {
    console.log(`[fetch-pdfium] Replacing ${current || "unknown"} with ${PDFIUM_VERSION}`);
    await rm(destPath, { force: true });
  }

  const tag = encodeURIComponent(PDFIUM_VERSION);
  const url = `https://github.com/bblanchon/pdfium-binaries/releases/download/${tag}/pdfium-${platformKey}.tgz`;

  console.log(`[fetch-pdfium] Downloading ${url}`);
  const res = await fetch(url);
  if (!res.ok) {
    console.warn(`[fetch-pdfium] Download failed (${res.status}), PDF rendering will need manual setup.`);
    process.exit(0);
  }

  const tmpTgz = path.join(RESOURCES, "pdfium.tgz");
  await pipeline(res.body, createWriteStream(tmpTgz));

  const tmpDir = path.join(RESOURCES, "extract");
  await mkdir(tmpDir, { recursive: true });
  await pipeline(createReadStream(tmpTgz), createGunzip(), extract({ cwd: tmpDir }));

  const found = await findLib(tmpDir);
  if (found) {
    await copyFile(found, destPath);
    await writeFile(VERSION_MARKER, `${PDFIUM_VERSION}\n`);
    console.log(`[fetch-pdfium] Installed ${destPath} (${PDFIUM_VERSION})`);
  } else {
    console.warn("[fetch-pdfium] Could not locate library in archive.");
  }

  await rm(tmpTgz, { force: true });
  await rm(tmpDir, { recursive: true, force: true });
}

main().catch((e) => {
  console.warn("[fetch-pdfium] Non-fatal error:", e.message);
  process.exit(0);
});
