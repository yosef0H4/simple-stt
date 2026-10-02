#!/usr/bin/env python3
"""Compare release Settings RSS using isolated configurations (Linux /proc)."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.parse import urlparse, parse_qs
from urllib.request import Request, urlopen


def measure(binary):
    results=[]
    for _ in range(5):
        with tempfile.TemporaryDirectory(prefix='simple-stt-settings-memory-') as temp:
            env=dict(os.environ,SIMPLE_STT_CONFIG=str(Path(temp)/'config.json'),
                     SIMPLE_STT_RUNTIME_ROOT=str(Path(temp)/'runtime'),XDG_DATA_HOME=str(Path(temp)/'data'))
            process=subprocess.Popen([str(binary.resolve()),'--no-browser'],env=env,cwd=temp,
                                     stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
            try:
                parsed=urlparse(process.stdout.readline().strip());origin=f'{parsed.scheme}://{parsed.netloc}'
                token=parse_qs(parsed.fragment)['token'][0]
                def request(path,body=None):
                    req=Request(origin+path,data=body,headers={'X-Simple-STT-Token':token,'Origin':origin})
                    with urlopen(req,timeout=10) as response:return response.read()
                for _ in range(5):
                    for path in ['/','/app.js','/styles.css','/api/state','/api/defaults']:
                        request(path)
                time.sleep(.05)
                rss=int(next(line.split()[1] for line in Path(f'/proc/{process.pid}/status').read_text().splitlines() if line.startswith('VmRSS:')))*1024
                results.append(rss)
                request('/api/close',b'{}');process.wait(timeout=5)
                assert process.returncode==0
            finally:
                if process.poll() is None:process.kill();process.wait()
    return sorted(results)[len(results)//2]


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--baseline',type=Path,required=True);parser.add_argument('--current',type=Path,default=Path('target/release/simple-stt-settings'));parser.add_argument('--output',type=Path);args=parser.parse_args()
    baseline=measure(args.baseline);current=measure(args.current);report={'baseline_rss':baseline,'current_rss':current,'delta':current-baseline,'allowance':max(2*1024*1024,int(baseline*.1))}
    print(json.dumps(report,indent=2))
    if args.output:
        args.output.parent.mkdir(parents=True,exist_ok=True)
        args.output.write_text(json.dumps(report,indent=2)+'\n')
    assert current-baseline<=report['allowance'],'Settings RSS budget exceeded'

if __name__=='__main__':main()
