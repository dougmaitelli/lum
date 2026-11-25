"""Entry point for Lum's pinned hrx-loom 0.3.0 venv, not a system Python tool."""

import sys
import time

from loom.model import fetch_main
from loom.model.fetch_console import ConsoleProgress


class LiveConsole(ConsoleProgress):
    def __call__(self, event):
        # The 0.3.0 presenter accumulates per-file counters on every event but
        # suppresses repeated lines for 30s. Open its display gate at 0.5s;
        # keep its counter accounting, escaping, phase and warning handling.
        if time.monotonic() - self.last_time >= 0.5:
            self.last_time = float("-inf")
        super().__call__(event)


if __name__ == "__main__":
    fetch_main.ConsoleProgress = LiveConsole
    sys.exit(fetch_main.main())
