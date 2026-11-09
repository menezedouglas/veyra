#!/usr/bin/env python3
"""Two real Rust processes; pipes carry only synthetic lab SDP in RAM.

Usage: python3 tests/connectivity/two_processes.py target/debug/veyra-transport-probe [--relay]
Relay credentials are read as one JSON object from stdin, never arguments/env.
"""
import argparse
import json
import selectors
import subprocess
import sys


def read(process, timeout=45):
    selector = selectors.DefaultSelector()
    selector.register(process.stdout, selectors.EVENT_READ)
    if not selector.select(timeout):
        raise RuntimeError("peer timeout")
    value = json.loads(process.stdout.readline())
    selector.close()
    return value


def write(process, value):
    process.stdin.write(json.dumps(value) + "\n")
    process.stdin.flush()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("binary")
    parser.add_argument("--relay", action="store_true")
    args = parser.parse_args()
    config = {"ice_servers": [], "relay_only": args.relay, "bind_addrs": ["0.0.0.0:0"]}
    if args.relay:
        config["ice_servers"] = [json.loads(sys.stdin.readline())]
    peers = [subprocess.Popen([args.binary, role], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=None, text=True, bufsize=1) for role in ["initiator", "responder"]]
    try:
        a, b = peers
        for peer in peers:
            write(peer, config)
        offer = read(a)
        assert offer["event"] == "description"
        write(b, offer["description"])
        answer = read(b)
        assert answer["event"] == "description"
        write(a, answer["description"])
        results = [read(peer) for peer in peers]
        route = "relay" if args.relay else "direct"
        assert all(result["event"] == "verified" and result["route"] == route for result in results)
        for peer in peers:
            write(peer, {"close": True})
        assert all(peer.wait(timeout=10) == 0 for peer in peers)
        print(json.dumps({"result": "passed", "processes": 2, "selected_route": route, "bidirectional": True}))
    finally:
        for peer in peers:
            if peer.poll() is None:
                peer.kill()
            peer.wait()


if __name__ == "__main__":
    main()
