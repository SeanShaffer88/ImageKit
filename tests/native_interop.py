"""Pixel-exact Python zlib PNG fixtures against a real DLL; no third-party packages."""
import ctypes, pathlib, struct, sys, zlib
lib=ctypes.CDLL(str(pathlib.Path(sys.argv[1]).resolve()))
lib.ik_load_bytes.argtypes=[ctypes.c_void_p,ctypes.c_size_t];lib.ik_load_bytes.restype=ctypes.c_uint64
lib.ik_get_pixel.argtypes=[ctypes.c_uint64,ctypes.c_uint32,ctypes.c_uint32,ctypes.POINTER(ctypes.c_uint32)];lib.ik_get_pixel.restype=ctypes.c_int
lib.ik_free_image.argtypes=[ctypes.c_uint64];lib.ik_free_image.restype=ctypes.c_int
lib.ik_resize.argtypes=[ctypes.c_uint64,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_int];lib.ik_resize.restype=ctypes.c_uint64
lib.ik_encode_png.argtypes=[ctypes.c_uint64,ctypes.POINTER(ctypes.c_void_p),ctypes.POINTER(ctypes.c_size_t)];lib.ik_encode_png.restype=ctypes.c_int
lib.ik_free_buffer.argtypes=[ctypes.c_void_p,ctypes.c_size_t]
def chunk(kind,data):return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
def png(w,h,body):return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',w,h,8,6,0,0,0))+chunk(b'IDAT',zlib.compress(body))+chunk(b'IEND',b'')
def load(data):
    buf=ctypes.create_string_buffer(data);return lib.ik_load_bytes(buf,len(data))
for width,height in [(1,1),(3,2),(17,9)]:
    pixels=bytes((i*37+11)%256 for i in range(width*height*4))
    encoded=png(width,height,b''.join(b'\0'+pixels[y*width*4:(y+1)*width*4] for y in range(height)))
    handle=load(encoded);assert handle
    try:
        for y in range(height):
            for x in range(width):
                value=ctypes.c_uint32();assert lib.ik_get_pixel(handle,x,y,ctypes.byref(value))==0
                start=(y*width+x)*4;assert value.value==int.from_bytes(pixels[start:start+4],'big')
        assert lib.ik_resize(handle,0xffffffff,0xffffffff,0)==0
        pointer=ctypes.c_void_p();length=ctypes.c_size_t();assert lib.ik_encode_png(handle,ctypes.byref(pointer),ctypes.byref(length))==0
        try: output=ctypes.string_at(pointer,length.value)
        finally: lib.ik_free_buffer(pointer,length.value)
        pos=8;compressed=b''
        while pos<len(output):
            n=int.from_bytes(output[pos:pos+4],'big');kind=output[pos+4:pos+8];body=output[pos+8:pos+8+n]
            assert zlib.crc32(kind+body)==int.from_bytes(output[pos+8+n:pos+12+n],'big')
            if kind==b'IDAT':compressed+=body
            pos+=12+n
        raw=zlib.decompress(compressed);decoded=bytearray();stride=width*4
        for y in range(height):
            filt=raw[y*(stride+1)];row=bytearray(raw[y*(stride+1)+1:(y+1)*(stride+1)])
            for x in range(stride):
                a=row[x-4] if x>=4 else 0;b=decoded[(y-1)*stride+x] if y else 0;c=decoded[(y-1)*stride+x-4] if y and x>=4 else 0
                p=a+b-c;dist=[abs(p-a),abs(p-b),abs(p-c)];paeth=[a,b,c][dist.index(min(dist))]
                row[x]=(row[x]+[0,a,b,(a+b)//2,paeth][filt])%256
            decoded+=row
        assert decoded==pixels
    finally:assert lib.ik_free_image(handle)==0
assert load(png(0xffffffff,0xffffffff,b''))==0
assert load(png(1,1,b'\0'+b'a'*10000))==0
print('Native interop passed: pixel-exact Python PNG -> DLL; DLL PNG -> Python zlib/filters; bounds rejected')
