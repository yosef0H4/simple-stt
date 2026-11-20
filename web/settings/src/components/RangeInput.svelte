<script lang="ts">
  let { id, min, max, value, step = 1, disabled = false, valueText, oninput }: {
    id: string;
    min: number;
    max: number;
    value: number;
    step?: number;
    disabled?: boolean;
    valueText?: string;
    oninput: (value: number) => void;
  } = $props();
  const fill = $derived(Math.max(0, Math.min(100, 100 * (value - min) / (max - min))));
</script>

<input
  type="range"
  class="segmented-range"
  {id} {min} {max} {step} {value} {disabled}
  aria-valuetext={valueText}
  style:--range-fill={`${fill}%`}
  oninput={(event) => oninput(event.currentTarget.valueAsNumber)}
/>
