from __future__ import annotations

from datetime import datetime
from pathlib import Path


class Logger:
    def __init__(self) -> None:
        self.path: Path | None = None

    def start(self, log_dir: Path, prefix: str = "harusora-flash") -> Path:
        log_dir.mkdir(parents=True, exist_ok=True)
        stamp = datetime.now().strftime("%Y%m%d-%H%M%S")
        self.path = log_dir / f"{prefix}-{stamp}.log"
        header = f"HarusoraFlashTool log - {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"
        self.path.write_text(header + "\n", encoding="utf-8")
        return self.path

    def write(self, text: str) -> None:
        if not self.path or not text:
            return
        ts = datetime.now().strftime("%H:%M:%S")
        line = text.rstrip("\r\n")
        with self.path.open("a", encoding="utf-8", errors="replace") as fh:
            for part in line.splitlines() or [""]:
                fh.write(f"[{ts}] {part}\n")

    def fatal(self, exc: BaseException) -> None:
        self.write(f"FATAL: {exc!r}")
