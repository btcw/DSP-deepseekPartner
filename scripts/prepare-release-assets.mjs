import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const pkg = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));
const releaseName = "DSP-deepseekPartner";
const outDir = path.join(root, "release-assets");

fs.rmSync(outDir, { recursive: true, force: true });
fs.mkdirSync(outDir, { recursive: true });

if (process.platform === "darwin") {
  copyFirst(
    path.join(root, "src-tauri", "target", "release", "bundle", "dmg"),
    ".dmg",
    `${releaseName}_${pkg.version}_macos-arm64.dmg`,
  );
} else if (process.platform === "win32") {
  copyFirst(
    path.join(root, "src-tauri", "target", "release", "bundle", "nsis"),
    ".exe",
    `${releaseName}_${pkg.version}_windows-x64-setup.exe`,
  );
  copyFirst(
    path.join(root, "src-tauri", "target", "release", "bundle", "msi"),
    ".msi",
    `${releaseName}_${pkg.version}_windows-x64.msi`,
  );
} else {
  throw new Error(`Unsupported release platform: ${process.platform}`);
}

function copyFirst(dir, extension, targetName) {
  if (!fs.existsSync(dir)) {
    return;
  }

  const source = fs
    .readdirSync(dir)
    .filter((name) => name.endsWith(extension))
    .map((name) => path.join(dir, name))
    .sort()[0];

  if (!source) {
    return;
  }

  fs.copyFileSync(source, path.join(outDir, targetName));
  console.log(`Prepared ${targetName}`);
}
