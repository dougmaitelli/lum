<script lang="ts">
  import type { Snapshot } from "../api/admin";
  let { data, stale }: { data: Snapshot; stale: boolean } = $props();
</script>

<footer id="system-status" aria-label="System status">
  {#if stale}
    System status unavailable · Reconnecting…
  {:else}
    <span class="engine">
      {data.mock ? "Mock engine" : "Engine"} · {data.online ? "Online" : "Unavailable"}
    </span>

    {#if data.online}
      <span>
        {data.models.filter((m) => m.loaded).length} loaded
      </span>
    {/if}
    {#each data.vram as gpu (gpu.card)}
      <span title="Driver-reported device memory, all processes">
        VRAM {data.vram.length > 1 ? gpu.card : ""}
        {gpu.summary}
      </span>
    {:else}
      <span>VRAM unavailable</span>
    {/each}
  {/if}
</footer>
