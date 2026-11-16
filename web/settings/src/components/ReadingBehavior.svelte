<script lang="ts">
import {reading} from "../lib/reading-state.svelte";
import type {Binding} from "../lib/modules.svelte";
import Group from "./Group.svelte";
import Combo from "./Combo.svelte";
import Range from "./Range.svelte";
import {t} from "../lib/i18n";
const bindings=$derived(reading.config ? [reading.config.speech.selection.single,...Object.values(reading.config.speech.selection.languages)] : []);
function value(key:"clean_text"|"currency_words") {
 const values=bindings.map(binding=>binding[key]??true);
 return values.every(Boolean)?"on":values.every(value=>!value)?"off":"mixed";
}
function update(key:"clean_text"|"currency_words",value:string){for(const binding of bindings)binding[key]=value==="on";}
function speed(){if(reading.config?.speech.provider.kind==="open_ai_compatible")return Math.round(reading.config.speech.speed*100);const speeds=bindings.map(binding=>binding.speed_percent??100);return speeds.every(value=>value===speeds[0])?speeds[0]:null;}
function setSpeed(value:number){if(!Number.isFinite(value))return;for(const binding of bindings)binding.speed_percent=value;if(reading.config)reading.config.speech.speed=value/100;}
const choices=[{value:"on",label:"settings.on"},{value:"off",label:"settings.off"}];
</script>
{#if reading.config}
<Group title="settings.reading_behavior" icon="text">
 <Range label="settings.reading_speed" path="reading.speed" value={speed()} min={50} max={200} step={5} unit="%" help="settings.reading_preferences_help" onchange={setSpeed}/>
 {#each [["clean_text","module.clean_text"],["currency_words","module.currency_words"]] as [key,label]}
 <Combo {label} disabled={key==="currency_words"&&value("clean_text")==="off"} path={`reading.${key}`} value={value(key as "clean_text"|"currency_words")} items={value(key as "clean_text"|"currency_words")==="mixed"?[{value:"mixed",label:"settings.mixed"},...choices]:choices} help={key==="clean_text"?"module.clean_text_help":"module.currency_words_help"} onselect={selected=>{if(selected!=="mixed")update(key as "clean_text"|"currency_words",selected);}}/>
 {/each}
</Group>
{/if}
