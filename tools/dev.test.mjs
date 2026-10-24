import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative } from "node:path";
import { fileURLToPath } from "node:url";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const scriptPath = fileURLToPath(new URL("./dev.ps1", import.meta.url));
const repoRoot = dirname(dirname(scriptPath));
const marker = "PINFEATHER_TEST:";
const probeArgs = ["-NoProfile", "-NonInteractive", "-Command", "Get-ExecutionPolicy"];
const probeOptions = { encoding: "utf8", windowsHide: true, timeout: 15000 };
let shell = "pwsh.exe";
let unavailable = process.platform !== "win32" ? "Windows only" : "";
if (!unavailable) {
  let probe = spawnSync(shell, probeArgs, probeOptions);
  if (probe.error?.code === "ENOENT") {
    shell = "powershell.exe";
    probe = spawnSync(shell, probeArgs, probeOptions);
  }
  if (probe.error) throw probe.error;
  if (probe.status !== 0) throw new Error(probe.stderr);
  const policy = probe.stdout.trim();
  if (policy === "Restricted" || policy === "AllSigned") {
    unavailable = `${shell} policy ${policy} does not allow this unsigned script`;
  }
}

// Run the real launcher; substitute only npm and the port query. Guard filesystem
// writes so even a broken default cannot create data outside this test's root.
const bootstrap = `
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
function global:Get-NetTCPConnection {
    if ($env:PINFEATHER_TEST_BUSY -eq '1') {
        [pscustomobject]@{ OwningProcess = $PID }
    }
}
function global:New-Item {
    param([string]$ItemType, [switch]$Force, [string]$Path)
    $root = [IO.Path]::GetFullPath($env:PINFEATHER_TEST_ROOT).TrimEnd('\\') + '\\'
    $target = [IO.Path]::GetFullPath($Path)
    if (-not $target.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'launcher tried to write outside the isolated test root'
    }
    Microsoft.PowerShell.Management\\New-Item -ItemType $ItemType -Force:$Force -Path $Path
}
function global:npm {
    Write-Output ('${marker}' + (ConvertTo-Json -Compress -InputObject @{
        arguments = @($args)
        cwd = (Get-Location).Path
    }))
    $global:LASTEXITCODE = 0
}
if ($env:PINFEATHER_TEST_EXPLICIT -eq '1') {
    & $env:PINFEATHER_TEST_SCRIPT -DataDir $env:PINFEATHER_TEST_DATA_DIR
} else {
    & $env:PINFEATHER_TEST_SCRIPT
}
`;

describe.skipIf(Boolean(unavailable))(`Windows development launcher (${unavailable || shell})`, () => {
  let testRoot;

  beforeAll(() => {
    testRoot = mkdtempSync(join(tmpdir(), "pinfeather-dev-test-"));
  });

  afterAll(() => {
    // testRoot is the exact directory created by mkdtempSync above.
    if (testRoot) rmSync(testRoot, { recursive: true, force: true });
  });

  function launch(dataDir, busy = false) {
    const child = spawnSync(shell, ["-NoProfile", "-NonInteractive", "-Command", bootstrap], {
      cwd: testRoot,
      env: {
        ...process.env,
        TEMP: testRoot,
        TMP: testRoot,
        PINFEATHER_TEST_ROOT: testRoot,
        PINFEATHER_TEST_SCRIPT: scriptPath,
        PINFEATHER_TEST_EXPLICIT: dataDir === undefined ? "0" : "1",
        PINFEATHER_TEST_DATA_DIR: dataDir ?? "",
        PINFEATHER_TEST_BUSY: busy ? "1" : "0",
      },
      encoding: "utf8",
      windowsHide: true,
      timeout: 15000,
    });
    expect(child.error).toBeUndefined();
    const match = child.stdout.match(/^PINFEATHER_TEST:(.+)$/m);
    return { ...child, invocation: match ? JSON.parse(match[1]) : null };
  }

  function expectInvocation(child, dataDir) {
    expect(child.status, child.stdout + child.stderr).toBe(0);
    // PowerShell consumes the first -- when calling the npm function substitute.
    expect(child.invocation).toEqual({
      arguments: ["run", "tauri", "dev", "--", "--", "--data-dir", dataDir, "--start-paused"],
      cwd: repoRoot,
    });
    expect(existsSync(dataDir)).toBe(true);
  }

  it("creates a distinct data directory below the system temp directory on each default launch", () => {
    const first = launch();
    expect(first.status, first.stdout + first.stderr).toBe(0);
    const firstDir = first.invocation.arguments.at(-2);
    expect(relative(testRoot, firstDir)).toMatch(/^Pinfeather[\\/]dev[\\/][a-f0-9]{32}$/);
    expectInvocation(first, firstDir);

    const second = launch();
    expect(second.status, second.stdout + second.stderr).toBe(0);
    const secondDir = second.invocation.arguments.at(-2);
    expect(relative(testRoot, secondDir)).toMatch(/^Pinfeather[\\/]dev[\\/][a-f0-9]{32}$/);
    expect(secondDir).not.toBe(firstDir);
    expectInvocation(second, secondDir);
  });

  it("preserves an explicit absolute directory with spaces and Unicode", () => {
    const dataDir = join(testRoot, "explicit data 片羽");
    expectInvocation(launch(dataDir), dataDir);
  });

  it("passes an explicit directory with a trailing separator for the host to normalize", () => {
    const dataDir = join(testRoot, "trailing-separator") + "\\";
    expectInvocation(launch(dataDir), dataDir);
  });

  it.each(["\\\\?\\", "\\\\.\\"])("rejects device namespace prefix %j before creating data", (prefix) => {
    const target = join(testRoot, "device-namespace");
    const child = launch(prefix + target);
    expect(child.status, child.stdout + child.stderr).toBe(1);
    expect(child.invocation).toBeNull();
    expect(child.stdout).toContain("DataDir");
    expect(existsSync(target)).toBe(false);
  });

  it.each(["", "relative-data", "C:relative-data", "\\root-relative"])(
    "rejects invalid directory %j before invoking npm",
    (dataDir) => {
      const child = launch(dataDir);
      expect(child.status, child.stdout + child.stderr).toBe(1);
      expect(child.invocation).toBeNull();
      expect(child.stdout).toContain("DataDir");
      expect(child.stderr).toBe("");
    },
  );

  it("stops before creating data or invoking npm when the dev port is occupied", () => {
    const dataDir = join(testRoot, "busy-port");
    const child = launch(dataDir, true);
    expect(child.status, child.stdout + child.stderr).toBe(1);
    expect(child.invocation).toBeNull();
    expect(child.stdout).toContain("1420");
    expect(existsSync(dataDir)).toBe(false);
  });
});
