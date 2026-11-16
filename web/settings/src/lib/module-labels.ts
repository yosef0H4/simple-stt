import { t } from './i18n';
import type { Runtime } from './modules.svelte';

/** Technical names remain searchable and appear as secondary option metadata. */
export function runtimeLabel(runtime: Pick<Runtime, 'backend' | 'label'>): string {
  return runtime.backend === 'cuda' ? t('module.nvidia')
    : runtime.backend === 'vulkan' ? t('ui.gpu_backend')
    : runtime.backend === 'cpu' ? t('module.processor') : runtime.label;
}

export function recommendationLabel(reason?: string | null): string {
  return reason ? t(`module.recommendation.${reason}`) : t('ui.recommended.9ef937');
}
