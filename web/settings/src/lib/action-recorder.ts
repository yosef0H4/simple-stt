import type {RecordedAction, RecordedKeyEvent} from './reader';
const names=new Set('Minus Equal Backspace Tab BracketLeft BracketRight Enter ControlLeft ControlRight Semicolon Quote Backquote ShiftLeft ShiftRight Backslash Comma Period Slash AltLeft AltRight Space CapsLock NumLock ScrollLock IntlBackslash NumpadMultiply NumpadSubtract NumpadAdd NumpadDecimal NumpadEnter NumpadDivide PrintScreen Home ArrowUp PageUp ArrowLeft ArrowRight End ArrowDown PageDown Insert Delete MetaLeft MetaRight ContextMenu'.split(' '));
export function supportedKey(key:string){return /^(Key[A-Z]|Digit[0-9]|Numpad[0-9]|F([1-9]|1[0-2]))$/.test(key)||names.has(key);}
export class ActionRecorder {
  events:RecordedKeyEvent[]=[];
  held=new Set<string>();
  origin:number|undefined;
  event(key:string,down:boolean,now:number){
    if(!supportedKey(key))throw Error('reading.action_unsupported');
    if(down===this.held.has(key))return;
    this.origin??=now;
    const at_ms=Math.max(0,Math.round(now-this.origin));
    if(this.events.length>=128||at_ms>30000)throw Error('reading.action_limit');
    if(down)this.held.add(key);else this.held.delete(key);
    this.events.push({key,down,at_ms});
  }
  finish():RecordedAction|null {
    if(!this.events.length||this.held.size)return null;
    return {events:this.events.map(event=>({...event}))};
  }
}
