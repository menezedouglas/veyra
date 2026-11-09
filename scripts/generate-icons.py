#!/usr/bin/env python3
"""Derive app icons from the polygon coordinates in assets/brand/icon.svg.

Python stdlib only; no network, fonts, or hidden binary source. Supersampling
provides antialiasing. The SVG remains the editable source of the Veyra mark.
"""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'apps/desktop/src-tauri/icons'
DEST.mkdir(parents=True, exist_ok=True)
POLYGONS = [([(12,20),(30,20),(48,56),(39,74)], (0,230,168,255)),
            ([(66,20),(84,20),(57,74),(39,74)], (59,170,160,255))]

def inside(x, y, points):
    hit = False
    j = len(points)-1
    for i, (xi, yi) in enumerate(points):
        xj, yj = points[j]
        if (yi > y) != (yj > y) and x < (xj-xi)*(y-yi)/(yj-yi)+xi:
            hit = not hit
        j = i
    return hit

def color(x,y):
    radius = 22
    cx,cy = min(max(x,radius),96-radius), min(max(y,radius),96-radius)
    if (x-cx)**2+(y-cy)**2 > radius**2: return (0,0,0,0)
    c = (11,42,52,255)
    for points, fill in POLYGONS:
        if inside(x,y,points): c=fill
    return c

def png(size):
    samples = 4
    data=bytearray()
    for y in range(size):
        data.append(0)
        for x in range(size):
            colors=[color((x+(sx+.5)/samples)*96/size,(y+(sy+.5)/samples)*96/size)
                    for sy in range(samples) for sx in range(samples)]
            data.extend(round(sum(c[i] for c in colors)/len(colors)) for i in range(4))
    def chunk(kind,payload):
        return struct.pack('!I',len(payload))+kind+payload+struct.pack('!I',zlib.crc32(kind+payload)&0xffffffff)
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('!IIBBBBB',size,size,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(data,9))+chunk(b'IEND',b'')

if __name__ == '__main__':
    images={size:png(size) for size in (16,24,32,48,64,128,256,512)}
    for size,image in images.items():
        (DEST/f'{size}x{size}.png').write_bytes(image)
    (DEST/'128x128@2x.png').write_bytes(images[256])
    (DEST/'icon.png').write_bytes(images[512])
    selected=[(s,images[s]) for s in (16,24,32,48,64,128,256)]
    offset=6+16*len(selected)
    entries=[]
    for size,image in selected:
        entries.append(struct.pack('<BBBBHHII',size if size<256 else 0,size if size<256 else 0,0,0,1,32,len(image),offset))
        offset+=len(image)
    (DEST/'icon.ico').write_bytes(struct.pack('<HHH',0,1,len(selected))+b''.join(entries)+b''.join(image for _,image in selected))
    print(f'Generated {len(images)} PNG sizes and Windows ICO from Veyra polygons.')
