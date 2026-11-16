const clone = <T>(value:T):T => JSON.parse(JSON.stringify(value));
export function mergeSavedFields(draft: Record<string,unknown>, baseline: Record<string,unknown>, saved: Record<string,unknown>, prefix='') {
  for(const key of new Set([...Object.keys(baseline),...Object.keys(saved)])){
    const current=draft[key],before=baseline[key],next=saved[key];
    const object=(value:unknown):value is Record<string,unknown>=>!!value&&typeof value==='object'&&!Array.isArray(value);
    const path=prefix?`${prefix}.${key}`:key;
    if(object(current)&&object(next))mergeSavedFields(current,object(before)?before:{},next,path);
    else if(path==='revision'||JSON.stringify(current??null)===JSON.stringify(before??null)){
      if(next===undefined)delete draft[key];else draft[key]=clone(next);
    }
  }
}
