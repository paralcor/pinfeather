import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// Parse the real file with Windows PowerShell's file decoder. Never execute it.
describe.skipIf(process.platform !== "win32")("personal launcher compatibility", () => {
  it("Windows PowerShell 5.1 parses the distributed launcher", () => {
    const target = fileURLToPath(new URL("./start-personal.ps1", import.meta.url));
    const code = "$tokens=$null; $errors=$null; [System.Management.Automation.Language.Parser]::ParseFile($env:PINFEATHER_LAUNCHER_TEST_PATH,[ref]$tokens,[ref]$errors) | Out-Null; if ($errors.Count -ne 0) { $errors | ForEach-Object { [Console]::Error.WriteLine($_.ErrorId) }; exit 1 }";
    const child = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", code], { env: { ...process.env, PINFEATHER_LAUNCHER_TEST_PATH: target }, encoding: "utf8", windowsHide: true, timeout: 15000 });
    expect(child.error).toBeUndefined();
    expect(child.status, child.stderr).toBe(0);
  });
});
