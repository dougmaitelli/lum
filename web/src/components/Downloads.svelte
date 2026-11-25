<script lang="ts">
  import { post, type Model, type Snapshot } from "../api/admin";
  let error = $state("");
  async function cancel(alias: string) {
    error = "";
    try {
      await post(`/admin/downloads/${alias}/cancel`, {});
    } catch (e) {
      error = String(e);
    }
  }
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
  {#if error}<p class="warning" role="alert">{error}</p>{/if}

  {#each jobs as m (m.alias)}
    <div class="job">
      <strong>{m.alias}</strong>
      <span class="state">
        {m.state === "queued" ? "Queued" : progress.stage || "Preparing"}
      </span>
      <small>{m.repo}</small>
      <button aria-label={`Cancel download of ${m.alias}`} onclick={() => cancel(m.alias)}>
        Cancel
      </button>

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
