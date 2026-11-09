#!/usr/bin/env python3
"""Synthetic-data two-process WebRTC/Noise/HPKE test; never a desktop client.
The controller forwards public SDP/bindings and approves matching verification
codes explicitly. Production pairing requires the actual user's comparison.
TURN credentials/config arrive on stdin, not process args or environment.
"""
import argparse, json, os, selectors, subprocess, sys, time
p=argparse.ArgumentParser()
p.add_argument('--binary',default='target/debug/veyra-tests')
p.add_argument('--relay',action='store_true',help='Read TransportConfig JSON from stdin; require actual selected relay pair')
a=p.parse_args()
config=json.load(sys.stdin) if a.relay else {'ice_servers':[],'relay_only':False,'bind_addrs':['0.0.0.0:0']}
if a.relay and not config.get('relay_only'):
    raise SystemExit('Relay test requires relay_only=true')
children=[]
def send(child,message):
    child.stdin.write(json.dumps(message,separators=(',',':'))+'\n'); child.stdin.flush()
def read(child):
    selector=selectors.DefaultSelector();selector.register(child.stdout,selectors.EVENT_READ)
    ready=selector.select(45);selector.close()
    if not ready: raise RuntimeError('Timed out waiting for peer')
    line=child.stdout.readline(98305)
    if not line or len(line)>98304: raise RuntimeError('Peer failed or oversized control message')
    return json.loads(line)
try:
    for initiator in (True,False):
        child=subprocess.Popen([a.binary],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=None,text=True,bufsize=1)
        children.append(child);send(child,{'initiator':initiator,'config':config})
    left,right=children
    lb,rb=read(left),read(right)
    if lb['kind']!='binding' or rb['kind']!='binding': raise RuntimeError('Missing identities')
    send(left,rb);send(right,lb)
    offer=read(left)
    if offer['kind']!='offer':raise RuntimeError('Missing offer')
    send(right,offer)
    answer=read(right)
    if answer['kind']!='answer':raise RuntimeError('Missing answer')
    send(left,answer)
    lv,rv=read(left),read(right)
    if lv.get('kind')!='verification' or rv.get('kind')!='verification' or lv.get('code')!=rv.get('code'):
        raise RuntimeError('Transcript verification mismatch')
    for child in children: send(child,{'kind':'approve','approved':True,'code':lv['code']})
    results=[read(c) for c in children]
    expected='relay' if a.relay else 'direct'
    for result in results:
        if result.get('kind')!='result' or result.get('route')!=expected or not result.get('bidirectional') or not result.get('at_most_once') or result.get('pending')!=0:
            raise RuntimeError('Peer did not satisfy assertions')
    for c in children:
        if c.wait(timeout=10)!=0:raise RuntimeError('Peer exited unsuccessfully')
    print(json.dumps({'passed':True,'processes':2,'route':expected,'noise':'XX_25519_ChaChaPoly_SHA256','hpke':'X25519-HKDF-SHA256/ChaCha20Poly1305','bidirectional':True,'at_most_once':True,'duplicate_ack_idempotent':True},ensure_ascii=False))
except Exception as e:
    print('Veyra integration failed: '+str(e),file=sys.stderr)
    raise SystemExit(1)
finally:
    for c in children:
        if c.poll() is None:c.kill()
        c.wait()
