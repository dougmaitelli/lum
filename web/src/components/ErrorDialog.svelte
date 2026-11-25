<script lang="ts">
  import Dialog from "./Dialog.svelte";
  let dialog: Dialog;
  let title = $state("Error"),
    message = $state("");
  export function show(heading: string, cause: unknown) {
    title = heading;
    message = cause instanceof Error ? cause.message : String(cause);
    dialog.showModal();
  }
</script>

<Dialog bind:this={dialog} {title}>
  <p class="warning" role="alert">{message}</p>
</Dialog>
