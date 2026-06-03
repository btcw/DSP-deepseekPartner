import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";

const forwardedArgs = process.argv.slice(2);
const env = { ...process.env };
const projectRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

if (process.platform === "darwin") {
  if (!env.LC_ALL || env.LC_ALL === "C.UTF-8") {
    env.LC_ALL = "en_US.UTF-8";
  }
  if (!env.LC_CTYPE || env.LC_CTYPE === "C.UTF-8") {
    env.LC_CTYPE = env.LC_ALL;
  }
  if (!env.LANG || env.LANG === "C.UTF-8") {
    env.LANG = env.LC_ALL;
  }
}

const bin = process.platform === "win32" ? "tauri.cmd" : "tauri";

if (process.platform === "darwin" && shouldUseStableMacPackaging(forwardedArgs)) {
  await buildStableMacPackage(forwardedArgs);
} else {
  await run(bin, ["build", ...forwardedArgs]);
}

function shouldUseStableMacPackaging(args) {
  if (args.includes("--no-bundle")) {
    return false;
  }

  const bundles = readBundles(args);
  return bundles === null || bundles.includes("app") || bundles.includes("dmg");
}

function readBundles(args) {
  const index = args.findIndex((arg) => arg === "--bundles" || arg === "-b");
  if (index !== -1) {
    return splitBundles(args[index + 1] ?? "");
  }

  const inline = args.find((arg) => arg.startsWith("--bundles="));
  if (inline) {
    return splitBundles(inline.slice("--bundles=".length));
  }

  return null;
}

function splitBundles(value) {
  return value
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
}

function stripBundleArgs(args) {
  const stripped = [];
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--bundles" || arg === "-b") {
      index += 1;
      continue;
    }
    if (arg.startsWith("--bundles=")) {
      continue;
    }
    stripped.push(arg);
  }
  return stripped;
}

async function buildStableMacPackage(args) {
  await run(bin, ["build", ...stripBundleArgs(args), "--bundles", "app"]);

  const config = JSON.parse(fs.readFileSync(path.join(projectRoot, "src-tauri", "tauri.conf.json"), "utf8"));
  const productName = config.productName;
  const version = config.version;
  const arch = process.arch === "arm64" ? "aarch64" : process.arch;
  const bundleDir = path.join(projectRoot, "src-tauri", "target", "release", "bundle");
  const appPath = path.join(bundleDir, "macos", `${productName}.app`);
  const dmgDir = path.join(bundleDir, "dmg");
  const dmgPath = path.join(dmgDir, `${productName}_${version}_${arch}.dmg`);

  fs.mkdirSync(dmgDir, { recursive: true });
  await ensureValidMacSignature(appPath);
  await createSimpleDmg({ appPath, dmgPath, productName });
  await verifyDmg(dmgPath);

  console.log(`    Finished macOS bundles at:
        ${appPath}
        ${dmgPath}`);
}

async function ensureValidMacSignature(appPath) {
  const result = await run("codesign", ["--verify", "--deep", "--strict", "--verbose=2", appPath], {
    allowFailure: true,
  });
  if (result === 0) {
    return;
  }

  console.log("     Signing macOS app with an ad-hoc local signature");
  await run("codesign", ["--force", "--deep", "--sign", "-", appPath]);
}

async function createSimpleDmg({ appPath, dmgPath, productName }) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), "deepseek-gateway-dmg-"));
  try {
    const stagedAppPath = path.join(tempDir, `${productName}.app`);
    await run("ditto", [appPath, stagedAppPath]);
    fs.symlinkSync("/Applications", path.join(tempDir, "Applications"));
    fs.rmSync(dmgPath, { force: true });
    await run("hdiutil", [
      "create",
      "-volname",
      productName,
      "-srcfolder",
      tempDir,
      "-ov",
      "-format",
      "UDZO",
      dmgPath,
    ]);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}

async function verifyDmg(dmgPath) {
  for (let attempt = 1; attempt <= 6; attempt += 1) {
    const code = await run("hdiutil", ["verify", dmgPath], { allowFailure: true });
    if (code === 0) {
      return;
    }
    if (attempt === 6) {
      throw new Error(`hdiutil verify failed after ${attempt} attempts`);
    }
    await sleep(2000 * attempt);
  }
}

function sleep(ms) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, {
      cwd: projectRoot,
      env,
      shell: process.platform === "win32",
      stdio: "inherit",
    });

    child.on("exit", (code, signal) => {
      if (signal) {
        reject(new Error(`${command} exited with signal ${signal}`));
        return;
      }
      if (code === 0 || options.allowFailure) {
        resolve(code ?? 0);
        return;
      }
      reject(new Error(`${command} exited with code ${code ?? 1}`));
    });
  });
}
