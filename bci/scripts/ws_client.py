# ws_client.py
from __future__ import annotations

import asyncio
import json
import socket
import struct
from typing import Any, Awaitable, Callable, Dict, Iterable, Tuple, Optional, Union

import numpy as np
import websockets

async def _tcp_probe(host: str, port: int, timeout: float = 2.0):
    """
    Try opening a TCP connection to host:port using any address family (IPv4/IPv6).
    Returns on first success, raises last error if all attempts fail.
    """
    infos = socket.getaddrinfo(host, port, family=socket.AF_UNSPEC, type=socket.SOCK_STREAM)
    last_err: Optional[Exception] = None
    for af, socktype, proto, _, sa in infos:
        try:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(host=sa[0], port=sa[1], family=af),
                timeout=timeout,
            )
            writer.close()
            await writer.wait_closed()
            return
        except Exception as e:
            last_err = e
            continue
    if last_err:
        raise last_err

MetaMap = Dict[str, Dict[str, Any]]
PacketHandler = Callable[[Dict[str, Any], np.ndarray, Optional[Dict[str, Any]]], Awaitable[None]]

async def start_data_ws(
    host: str,
    subscriptions: Iterable[Tuple[str, int]],
    on_packet: PacketHandler,
    port: int = 9000,
    scheme: str = "ws",
    probe: bool = True,
) -> None:
    """Connect to the data WebSocket, subscribe to topics, and dispatch packets."""
    metadata: MetaMap = {}

    target_host = host
    url = f"{scheme}://{target_host}:{port}/ws/data"

    if probe:
        print("[ws_client] TCP probe:", target_host, port)
        try:
            await _tcp_probe(target_host, port)
            print("[ws_client] TCP probe OK")
        except socket.gaierror as e:
            print(f"[ws_client] DNS resolution failed for {target_host}:{port} -> {e}")
            # Try sensible local fallbacks for typical mDNS hosts
            fallbacks = []
            if target_host in ("raspberrypi.local", "raspberrypi", "pi.local"):
                fallbacks = ["localhost", "127.0.0.1"]
            for fb in fallbacks:
                print(f"[ws_client] Trying fallback host: {fb}:{port}")
                try:
                    await _tcp_probe(fb, port)
                    target_host = fb
                    print(f"[ws_client] Fallback probe OK -> using {target_host}:{port}")
                    break
                except Exception as ef:
                    print(f"[ws_client] Fallback {fb} failed: {ef}")
            if target_host == host:
                # no fallback succeeded; re-raise original
                raise
        except OSError as e:
            print(f"[ws_client] TCP probe error for {target_host}:{port} -> {e}")
            raise

    url = f"{scheme}://{target_host}:{port}/ws/data"
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
