import type { Turn } from "./admin";

// SSE records and even individual UTF-8 characters can span network chunks.
export async function streamReply(
  alias: string,
  prompt: string,
  history: Turn[],
  signal: AbortSignal,
  append: (text: string) => void,
) {
  const res = await fetch(`/admin/chat/${alias}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "text/event-stream" },
    body: JSON.stringify({ prompt, history }),
    signal,
  });
  if (!res.ok) throw Error((await res.text()) || `Request failed (${res.status})`);
  if (!res.body || !res.headers.get("Content-Type")?.startsWith("text/event-stream"))
    throw Error("Expected a chat stream");
  const reader = res.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "",
    data: string[] = [],
    reason: string | null = null,
    tokens: number | null = null;
  try {
    for (;;) {
      const { value, done } = await reader.read();
      if (done) throw Error("The response stream ended unexpectedly.");
      buffer += value;
      let end: number;
      while ((end = buffer.indexOf("\n")) !== -1) {
        const line = buffer.slice(0, end).replace(/\r$/, "");
        buffer = buffer.slice(end + 1);
        if (line.startsWith("data:")) {
          data.push(line.slice(5).replace(/^ /, ""));
          continue;
        }
        if (line || !data.length) continue;
        const payload = data.join("\n");
        data = [];
        if (payload.trim() === "[DONE]") return { reason, tokens };
        const event = JSON.parse(payload);
        if (event.error) throw Error(event.error.message || "Loom stopped the response.");
        if (typeof event.usage?.completion_tokens === "number")
          tokens = event.usage.completion_tokens;
        const choice = event.choices?.[0];
        if (typeof choice?.finish_reason === "string") reason = choice.finish_reason;
        const delta = choice?.delta;
        const text =
          typeof delta?.content === "string"
            ? delta.content
            : Array.isArray(delta?.content)
              ? delta.content
                  .filter(
                    (p: { type: string; text?: string }) =>
                      p?.type === "text" && typeof p.text === "string",
                  )
                  .map((p: { text: string }) => p.text)
                  .join("")
              : "";
        if (text) append(text);
        else if (typeof delta?.refusal === "string" && delta.refusal) append(delta.refusal);
      }
    }
  } finally {
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
