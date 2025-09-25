# ws_client.py
from __future__ import annotations

import asyncio
import json
import socket
import struct
from typing import Any, Awaitable, Callable, Dict, Iterable, Tuple, Optional, Union

import numpy as np
import websockets

async def _tcp_probe_ipv4(host: str, port: int):
    infos = socket.getaddrinfo(host, port, family=socket.AF_INET, type=socket.SOCK_STREAM)
    af, socktype, proto, _, sa = infos[0]
    reader, writer = await asyncio.open_connection(host=sa[0], port=sa[1], family=af)
    writer.close()
    await writer.wait_closed()

MetaMap = Dict[str, Dict[str, Any]]
PacketHandler = Callable[[Dict[str, Any], np.ndarray, Optional[Dict[str, Any]]], Awaitable[None]]

async def start_data_ws(
    host: str,
    subscriptions: Iterable[Tuple[str, int]],
    on_packet: PacketHandler,
) -> None:
    """Connect to the data WebSocket, subscribe to topics, and dispatch packets."""
    url = f"ws://{host}:9000/ws/data"
    metadata: MetaMap = {}

    print("[ws_client] TCP probe:", host, 9000)
    await _tcp_probe_ipv4(host, 9000)
    print("[ws_client] TCP probe OK")
    print("[ws_client] connecting to:", url)

    async with websockets.connect(url, max_size=None) as ws:
        for topic, epoch in subscriptions:
            await ws.send(json.dumps({"type": "subscribe", "topic": topic, "epoch": epoch}))

        async for message in ws:
            kind, payload = _ws_data_received(message, metadata)
            if kind == "samples":
                header, samples, meta = payload
                result = on_packet(header, samples, meta)
                if asyncio.iscoroutine(result):
                    await result

def _ws_data_received(
    message: Union[str, bytes],
    metadata: MetaMap,
) -> Tuple[str, Any]:
    """Dispatch text vs. binary frames and keep metadata up to date."""
    if isinstance(message, str):
        obj = json.loads(message)
        if obj.get("message_type") == "meta_update":
            metadata[obj["topic"]] = obj["meta"]
            return "meta", obj
        return "control", obj

    view = memoryview(message)
    (header_len,) = struct.unpack(">I", view[:4])
    header = json.loads(view[4:4 + header_len].tobytes())
    payload = view[4 + header_len:].tobytes()
    samples = _ws_data_to_np_array(header, payload)
    meta = metadata.get(header["topic"])
    return "samples", (header, samples, meta)

def _ws_data_to_np_array(header: Dict[str, Any], payload: bytes) -> np.ndarray:
    """Decode the binary payload into a (batch_size, num_channels) NumPy array."""
    dtype = "<i4" if header.get("packet_type") == "RawI32" else "<f4"
    data = np.frombuffer(payload, dtype=dtype)
    channels = header.get("num_channels", 0) or 1
    batch = header.get("batch_size", 0) or max(1, len(data) // channels)
    return data.reshape(batch, channels, order="C")
