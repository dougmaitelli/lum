<script lang="ts">
  import { onMount } from "svelte";
  import { runtimeStatus, type RuntimeStatus } from "./api/admin";
  import Chat from "./pages/Chat.svelte";
  import Api from "./pages/Api.svelte";
  import Remove from "./pages/Remove.svelte";
  import Setup from "./pages/Setup.svelte";
  const path = location.pathname;
  let status = $state<RuntimeStatus>(),
    error = $state("");
  onMount(() => {
    let closed = false,
      timer: ReturnType<typeof setTimeout>;
    async function poll() {
      try {
        status = await runtimeStatus();
        error = "";
      } catch (e) {
        error = String(e);
      }
      if (!closed && status?.setup.phase !== "ready") timer = setTimeout(poll, 1500);
    }
    void poll();
    return () => {
      closed = true;
      clearTimeout(timer);
    };
  });
</script>

{#if status?.setup.phase !== "ready"}
  <Setup {status} {error} />
{:else if path === "/api"}
  <Api />
{:else if path.startsWith("/remove/")}
  <Remove alias={path.slice("/remove/".length)} />
{:else}
  <Chat />
{/if}
