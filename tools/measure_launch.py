"""Explicit cold-run launcher. No hotkeys, clipboard operations, or retries."""
import argparse
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


def qpc():
    value = ctypes.c_longlong()
    if not ctypes.windll.kernel32.QueryPerformanceCounter(ctypes.byref(value)) or value.value <= 0:
        raise RuntimeError("QPC unavailable")
    return value.value


def frequency():
    value = ctypes.c_longlong()
    if not ctypes.windll.kernel32.QueryPerformanceFrequency(ctypes.byref(value)) or value.value <= 0:
        raise RuntimeError("QPC frequency unavailable")
    return value.value


def checked_path(value, directory=False):
    path = Path(value)
    if not path.is_absolute() or str(path) != str(path.resolve(strict=False)):
        raise ValueError("canonical absolute path required")
    for parent in (path if directory else path.parent).parents:
        if parent.stat().st_file_attributes & 0x400:
            raise ValueError("reparse parent forbidden")
    parent = path if directory else path.parent
    if not parent.is_dir() or parent.stat().st_file_attributes & 0x400:
        raise ValueError("existing plain parent required")
    return path


def launch(exe, data_dir, output, receipt, run_id, *, tick=qpc, hz=frequency, spawn=subprocess.Popen):
    if not re.fullmatch(r"[A-Za-z0-9_-]{1,64}", run_id):
        raise ValueError("invalid run id")
    exe, data_dir, output, receipt = (checked_path(exe), checked_path(data_dir, True), checked_path(output), checked_path(receipt))
    if not exe.is_file() or exe.stat().st_file_attributes & 0x400 or output.exists() or receipt.exists() or output == receipt:
        raise ValueError("explicit candidate and unused output paths required")
    if output.is_relative_to(data_dir) or receipt.is_relative_to(data_dir):
        raise ValueError("evidence must be outside data directory")
    candidate_hash = hashlib.sha256(exe.read_bytes()).hexdigest()
    measured_hz = hz()
    with receipt.open("x", encoding="utf-8") as stream:
        # All filesystem/hash work precedes the native clock/start pair.
        start = tick()
        process = spawn([str(exe), "--data-dir", str(data_dir), "--start-paused", "--measure-run-id", run_id,
                         "--measure-output", str(output), "--measure-cold-start-qpc", str(start),
                         "--measure-qpc-frequency", str(measured_hz)], shell=False)
        json.dump({"schema": 1, "run_id": run_id, "source": "python_subprocess_popen", "candidate_sha256": candidate_hash,
                   "cold_start_qpc": str(start), "qpc_frequency": str(measured_hz), "pid": process.pid}, stream)
        stream.write("\n")
        stream.flush()
        os.fsync(stream.fileno())
    return process


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("exe", "data-dir", "output", "launch-receipt", "run-id"):
        parser.add_argument("--" + name, required=True)
    args = parser.parse_args()
    launch(args.exe, args.data_dir, args.output, args.launch_receipt, args.run_id)
