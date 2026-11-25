<script lang="ts">
  import { onMount } from "svelte";
  import CopyableText from "../components/CopyableText.svelte";
  import { apiInfo, createKey, revokeKey, type ApiInfo } from "../api/admin";
  let info = $state<ApiInfo>(),
    name = $state(""),
    busy = $state(false),
    secret = $state(""),
    error = $state("");
  async function refresh() {
    info = await apiInfo();
  }
  onMount(() => {
    void refresh().catch((e) => (error = String(e)));
  });
  async function change(id?: number) {
    busy = true;
    error = "";
    secret = "";
    try {
      if (id === undefined) {
        const key = await createKey(name);
        secret = key.secret;
        name = "";
      } else {
        await revokeKey(id);
      }
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
</script>

<svelte:head><title>API management · Lum</title></svelte:head>
<svelte:window onpagehide={() => (secret = "")} />
<main class="notice api-page">
  <a href="/">← Chat</a>
  <h1>API management</h1>
  {#if error}<p class="warning" role="alert">{error}</p>{/if}
  {#if info}
    <section>
      <h2>OpenAI-compatible inference</h2>
      <div>Client base URL</div>
      <CopyableText text={info.base} label="Copy client base URL" />
    </section>
    <section>
      <h2>API keys</h2>
      <p class="muted">
        Keys grant inference access, not admin access. Anyone who can reach this admin UI can manage
        keys.
      </p>
      <div class="key-row">
        <form
          class="key-create"
          onsubmit={(e) => {
            e.preventDefault();
            void change();
          }}
        >
          <label>
            Key name
            <input bind:value={name} maxlength="80" required placeholder="e.g. editor" />
          </label>
          <button class="primary" disabled={busy}>Create key</button>
        </form>
        <!-- Reserve the result's actual wrapped size, also on narrow screens. -->
        <div
          class="secret"
          class:reserved={!secret}
          inert={!secret}
          id={secret ? "new-key" : undefined}
        >
          <p>Copy this key now. It won’t be shown again.</p>
          <CopyableText text={secret || "0".repeat(47)} label="Copy generated API key" />
        </div>
      </div>
      {#if info.keys.length}<ul class="key-list">
          {#each info.keys as key (key.id)}<li>
              <div>
                <strong>{key.name}</strong>
                <small>Created {key.created} UTC</small>
              </div>
              <button disabled={busy} onclick={() => change(key.id)}>Revoke</button>
            </li>{/each}
        </ul>{:else}<p class="muted">No active keys.</p>{/if}
    </section>
  {:else if !error}<p>Loading API settings…</p>{/if}
</main>
