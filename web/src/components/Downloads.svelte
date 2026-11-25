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
  let hadJobs = false;
  $effect(() => {
    const hasJobs = jobs.length > 0;
    if (hadJobs && !hasJobs && isOpen()) popover.hidePopover();
    hadJobs = hasJobs;
  });
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
    {@const queued = m.state === "queued"}
    <div class="job">
      <div class="job-detail">
        <strong title={m.repo}>{m.alias}</strong>
        <progress
          aria-label={"Current file progress for " + m.alias}
          max="100"
          value={queued ? 0 : (progress.percent ?? undefined)}
        ></progress>
        <small class="state">
          {queued
            ? "Queued"
            : progress.stage || "Preparing"}{#if !queued && progress.percent !== null}
            · {progress.percent}%{/if}
        </small>
      </div>
      <button aria-label={`Cancel download of ${m.alias}`} onclick={() => cancel(m.alias)}>
        Cancel
      </button>
    </div>
  {:else}
    <p class="muted">No active downloads.</p>
  {/each}
</div>
