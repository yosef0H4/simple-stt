#!/usr/bin/env python3
"""Exercise real OCR/TTS worker clients against a private concurrent HTTP server."""
import os
from pathlib import Path
import signal
import struct
import subprocess
import tempfile
import zlib

def main():
    subprocess.run(['cargo','build','-p','reader-tts','-p','reader-ocr'],check=True)
    with tempfile.TemporaryDirectory(prefix='simple-stt-remote-chunks-') as folder:
        root=Path(folder);capture=root/'capture';capture.mkdir()
        def chunk(kind,data):
            return struct.pack('>I',len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
        image=b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',100,50,8,2,0,0,0))+chunk(b'IDAT',zlib.compress((b'\0'+b'\xff'*300)*50))+chunk(b'IEND',b'')
        (capture/'mock.png').write_bytes(image)
        env={**os.environ,'SIMPLE_STT_READER_DATA':str(root),'SIMPLE_TTS_CONFIG':str(root/'config.json'),'SIMPLE_TTS_OCR_API_KEY':'private-fixture-key'}
        child=subprocess.Popen(['cargo','test','-p','readerd','--bin','readerd','remote_read_along_chunks_overlap_independent_ocr','--','--ignored','--nocapture'],env=env,start_new_session=os.name!='nt')
        try:
            assert child.wait(timeout=120)==0,'remote chunk integration failed'
        except BaseException:
            if child.poll() is None:
                if os.name!='nt':os.killpg(child.pid,signal.SIGKILL)
                else:subprocess.run(['taskkill','/PID',str(child.pid),'/T','/F'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            child.wait();raise
    print('PASS real client workers: per-chunk HTTP, literal exclusions, metadata, OCR/TTS concurrency, and no playback device')

if __name__=='__main__':main()
