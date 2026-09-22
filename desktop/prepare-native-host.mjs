// Build the native messaging host before Tauri dev starts. Registration is
// handled by the running app on macOS and Windows; this is intentionally
// cross-platform so `npm run tauri:dev` works on an Intel Mac too.
import { spawnSync } from "node:child_process";
import { platform } from "node:os";
const result = spawnSync("cargo", ["build", "-p", "odm-native-host"], {
  cwd: new URL("..", import.meta.url), stdio: "inherit", shell: platform() === "win32"
});
if (result.status !== 0) process.exit(result.status ?? 1);
