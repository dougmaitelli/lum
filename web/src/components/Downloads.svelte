<script lang="ts">
  import type { Model, Snapshot } from "../api/admin";
  let { jobs, progress }: { jobs: Model[]; progress: Snapshot["progress"] } = $props();
  let popover: HTMLElement;
  export function open() {
    popover.showPopover();
  }
  export function isOpen() {
    return popover.matches(":popover-open");
  }
</script>

<div id="downloads" bind:this={popover} popover>
  <h2>Downloads</h2>

  {#each jobs as m (m.alias)}
    <div class="job">
      <strong>{m.alias}</strong>
      <span class="state">
        {m.state === "queued" ? "Queued" : progress.stage || "Preparing"}
      </span>
      <small>{m.repo}</small>

      {#if m.state === "downloading"}
        <progress
          aria-label={"Current file progress for " + m.alias}
          max="100"
          value={progress.percent ?? undefined}
        ></progress>

        {#if progress.percent !== null}
          <small>
            Current file: {progress.percent}%
          </small>
        {/if}
      {/if}
    </div>
  {:else}
    <p class="muted">No active downloads.</p>
  {/each}
</div>
