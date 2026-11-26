<script lang="ts">
  import { clearLocalChats } from "../api/chat-storage";
</script>

<form method="post" action="/auth/logout" onsubmit={clearLocalChats}>
  <button type="submit">Sign out</button>
</form>
