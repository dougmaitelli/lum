<script lang="ts">
  import { onMount } from "svelte";
  import { clearLocalChats } from "../api/chat-storage";
  onMount(clearLocalChats);
</script>

<svelte:head><title>Sign in · Lum</title></svelte:head>
<main class="notice">
  <h1>Sign in to Lum</h1>
  <p>Use your administrator account to manage models and inference keys.</p>
  <a class="button-link" href="/auth/login">Sign in with OpenID Connect</a>
</main>
