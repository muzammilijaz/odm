// Cross-platform Tauri bundle staging. The old PowerShell-only version made
// macOS builds impossible; this script intentionally keeps the same Windows
// resources while using the native host produced for the current target.
import { cp, mkdir, rm, copyFile, access, chmod } from "node:fs/promises";
import { join, resolve } from "node:path";
import { platform } from "node:os";
import { spawnSync } from "node:child_process";

const tauriDir = resolve(new URL(".", import.meta.url).pathname);
const repoRoot = resolve(tauriDir, "..");
const extension = join(repoRoot, "extension");
const resources = join(tauriDir, "src-tauri", "resources");
const releaseDir = join(repoRoot, "target", "release");
const exe = platform() === "win32" ? ".exe" : "";
const targetTriple = process.env.TAURI_ENV_TARGET_TRIPLE ?? process.env.TARGET ?? "";
const host = join(releaseDir, `odm-native-host${exe}`);

const run = (command, args) => {
  const result = spawnSync(command, args, { cwd: repoRoot, stdio: "inherit", shell: platform() === "win32" });
  if (result.status !== 0) throw new Error(`${command} failed`);
};

if (targetTriple === "universal-apple-darwin") {
  const intel = join(repoRoot, "target", "x86_64-apple-darwin", "release", "odm-native-host");
  const arm = join(repoRoot, "target", "aarch64-apple-darwin", "release", "odm-native-host");
  for (const target of ["x86_64-apple-darwin", "aarch64-apple-darwin"]) {
    run("cargo", ["build", "-p", "odm-native-host", "--release", "--target", target]);
  }
  run("lipo", ["-create", intel, arm, "-output", host]);
} else {
  try { await access(host); } catch {
    run("cargo", ["build", "-p", "odm-native-host", "--release"]);
  }
}

await rm(join(resources, "extension"), { recursive: true, force: true });
await mkdir(join(resources, "extension"), { recursive: true });
for (const entry of ["manifest.json", "background.js", "content.js", "content.css", "popup.html", "popup.js", "icons"]) {
  await cp(join(extension, entry), join(resources, "extension", entry), { recursive: true });
}
await copyFile(host, join(resources, `odm-native-host${exe}`));
if (platform() !== "win32") await chmod(join(resources, "odm-native-host"), 0o755);
await copyFile(join(extension, "native-host-manifest", "com.odm.nativehost.json"), join(resources, "com.odm.nativehost.template.json"));

// Windows NSIS still gets its existing post-install registration hook. macOS
// registration is performed by the app at startup (see native_messaging.rs).
if (platform() === "win32") {
  await copyFile(join(tauriDir, "windows", "register-native-host.ps1"), join(resources, "register-native-host.ps1"));
}
