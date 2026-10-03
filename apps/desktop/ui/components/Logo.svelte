<script lang="ts">
  let { size = 24, mono = false }: { size?: number; mono?: boolean } = $props();
</script>

{#if mono}
  <svg width={size} height={size} viewBox="0 0 1024 1024" fill="none" aria-hidden="true">
    <path d="M711.5 495.3 A250 250 0 1 1 534.7 318.5" stroke="currentColor" stroke-width="104" stroke-linecap="round" />
    <path d="M470 560 L770 260" stroke="currentColor" stroke-width="72" stroke-linecap="round" />
    <circle cx="470" cy="560" r="60" fill="currentColor" />
  </svg>
{:else}
  <svg width={size} height={size} viewBox="0 0 1024 1024" aria-hidden="true">
    <rect width="1024" height="1024" rx="232" fill="#16181D" />
    <path
      d="M711.5 495.3 A250 250 0 1 1 534.7 318.5"
      fill="none"
      stroke="#F3F0E8"
      stroke-width="96"
      stroke-linecap="round"
    />
    <path d="M470 560 L770 260" fill="none" stroke="#8FA4FF" stroke-width="64" stroke-linecap="round" />
    <circle cx="470" cy="560" r="54" fill="#8FA4FF" />
  </svg>
{/if}
