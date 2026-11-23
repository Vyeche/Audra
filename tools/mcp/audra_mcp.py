#!/usr/bin/env python3
"""Audra's control socket as MCP tools, and as a command line.

Audra listens on $XDG_RUNTIME_DIR/audra-control.sock when started with
`--control` (crates/app/src/control.rs). This script speaks to it.

As an MCP server (stdio, JSON-RPC 2.0; no packages needed):
    python3 -I tools/mcp/audra_mcp.py
Tools: audra_keys, audra_press, audra_view, audra_state, audra_screenshot,
audra_clap
(the screenshot comes back as an image, so the session sees the window).

As a command line, for a quick check:
    python3 -I tools/mcp/audra_mcp.py keys
    python3 -I tools/mcp/audra_mcp.py press MONO
    python3 -I tools/mcp/audra_mcp.py view mixer
    python3 -I tools/mcp/audra_mcp.py state
    python3 -I tools/mcp/audra_mcp.py clap
    python3 -I tools/mcp/audra_mcp.py screenshot /abs/path.png
"""
import base64
import json
import os
import socket
import sys
import tempfile

# The real Audra by default; AUDRA_CONTROL_SOCKET points elsewhere, e.g. the
# simulation's audra-mock-control.sock.
SOCKET = os.environ.get("AUDRA_CONTROL_SOCKET") or os.path.join(os.environ.get("XDG_RUNTIME_DIR", tempfile.gettempdir()), "audra-control.sock")


def ask(cmd: dict, timeout: float = 10.0) -> dict:
    """Sends one request and returns Audra's reply."""
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
        s.settimeout(timeout)
        try:
            s.connect(SOCKET)
        except OSError as e:
            return {"ok": False, "error": f"Audra isn't listening on {SOCKET} ({e}); start it with --control"}
        s.sendall((json.dumps(cmd) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
    return json.loads(buf.decode() or '{"ok": false, "error": "no reply"}')


TOOLS = [
    {
        "name": "audra_keys",
        "description": "List Audra's Control Room keys (Dim, Mono, Talkback...) with whether each is lit and can be pressed.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "audra_press",
        "description": "Press one of Audra's Control Room keys, as a click would. Changes what the unit plays.",
        "inputSchema": {"type": "object", "properties": {"key": {"type": "string", "description": "e.g. MONO, DIM"}}, "required": ["key"]},
    },
    {
        "name": "audra_view",
        "description": "Switch Audra's view.",
        "inputSchema": {"type": "object", "properties": {"to": {"type": "string", "enum": ["mixer", "matrix", "quick"]}}, "required": ["to"]},
    },
    {
        "name": "audra_state",
        "description": "Read Audra's state: the view, the mix being edited, the Control Room switches, Mono on the device, the notice showing.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "audra_clap",
        "description": "Play one clap through Audra's Reverb and Echo (the page's \"Hear a clap\"), to check the FX path end to end. Needs an effect on.",
        "inputSchema": {"type": "object", "properties": {}},
    },
    {
        "name": "audra_snapshot",
        "description": "Recall one of Audra's stored snapshots (1-8), as a click on its key would. Changes what the unit plays; switches Reverb and Echo as the snapshot keeps them.",
        "inputSchema": {"type": "object", "properties": {"n": {"type": "integer", "minimum": 1, "maximum": 8}}, "required": ["n"]},
    },
    {
        "name": "audra_fx",
        "description": "Switch Audra's Reverb or Echo on or off, as its key on the Reverb & Echo page would.",
        "inputSchema": {"type": "object", "properties": {"unit": {"type": "string", "enum": ["reverb", "echo"]}, "on": {"type": "boolean"}}, "required": ["unit", "on"]},
    },
    {
        "name": "audra_screenshot",
        "description": "Take a picture of Audra's window as it is now; returns the image.",
        "inputSchema": {"type": "object", "properties": {}},
    },
]


def call(name: str, args: dict) -> list:
    """Runs a tool and returns MCP content blocks."""
    if name == "audra_keys":
        reply = ask({"cmd": "keys"})
    elif name == "audra_press":
        reply = ask({"cmd": "press", "key": args.get("key", "")})
    elif name == "audra_view":
        reply = ask({"cmd": "view", "to": args.get("to", "")})
    elif name == "audra_state":
        reply = ask({"cmd": "state"})
    elif name == "audra_clap":
        reply = ask({"cmd": "clap"})
    elif name == "audra_snapshot":
        reply = ask({"cmd": "snapshot", "n": args.get("n", 0)})
    elif name == "audra_fx":
        reply = ask({"cmd": "fx", "unit": args.get("unit", ""), "on": bool(args.get("on"))})
    elif name == "audra_screenshot":
        fd, path = tempfile.mkstemp(prefix="audra-", suffix=".png")
        os.close(fd)
        reply = ask({"cmd": "screenshot", "path": path}, timeout=20.0)
        if reply.get("ok"):
            with open(path, "rb") as f:
                data = base64.b64encode(f.read()).decode()
            os.unlink(path)
            return [{"type": "image", "data": data, "mimeType": "image/png"}, {"type": "text", "text": json.dumps(reply)}]
    else:
        reply = {"ok": False, "error": f"unknown tool {name}"}
    return [{"type": "text", "text": json.dumps(reply, indent=1)}]


def serve() -> None:
    """A minimal MCP server over stdio."""
    for line in sys.stdin:
        try:
            req = json.loads(line)
        except json.JSONDecodeError:
            continue
        rid, method = req.get("id"), req.get("method", "")
        if rid is None:
            continue  # a notification
        if method == "initialize":
            result = {
                "protocolVersion": req.get("params", {}).get("protocolVersion", "2024-11-05"),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "audra", "version": "0.1"},
            }
        elif method == "tools/list":
            result = {"tools": TOOLS}
        elif method == "tools/call":
            p = req.get("params", {})
            content = call(p.get("name", ""), p.get("arguments") or {})
            result = {"content": content, "isError": False}
        elif method == "ping":
            result = {}
        else:
            print(json.dumps({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": method}}), flush=True)
            continue
        print(json.dumps({"jsonrpc": "2.0", "id": rid, "result": result}), flush=True)


def main() -> None:
    if len(sys.argv) == 1:
        serve()
        return
    cmd, rest = sys.argv[1], sys.argv[2:]
    if cmd == "press":
        out = ask({"cmd": "press", "key": rest[0]})
    elif cmd == "view":
        out = ask({"cmd": "view", "to": rest[0]})
    elif cmd == "screenshot":
        out = ask({"cmd": "screenshot", "path": os.path.abspath(rest[0])}, timeout=20.0)
    elif cmd == "snapshot":
        out = ask({"cmd": "snapshot", "n": int(rest[0])})
    elif cmd == "fx":
        out = ask({"cmd": "fx", "unit": rest[0], "on": rest[1] == "on"})
    else:
        out = ask({"cmd": cmd})
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
