import MarkdownIt from "markdown-it";

// Recognize CommonMark fences without interpreting the rest of the message as HTML or Markdown.
const markdown = new MarkdownIt("zero", { html: false }).enable("fence");
type Segment = { kind: "text"; value: string } | { kind: "code"; value: string; language: string };

export function segments(text: string): Segment[] {
  if (!text.includes("```") && !text.includes("~~~")) return [{ kind: "text", value: text }];
  const lines = text.split(/\r\n?|\n/);
  const parts: Segment[] = [];
  let start = 0;
  for (const token of markdown.parse(text, {})) {
    if (token.type !== "fence" || !token.map) continue;
    const [from, to] = token.map;
    if (from > start) parts.push({ kind: "text", value: lines.slice(start, from).join("\n") });
    parts.push({
      kind: "code",
      value: token.content,
      language: token.info.trim().split(/\s+/, 1)[0],
    });
    start = to;
  }
  if (start < lines.length) {
    const rest = lines.slice(start).join("\n");
    if (rest) parts.push({ kind: "text", value: rest });
  }
  return parts;
}
