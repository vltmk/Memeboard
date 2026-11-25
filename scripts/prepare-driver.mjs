import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { pipeline } from 'node:stream/promises';
import { Readable } from 'node:stream';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const FILE_NAME = 'USBip-0.9.8.1-x64.exe';
const URL = `https://github.com/vadimgrn/usbip-win2/releases/download/v.0.9.8.1/${FILE_NAME}`;
const EXPECTED_SHA256 = '38cad6d4432b52d5bb9409d9ad03b72fdffc4ada4cd3a48fbeca1a2752a8518a';

const outDir = path.resolve(__dirname, '..', 'src-tauri', 'resources', 'driver');
const destPath = path.join(outDir, FILE_NAME);
const localCachePath = process.env.MEMEBOARD_DRIVER_CACHE
  ? path.join(process.env.MEMEBOARD_DRIVER_CACHE, FILE_NAME)
  : null;

async function sha256File(filePath) {
  const hash = crypto.createHash('sha256');
  const stream = fs.createReadStream(filePath);
  for await (const chunk of stream) {
    hash.update(chunk);
  }
  return hash.digest('hex').toLowerCase();
}

async function isValid(filePath) {
  if (!fs.existsSync(filePath)) return false;
  return (await sha256File(filePath)) === EXPECTED_SHA256;
}

async function main() {
  if (process.platform !== 'win32' && process.env.MEMEBOARD_REQUIRE_WIN_DRIVER !== '1') {
    // Non-Windows build without Windows driver requirement: ensure directory exists
    fs.mkdirSync(outDir, { recursive: true });
    return;
  }

  fs.mkdirSync(outDir, { recursive: true });

  if (await isValid(destPath)) {
    console.log(`[prepare-driver] Verified ${FILE_NAME} (${EXPECTED_SHA256.slice(0, 12)}...).`);
    return;
  }

  if (localCachePath && (await isValid(localCachePath))) {
    fs.copyFileSync(localCachePath, destPath);
    console.log(`[prepare-driver] Copied verified ${FILE_NAME} from local cache.`);
    return;
  }

  console.log(`[prepare-driver] Downloading ${FILE_NAME} from vadimgrn/usbip-win2 v.0.9.8.1...`);
  const res = await fetch(URL, { redirect: 'follow' });
  if (!res.ok || !res.body) {
    throw new Error(`Failed to download ${URL}: HTTP ${res.status} ${res.statusText}`);
  }

  const tmpPath = `${destPath}.part`;
  await pipeline(Readable.fromWeb(res.body), fs.createWriteStream(tmpPath));

  const actual = await sha256File(tmpPath);
  if (actual !== EXPECTED_SHA256) {
    fs.rmSync(tmpPath, { force: true });
    throw new Error(`SHA256 mismatch for ${FILE_NAME}: expected ${EXPECTED_SHA256}, got ${actual}`);
  }

  fs.renameSync(tmpPath, destPath);
  console.log(`[prepare-driver] Verified and staged ${FILE_NAME}.`);
}

main().catch((err) => {
  console.error('[prepare-driver] Error:', err);
  process.exit(1);
});
