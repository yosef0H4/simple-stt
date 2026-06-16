#!/usr/bin/env python3
"""Dependency-free loopback IPC proof of concept for the thin AHK shell design."""
import argparse, json, os, socket, threading, time

def send_line(conn, value):
    conn.sendall((json.dumps(value, ensure_ascii=False) + "\n").encode("utf-8"))
def recv_line(file):
    line = file.readline()
    if not line: raise EOFError("closed")
    return json.loads(line.decode("utf-8"))

def main():
    ap=argparse.ArgumentParser(); ap.add_argument("--state", required=True); ap.add_argument("--token", required=True); args=ap.parse_args()
    events=[]; seq=0
    sock=socket.socket(); sock.bind(("127.0.0.1",0)); sock.listen()
    with open(args.state+".tmp","w",encoding="utf-8") as f: json.dump({"protocol":1,"pid":os.getpid(),"address":"%s:%s"%sock.getsockname()},f)
    os.replace(args.state+".tmp",args.state)
    def client(conn):
        nonlocal seq
        with conn, conn.makefile("rb") as f:
            hello=recv_line(f)
            if hello != {"type":"hello","protocol":1,"token":args.token}: send_line(conn,{"type":"error","code":"unauthorized","message":"bad hello"}); return
            send_line(conn,{"type":"hello_ack","protocol":1})
            request=recv_line(f); name=request["command"]["name"]
            if name=="ping": response={"ok":True,"message":"pong","events":[]}
            elif name=="start_recording": seq+=1; events.append({"seq":seq,"kind":"recording_started","session_id":request["command"]["session_id"],"text":""}); response={"ok":True,"message":"started","events":[]}
            elif name=="stop_recording": seq+=1; events.append({"seq":seq,"kind":"transcript","session_id":request["command"]["session_id"],"text":"مرحبا 世界 🙂"}); response={"ok":True,"message":"stopped","events":[]}
            elif name=="cancel": seq+=1; events.append({"seq":seq,"kind":"notice","level":"warning","text":"Cancelled"}); response={"ok":True,"message":"cancelled","events":[]}
            elif name=="poll_events": response={"ok":True,"message":"events","values":{"latest_seq":str(seq)},"events":[e for e in events if e["seq"]>request["command"]["after_seq"]]}
            else: response={"ok":False,"message":"unsupported poc command","events":[]}
            response.setdefault("values",{})
            send_line(conn,{"type":"response","request_id":request["request_id"],"response":response})
    while True:
        conn,_=sock.accept(); threading.Thread(target=client,args=(conn,),daemon=True).start()
if __name__=="__main__": main()
