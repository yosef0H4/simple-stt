<script lang="ts">
import {t} from '../lib/i18n';
let {id,value=$bindable(''),saved=false,busy=false,onsave,onclear}:{id:string;value?:string;saved?:boolean;busy?:boolean;onsave:()=>Promise<void>;onclear:()=>Promise<void>}=$props();
</script>
<div class="field"><label for={id}>{t('reading.api_key')}{saved?' ✓':''}</label><div class="control">
<input {id} type="password" autocomplete="new-password" bind:value placeholder={t(saved?'ui.replace_saved_key.893959':'ui.paste_api_key.7da909')}/>
<button type="button" disabled={busy} aria-busy={busy} onclick={onsave}>{t('settings.save_connection')}</button>
<button type="button" disabled={busy||(!saved&&!value)} onclick={onclear}>{t('reading.clear_key')}</button>
</div></div>
