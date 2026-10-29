import struct, sys
from PIL import Image
f = sys.argv[1]; d = open(f, 'rb').read()
h = struct.unpack('>25I', d[:100])
hs, depth, w, hgt, bo, bpp, bpl, ncol = h[0], h[3], h[4], h[5], h[7], h[11], h[12], h[19]
off = hs + ncol * 12
raw = d[off:off + bpl * hgt]
if bpp == 32:
    mode = 'BGRX' if bo == 0 else 'XRGB'
    img = Image.frombytes('RGB', (w, hgt), raw, 'raw', mode, bpl)
else:
    img = Image.frombytes('RGB', (w, hgt), raw, 'raw', 'BGR', bpl)
img.save(f[:-4] + '.png'); print('wrote', img.size, 'bpp', bpp, 'depth', depth, 'byteorder', bo)
