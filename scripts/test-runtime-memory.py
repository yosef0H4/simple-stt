#!/usr/bin/env python3
"""Measure exact private worker RAM/VRAM after inference without desktop or audio.

CPU comparisons are deliberate: they identify lighter alternatives to retained
GPU contexts. No settings are changed, and no sound or keyboard input is used.
"""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('workers', ROOT / 'scripts/test-reader-workers.py')
workers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(workers)
OUT = ROOT / 'artifacts/combined-app/memory'
GPU = Path.home() / '.local/share/simple-tts/runtime'
CPU = OUT / 'cpu-runtime'
MODELS = {
    'paddle': ROOT / 'artifacts/combined-app/downloads/model-paddle',
    'kokoro': Path.home() / '.local/share/simple-tts/models/kokoro-multi-lang-v1_0',
    'supertonic3': ROOT / 'artifacts/combined-app/downloads/sherpa-onnx-supertonic-3-tts-int8-2026-05-11',
}

def memory(pid):
    result = {'pid': pid}
    status = dict(line.split(':', 1) for line in Path(f'/proc/{pid}/status').read_text().splitlines() if ':' in line)
    for key in ['VmRSS','VmHWM','VmSwap','Threads']:
        result[key] = int(status[key].strip().split()[0])
    rollup = dict(line.split(':',1) for line in Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines() if ':' in line)
    for key in ['Pss','Private_Clean','Private_Dirty','Swap']:
        result[key] = int(rollup.get(key,'0').strip().split()[0])
    gpu = subprocess.check_output(['nvidia-smi','--query-compute-apps=pid,used_gpu_memory','--format=csv,noheader,nounits'], text=True,timeout=8)
    result['vram_mib'] = sum(int(row.split(',')[1].strip()) for row in gpu.splitlines() if row.split(',')[0].strip()==str(pid))
    return result

def infer(child, events, model, number, device):
    request = {'job_id':str(number),'generation':1}
    if model == 'paddle':
        request.update(type='recognize', image_path=str(ROOT/'scripts/fixtures/reader/ocr-light.png'), language='eng')
    else:
        request.update(type='synthesize',sentence=0,unit=0,text='مرحبا بالعالم هذا اختبار باللغة العربية.' if model=='supertonic3' else 'Hello world, this is a memory test.',
                       voice='sid:0' if model=='supertonic3' else 'af_heart',language='ar' if model=='supertonic3' else 'en',
                       steps=5,speed=1.0,device='cuda' if device=='gpu' else 'cpu')
    start=time.monotonic(); workers.send(child,request)
    while True:
        kind,event,pcm=events.get(timeout=60)
        assert kind!='error',event
        if kind=='control':
            assert event['type']!='error',event
            if event['type']=='finished':return round((time.monotonic()-start)*1000,1)

def main():
    OUT.mkdir(parents=True,exist_ok=True)
    results=[]
    with tempfile.TemporaryDirectory(prefix='private-runtime-memory-') as tmp:
        for device,runtime in [('gpu',GPU),('cpu',CPU)]:
            for model,path in MODELS.items():
                env=dict(os.environ, XDG_DATA_HOME=tmp,XDG_CONFIG_HOME=tmp,XDG_CACHE_HOME=tmp,XDG_RUNTIME_DIR=tmp,
                         DISPLAY='',WAYLAND_DISPLAY='',DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-memory-bus',
                         LD_LIBRARY_PATH=str(runtime/'lib'),SIMPLE_TTS_CAPTURE_DIR=str(ROOT/'scripts/fixtures/reader'))
                arguments=['--model-dir',str(path),'--runtime-dir',str(runtime),'--device',device if model=='paddle' else 'cuda' if device=='gpu' else 'cpu']
                if model!='paddle':arguments += ['--provider','local_supertonic' if model=='supertonic3' else 'local_kokoro']
                binary=ROOT/'target/debug'/('reader-ocr' if model=='paddle' else 'reader-tts')
                with (OUT/f'{model}-{device}.log').open('w') as log:
                    child,events=workers.launch(binary,arguments,env,log)
                    try:
                        cold=infer(child,events,model,0,device)
                        warm=infer(child,events,model,1,device)
                        after=memory(child.pid)
                        time.sleep(3)
                        idle=memory(child.pid)
                        assert (idle['vram_mib']>0)==(device=='gpu'),idle
                        result={'model':model,'device':device,'cold_ms':cold,'warm_ms':warm,'after':after,'idle_3s':idle}
                        results.append(result)
                        print(json.dumps(result),flush=True)
                        (OUT/'report.json').write_text(json.dumps(results,indent=2))
                    finally:workers.finish(child)
    print('PASS private memory profiles; no desktop, audio or clipboard use')

if __name__=='__main__':main()
