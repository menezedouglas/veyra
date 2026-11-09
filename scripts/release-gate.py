#!/usr/bin/env python3
"""Refuse production release while any mandatory acceptance item is pending."""
import json
from pathlib import Path
root=Path(__file__).resolve().parents[1]
matrix=json.loads((root/'docs/release-readiness.json').read_text())
pending=[name for name,value in matrix.items() if value is not True]
if pending:
    raise SystemExit('Production release blocked: '+', '.join(pending))
print('Release evidence gates explicitly complete; independent review still applies.')
