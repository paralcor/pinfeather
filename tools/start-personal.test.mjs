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

  it.each([false, true])("supplies the expected startup arguments (resume=%s)", resume => {
    const target = fileURLToPath(new URL("./start-personal.ps1", import.meta.url));
    // Shadow the process and path cmdlets so the launcher cannot touch personal data.
    const code = [
      "function Resolve-Path { param($LiteralPath) [pscustomobject]@{Path=$LiteralPath} }",
      "function Start-Process { param($FilePath,$ArgumentList,$WindowStyle) ConvertTo-Json -Compress -InputObject @($FilePath,$WindowStyle,$ArgumentList) }",
      "$flags=@{ Executable='D:\\Synthetic\\trial.exe' }",
      resume ? "$flags.ResumeCapture=$true" : "$null=$null",
      "& $env:PINFEATHER_LAUNCHER_TEST_PATH @flags",
    ].join("; ");
    const child = spawnSync("powershell.exe", ["-NoProfile", "-NonInteractive", "-Command", code], { env: { ...process.env, PINFEATHER_LAUNCHER_TEST_PATH: target }, encoding: "utf8", windowsHide: true, timeout: 15000 });
    expect(child.error).toBeUndefined();
    expect(child.status, child.stderr).toBe(0);
    const args = ["--data-dir", "D:\\Develop\\PinfeatherData\\personal"];
    if (!resume) args.push("--start-paused");
    expect(JSON.parse(child.stdout)).toEqual(["D:\\Synthetic\\trial.exe", "Hidden", args]);
  });
});
