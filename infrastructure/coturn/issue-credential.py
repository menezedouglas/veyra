#!/usr/bin/env python3
"""Operator helper; reads protected TURN REST secret from stdin, emits a
short-lived random username/password JSON to its authorized caller. Never run
as an unauthenticated public endpoint or send this output to application logs.
"""
import base64
import hashlib
import hmac
import json
import secrets
import sys
import time

secret = sys.stdin.buffer.read(4097).strip()
if len(secret) < 32 or len(secret) > 4096:
    raise SystemExit("A protected operator secret of 32–4096 bytes is required on stdin")
username = str(int(time.time()) + 300) + ":" + secrets.token_hex(16)
password = base64.b64encode(hmac.new(secret, username.encode("ascii"), hashlib.sha1).digest()).decode("ascii")
print(json.dumps({"username": username, "credential": password, "expires_in_seconds": 300}))
# Python does not guarantee zeroization of these operator-only values. The
# helper is separate from the client/vault and terminates immediately.
