<script lang="ts">
import Help from "./Help.svelte";
import {t} from "../lib/i18n";
let {label,path,value,min,max,step=1,unit="",help="",disabled=false,onchange}:{label:string;path:string;value:number|null;min:number;max:number;step?:number;unit?:string;help?:string;disabled?:boolean;onchange:(value:number)=>void}=$props();
function change(event:Event){const value=(event.currentTarget as HTMLInputElement).valueAsNumber;if(Number.isFinite(value))onchange(value);}
</script>
<div class="field" class:field-disabled={disabled} data-setting-path={path}>
 <div class="field-copy"><label for={path}>{t(label)}</label>{#if help}<Help text={help}/>{/if}</div>
 <div class="control range-control"><input id={path} type="range" {min} {max} {step} {disabled} value={value??min} aria-valuetext={value===null?t("settings.mixed"):`${value}${unit}`} oninput={change}/><input class="range-number" type="number" aria-label={`${t(label)} ${t("settings.exact_value")}`} {min} {max} {step} {disabled} value={value??""} placeholder={t("settings.mixed")} oninput={change}/>{#if unit}<span class="range-value">{unit}</span>{/if}</div>
</div>
