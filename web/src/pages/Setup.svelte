<script lang="ts">
  import SignOut from "../components/SignOut.svelte";
  import { retryRuntime, type RuntimeStatus } from "../api/admin";
  let { status, error }: { status: RuntimeStatus | undefined; error: string } = $props();
  let retrying = $state(false),
    notice = $state("");
  async function retry() {
    retrying = true;
    notice = "";
    try {
      await retryRuntime();
      notice = "Retry requested.";
    } catch (e) {
      notice = String(e);
    } finally {
      retrying = false;
    }
  }
</script>

<svelte:head><title>Runtime setup · Lum</title></svelte:head>
<main class="notice">
  <h1>Runtime setup</h1>
  {#if status?.oidc}<SignOut />{/if}
  {#if status}<h2>{status.runtime}</h2>{/if}
  {#if error}<p class="warning" role="alert">{error} · Reconnecting…</p>
  {:else if status?.setup.phase === "failed"}
    <pre class="warning">{status.setup.error}</pre>
    <button disabled={retrying} onclick={retry}>Retry setup</button>
    <p role="status">{notice}</p>
  {:else}<p role="status">
      {status
        ? "Preparing the runtime and starting the engine. First installation needs internet access; later starts reuse the verified installation."
        : "Connecting to Lum…"}
    </p>{/if}
</main>
