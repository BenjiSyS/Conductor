<script lang="ts">
  let {
    checked,
    label,
    onchange,
    disabled = false,
  }: { checked: boolean; label: string; onchange: (v: boolean) => void; disabled?: boolean } = $props();
</script>

<button
  class="toggle"
  role="switch"
  aria-checked={checked}
  aria-label={label}
  {disabled}
  onclick={() => onchange(!checked)}
></button>
