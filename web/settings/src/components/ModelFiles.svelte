<script lang="ts">
import {ui,action} from "../lib/state.svelte";
import {fields} from "../lib/settings";
import type {Feature} from "../lib/modules.svelte";
import Field from "./Field.svelte";
import Group from "./Group.svelte";
import IconButton from "./IconButton.svelte";
import {t} from "../lib/i18n";
let {feature}:{feature:Feature}=$props();
</script>
{#if feature==='stt'}
<details class="guide"><summary>{t("settings.model_files")}</summary>
<Group title="settings.model_files" icon="advanced" paths={["speech.runtime_dir","speech.model_dir"]}>
 {#snippet actions()}<IconButton icon="close" label="ui.unload_speech_model.045291" disabled={!ui.state?.service_online} onclick={()=>action("unload_model")}/>{/snippet}
 <Field spec={fields.runtime}/><Field spec={fields.modelDir}/>
 <code>{ui.state?.resolved_runtime_dir}</code><code>{ui.state?.resolved_model_dir}</code>
</Group>
</details>
{/if}
