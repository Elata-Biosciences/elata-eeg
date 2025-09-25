#!/usr/bin/env python3
# print_data.py
from __future__ import annotations

import asyncio
import socket
from typing import Dict, Any, Optional

import numpy as np
from ws_client import start_data_ws

async def handle_packet(header: Dict[str, Any], samples: np.ndarray, meta: Optional[Dict[str, Any]]):
    """
    Simple packet handler: print header info and sample values.
    """
    topic = header.get("topic", "?")
    fs = header.get("fs", "unknown")
    shape = samples.shape

    print(f"[{topic}] fs={fs} Hz, samples={shape}")
    if shape[0] > 0:
        print("First row:", samples[0, :])

async def main():
    host = "raspberrypi.local"   # change if needed
    # Resolve to IPv4 to match the TCP probe:
    host = socket.gethostbyname(host)
    topic = "eeg_voltage"
    epoch = 1

    print(f"Connecting to ws://{host}:9000/ws/data, topic={topic}, epoch={epoch}")
    await start_data_ws(host, [(topic, epoch)], handle_packet)

if __name__ == "__main__":
    asyncio.run(main())
