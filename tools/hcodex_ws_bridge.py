#!/usr/bin/env python3

import argparse
import asyncio
import json
import sys
from pathlib import Path

import websockets


async def relay_messages(source, target):
    try:
        async for message in source:
            await target.send(message)
    except websockets.ConnectionClosed:
        pass
    finally:
        await target.close()


async def run_bridge(upstream_url: str, ready_file: Path) -> int:
    stop_event = asyncio.Event()

    async def handler(client_ws):
        try:
            async with websockets.connect(upstream_url) as upstream_ws:
                await asyncio.gather(
                    relay_messages(client_ws, upstream_ws),
                    relay_messages(upstream_ws, client_ws),
                )
        finally:
            stop_event.set()

    server = await websockets.serve(handler, "127.0.0.1", 0)
    port = server.sockets[0].getsockname()[1]
    ready_file.write_text(
        json.dumps({"ws_url": f"ws://127.0.0.1:{port}"}) + "\n",
        encoding="utf-8",
    )

    try:
        await stop_event.wait()
    finally:
        server.close()
        await server.wait_closed()
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream", required=True)
    parser.add_argument("--ready-file", required=True)
    args = parser.parse_args()
    return asyncio.run(run_bridge(args.upstream, Path(args.ready_file)))


if __name__ == "__main__":
    raise SystemExit(main())
