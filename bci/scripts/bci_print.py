#!/usr/bin/env python3
import asyncio
import websockets
import json

async def main():
    url = "ws://127.0.0.1:8766/output"
    print(f"Connecting to {url} ...")
    async with websockets.connect(url) as ws:
        async for msg in ws:
            try:
                obj = json.loads(msg)
                # just print up/down
                print(obj.get("y", "?"), flush=True)
            except Exception:
                print(msg, flush=True)

if __name__ == "__main__":
    asyncio.run(main())
