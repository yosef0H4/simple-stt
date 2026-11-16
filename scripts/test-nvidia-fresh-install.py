#!/usr/bin/env python3
"""Fresh pinned GPU downloads -> automatic choices -> real WAV/OCR, no desktop/audio.

This deliberately downloads about 1.8 GB into a separate profile. --reuse can
recheck an already verified test profile; it must not be called a fresh run.
"""
import argparse,importlib.util,json,os,subprocess,tempfile,threading,time,wave
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
def module(name,file):
    spec=importlib.util.spec_from_file_location(name,ROOT/'scripts'/file);value=importlib.util.module_from_spec(spec);spec.loader.exec_module(value);return value
settings=module('settings_api','test-settings-selection-api.py')
service=module('reader_service','test-reader-service.py')

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile',type=Path,required=True)
    parser.add_argument('--bin-dir',type=Path,default=ROOT/'target/debug')
    parser.add_argument('--reuse',action='store_true')
    args=parser.parse_args();profile=args.profile.resolve();bins=args.bin_dir.resolve()
    assert profile.is_relative_to(ROOT/'artifacts'),'Use an ignored artifact profile'
    if not args.reuse:assert not profile.exists() or not any(profile.iterdir()),'Fresh run requires an empty profile'
    profile.mkdir(parents=True,exist_ok=True)
    process,api=settings.start_settings_server(bins/'simple-stt-settings',profile/'stt-config.json',profile/'runtime-root',profile/'data')
    daemon=monitor=None;log=None;socket_alias=None
    report={'fresh':not args.reuse,'downloads':{},'operations':[]}
    try:
        initial=api('/api/assets');assets_root=Path(initial['assets'][0]['directory']).parent
        report['initially_installed']=[asset['id'] for asset in initial['assets'] if asset['installed']]
        if not args.reuse:assert not any(asset['installed'] for asset in initial['assets'])
        before=api('/api/reader')['config']
        if not args.reuse:assert before['speech']['selection']['languages']=={}
        def install(ids):
            for identifier in ids:api('/api/asset-action',{'action':'install','id':identifier})
            end=time.monotonic()+900;last={}
            while time.monotonic()<end:
                state=api('/api/assets');jobs={job['job_id']:job for job in state['jobs']}
                for identifier in ids:
                    job=jobs.get(identifier,{})
                    phase=job.get('phase','starting')
                    key=(phase,job.get('id'),int(job.get('downloaded',0)/max(job.get('total',1),1)*10))
                    if last.get(identifier)!=key:
                        print(f'{identifier}: {phase}, component={job.get("id","")}, bytes={job.get("downloaded",0)}/{job.get("total",0)}',flush=True);last[identifier]=key
                    assert phase not in ('failed','cancelled'),job
                if all(next(asset for asset in state['assets'] if asset['id']==identifier)['installed'] and jobs.get(identifier,{}).get('phase')=='ready' for identifier in ids):
                    report['downloads'].update({identifier:jobs[identifier] for identifier in ids});return state
                time.sleep(.3)
            raise AssertionError('Fresh download timeout')
        install(['sherpa-cuda','supertonic3'])
        current=api('/api/reader')['config'];selection=current['speech']['selection']
        assert 'fallback' not in selection and 'uncertain_language' not in selection
        expected_languages={row['id'].split('-')[0] for row in api('/api/keyboard-languages')['languages']}|set(before['speech']['selection']['languages'])
        assert set(selection['languages'])<=expected_languages,'Only explicit and keyboard languages should get assignments'
        # Real model downloads, including Paddle's own detector/dictionary dependencies.
        install(['paddle-ar-en'])
        current=api('/api/reader')['config']
        assert current['ocr']['selection']['single']['runtime']=='paddle-cuda',current['ocr']['selection']
        assert current['ocr']['selection']['single']['model']=='paddle-ar-en'
        # Exercise explicit GPU OCR as well as the default CPU preference covered elsewhere.
        current['ocr']['selection']['single']['device']='gpu'
        current=api('/api/reader-save',{'config':current,'expected_revision':current['revision']})['config']
        install(['kokoro'])
        assert api('/api/reader')['config']['speech']['selection']==selection,'Installing another model must preserve existing choices'
        reader=assets_root.parent/'reader'
        # Unix sockets have a short path limit. Alias the complete instance so
        # its sibling assets still resolve to the real downloads in this profile.
        socket_alias=tempfile.TemporaryDirectory(prefix='stt-gpu-')
        reader.mkdir(parents=True,exist_ok=True)
        short_instance=Path(socket_alias.name)/'instance';short_instance.symlink_to(reader.parent,target_is_directory=True)
        short_reader=short_instance/'reader'
        env=dict(os.environ,SIMPLE_STT_CONFIG=str(profile/'stt-config.json'),SIMPLE_STT_RUNTIME_ROOT=str(profile/'runtime-root'),XDG_DATA_HOME=str(profile/'data'),XDG_CONFIG_HOME=str(profile/'config'),XDG_CACHE_HOME=str(profile/'cache'),SIMPLE_STT_READER_DATA=str(short_reader),SIMPLE_TTS_CONFIG=str(short_reader/'config.json'),SIMPLE_TTS_NO_DESKTOP='1',SIMPLE_STT_NO_DESKTOP='1',DISPLAY='',WAYLAND_DISPLAY='',DBUS_SESSION_BUS_ADDRESS='unix:path=/unavailable-fresh-gpu-test')
        env.pop('LD_LIBRARY_PATH',None)
        log=(profile/'readerd.log').open('w')
        daemon=subprocess.Popen([str(bins/'readerd'),'daemon','--foreground','--no-desktop'],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=log)
        monitor=service.WorkerMonitor(daemon.pid)
        service.wait_for(lambda:(reader/'simple-tts.sock').exists(),12,'fresh reader controller')
        observations={};stop=threading.Event()
        def observe():
            while not stop.wait(.1):
                gpu=subprocess.run(['nvidia-smi','--query-compute-apps=pid,used_gpu_memory','--format=csv,noheader,nounits'],capture_output=True,text=True,timeout=3)
                usage={int(row.split(',')[0]):row.split(',')[1].strip() for row in gpu.stdout.splitlines() if ',' in row and row.split(',')[0].strip().isdigit()}
                for pid in service.children(daemon.pid):
                    name=service.executable(pid)
                    if name not in ('reader-tts','reader-ocr'):continue
                    try:
                        maps=Path(f'/proc/{pid}/maps').read_text()
                        paths=sorted({line.split()[-1] for line in maps.splitlines() if 'libcublas' in line or 'libnvrtc' in line or 'onnxruntime_providers_cuda' in line})
                        if usage.get(pid) and paths:observations[pid]={'worker':name,'vram_mib':usage[pid],'libraries':paths}
                    except FileNotFoundError:pass
        watcher=threading.Thread(target=observe);watcher.start()
        try:
            for language,text in [('en','Hello world. This is a fresh model installation.'),('ar','مرحبا بالعالم. هذا اختبار تثبيت النموذج.')]:
                output=profile/f'{language}.wav'
                output.unlink(missing_ok=True)
                service.run([bins/'simple-stt-linux','media','speak',text,'--output',output],env,timeout=120)
                with wave.open(str(output),'rb') as audio:assert audio.getnframes()>0 and any(audio.readframes(audio.getnframes()))
                report['operations'].append(f'{language} non-silent WAV');print(f'PASS {language} automatic model selection and non-silent GPU WAV',flush=True)
            config=api('/api/reader')['config']
            config['speech']['selection']['languages']['en']={'runtime':'sherpa-cuda','model':'kokoro','device':'gpu','voice':'af_heart'}
            api('/api/reader-save',{'config':config,'expected_revision':config['revision']})
            output=profile/'kokoro.wav'
            output.unlink(missing_ok=True)
            service.run([bins/'simple-stt-linux','media','speak','Hello world. This voice uses Kokoro.','--output',output],env,timeout=120)
            with wave.open(str(output),'rb') as audio:assert audio.getnframes()>0 and any(audio.readframes(audio.getnframes()))
            report['operations'].append('Kokoro non-silent GPU WAV');print('PASS downloaded Kokoro GPU WAV',flush=True)
            text=service.run([bins/'simple-stt-linux','media','ocr',service.FIXTURE],env,timeout=120)
            assert text==service.EXPECTED,(text,service.EXPECTED)
            report['operations'].append('exact OCR text');print('PASS exact GPU OCR text',flush=True)
        finally:
            stop.set();watcher.join(timeout=5)
        assert any(value['worker']=='reader-tts' for value in observations.values()),observations
        assert any(value['worker']=='reader-ocr' for value in observations.values()),observations
        for value in observations.values():
            for path in value['libraries']:assert path.startswith(str(assets_root)),f'Runtime leaked inherited native libraries: {path}'
        report['gpu_workers']=observations
        report['selection']=api('/api/reader')['config']['speech']['selection']
        report['result']='PASS';(profile/'report.json').write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
        print('PASS verified NVIDIA runtime/model downloads, persisted automatic selection, real GPU WAV/OCR and private native dependencies',flush=True)
    finally:
        if daemon and daemon.poll() is None:
            try:service.run([bins/'readerctl','shutdown'],env,timeout=8);daemon.wait(timeout=8)
            except Exception:
                daemon.terminate()
                try:daemon.wait(timeout=3)
                except subprocess.TimeoutExpired:daemon.kill();daemon.wait(timeout=3)
        if monitor:monitor.close()
        if log:log.close()
        if socket_alias:socket_alias.cleanup()
        settings.stop_settings_server(process)

if __name__=='__main__':main()
