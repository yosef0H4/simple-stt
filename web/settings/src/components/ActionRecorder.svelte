<script lang="ts">
import {tick} from 'svelte';
import {reading} from '../lib/reading-state.svelte';
import {ActionRecorder} from '../lib/action-recorder';
import {t} from '../lib/i18n';
let recorder=$state<ActionRecorder|null>(null),count=$state(0),error=$state(''),arming=$state(''),pending=$state(false);
let pad=$state<HTMLTextAreaElement>();
let activation='';
const summary=$derived(reading.config?.read_along.sequence?.events.filter(event=>event.down).map(event=>event.key.replace(/^(Key|Digit)/,'')).join(' → ')||reading.config?.read_along.advance_key||'Space');
async function start(){recorder=new ActionRecorder();count=0;error='';pending=false;arming=activation;activation='';await tick();pad?.focus();}
function stop(){
 if(!recorder)return;
 if(recorder.held.size){pending=true;return;}
 const action=recorder.finish();if(action&&reading.config)reading.config.read_along.sequence=action;
 recorder=null;pending=false;
}
function cancel(){recorder=null;pending=false;}
function record(event:KeyboardEvent,down:boolean){
 event.preventDefault();event.stopPropagation();
 if(!recorder)return;
 if(arming){if(!down&&event.code===arming)arming='';return;}
 if(event.code==='Escape'){if(down)stop();return;}
 if(pending&&down)return;
 try{recorder.event(event.code,down,performance.now());count=recorder.events.length;if(pending&&!recorder.held.size)stop();}
 catch(e){error=t(e instanceof Error?e.message:'reading.action_unsupported');cancel();}
}
</script>
<div class="field"><label for="recorded-action">{t('reading.recorded_action')}</label><div class="control">
 <input id="recorded-action" readonly value={summary} />
 <button type="button" disabled={!!recorder} onkeydown={event=>{activation=event.code==='Enter'?'Enter':'';}} onclick={start}>{t('reading.record_action')}</button>
 <button type="button" disabled={!!recorder} onclick={()=>{if(reading.config){reading.config.read_along.sequence=null;reading.config.read_along.advance_key='Space';}}}>{t('reading.reset_action')}</button>
</div></div>
{#if recorder}
 <textarea bind:this={pad} rows="2" readonly value={`${t('reading.recording_action')} · ${count}`} aria-label={t('reading.recording_action')} onkeydown={event=>record(event,true)} onkeyup={event=>record(event,false)} onblur={cancel}></textarea>
 <div class="reading-actions"><button type="button" onpointerdown={event=>{event.preventDefault();stop();}} onclick={stop}>{t('reading.stop')}</button><button type="button" onpointerdown={event=>{event.preventDefault();cancel();}} onclick={cancel}>{t('ui.cancel.77dfd2')}</button></div>
 <p class="field-help">{t('reading.record_action_help')}</p>
{/if}
{#if error}<p role="alert">{error}</p>{/if}
