<script lang="ts">
  import Dialog from "./Dialog.svelte";
  let dialog: Dialog;
  let title = $state("Error"),
    message = $state("");
  const pending: [string, string][] = [];
  let showing = false;
  function next() {
    const item = pending.shift();
    showing = Boolean(item);
    if (!item) return;
    [title, message] = item;
    dialog.showModal();
  }
  export function show(heading: string, cause: unknown) {
    pending.push([heading, cause instanceof Error ? cause.message : String(cause)]);
    if (!showing) next();
  }
</script>

<Dialog bind:this={dialog} {title} onclose={next}>
  <p class="warning" role="alert">{message}</p>
</Dialog>
