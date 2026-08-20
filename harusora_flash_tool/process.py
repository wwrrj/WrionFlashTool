from __future__ import annotations

import subprocess
import time
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path

from .log import Logger


@dataclass
class ProcessResult:
    code: int
    text: str
    stdout: str
    stderr: str


def run_process(
    file_path: str | Path,
    arguments: Sequence[str],
    *,
    logger: Logger | None = None,
    show_output: bool = False,
    timeout: float | None = None,
    on_output_line: Callable[[str], None] | None = None,
    heartbeat_seconds: float = 1.0,
) -> ProcessResult:
    """Run a native process with list args (no shell). Stream-friendly capture."""
    exe = str(file_path)
    args = [exe, *[str(a) for a in arguments]]
    if logger:
        logger.write(" ".join(args))

    proc = subprocess.Popen(
        args,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        stdin=subprocess.DEVNULL,
        creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0),
        text=True,
        encoding="utf-8",
        errors="replace",
    )

    stdout_chunks: list[str] = []
    stderr_chunks: list[str] = []
    start = time.monotonic()
    last_beat = start
    waiting_dots = False

    assert proc.stdout is not None and proc.stderr is not None

    import sys

    use_select = sys.platform != "win32"
    if use_select:
        import select

    def _drain(stream, bucket: list[str]) -> None:
        if stream is None:
            return
        try:
            data = stream.read()
        except Exception:
            return
        if data:
            bucket.append(data)

    if use_select:
        while proc.poll() is None:
            if timeout is not None and (time.monotonic() - start) > timeout:
                proc.kill()
                raise TimeoutError(f"Process timed out after {timeout}s: {exe}")
            rlist, _, _ = select.select([proc.stdout, proc.stderr], [], [], heartbeat_seconds)
            for stream in rlist:
                line = stream.readline()
                if not line:
                    continue
                if stream is proc.stdout:
                    stdout_chunks.append(line)
                else:
                    stderr_chunks.append(line)
                cleaned = line.rstrip("\r\n")
                if on_output_line:
                    on_output_line(cleaned)
                elif show_output and cleaned:
                    print(f"     {cleaned}")
            if show_output and (time.monotonic() - last_beat) >= heartbeat_seconds and not rlist:
                print(".", end="", flush=True)
                waiting_dots = True
                last_beat = time.monotonic()
        _drain(proc.stdout, stdout_chunks)
        _drain(proc.stderr, stderr_chunks)
    else:
        # Windows: communicate with optional timeout; for long flash ops show heartbeat.
        # Use threads for live output when show_output is needed.
        import threading

        def _reader(stream, bucket: list[str]) -> None:
            try:
                for line in iter(stream.readline, ""):
                    bucket.append(line)
                    cleaned = line.rstrip("\r\n")
                    if on_output_line:
                        on_output_line(cleaned)
                    elif show_output and cleaned:
                        print(f"     {cleaned}")
            except Exception:
                pass

        t_out = threading.Thread(target=_reader, args=(proc.stdout, stdout_chunks), daemon=True)
        t_err = threading.Thread(target=_reader, args=(proc.stderr, stderr_chunks), daemon=True)
        t_out.start()
        t_err.start()
        while proc.poll() is None:
            if timeout is not None and (time.monotonic() - start) > timeout:
                proc.kill()
                raise TimeoutError(f"Process timed out after {timeout}s: {exe}")
            if show_output and (time.monotonic() - last_beat) >= heartbeat_seconds:
                # Only print dots if no recent line output (threads may print lines)
                print(".", end="", flush=True)
                waiting_dots = True
                last_beat = time.monotonic()
            time.sleep(0.2)
        t_out.join(timeout=5)
        t_err.join(timeout=5)

    if waiting_dots:
        print()

    code = proc.returncode if proc.returncode is not None else -1
    stdout = "".join(stdout_chunks)
    stderr = "".join(stderr_chunks)
    text = "\n".join(p.strip("\r\n") for p in (stdout, stderr) if p and p.strip())
    if logger and text:
        logger.write(text)
    if show_output and text and not on_output_line:
        # Lines may already have been printed on Windows via threads
        pass
    return ProcessResult(code=code, text=text, stdout=stdout, stderr=stderr)
