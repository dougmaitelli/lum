import MarkdownIt from "markdown-it";

// Recognize CommonMark fences without interpreting the rest of the message as HTML or Markdown.
const markdown = new MarkdownIt("zero", { html: false }).enable("fence");
type Segment =
  | { kind: "text"; value: string }
  | { kind: "code"; value: string; language: string; open: boolean };

export function segments(text: string): Segment[] {
  if (!text.includes("```") && !text.includes("~~~")) return [{ kind: "text", value: text }];
  const lines = text.split(/\r\n?|\n/);
  const parts: Segment[] = [];
  let start = 0;
  for (const token of markdown.parse(text, {})) {
    if (token.type !== "fence" || !token.map) continue;
    const [from, to] = token.map;
    if (from > start) parts.push({ kind: "text", value: lines.slice(start, from).join("\n") });
    const last = (lines[to - 1] ?? "").trim();
    const open = !(
      to > from + 1 &&
      last.length >= token.markup.length &&
      [...last].every((c) => c === token.markup[0])
    );
    // CommonMark adds a final newline even to an unfinished line. Don't
    // insert it between the highlighted prefix and new streamed text.
    parts.push({
      kind: "code",
      open,
      value: open && !text.endsWith("\n") ? token.content.replace(/\n$/, "") : token.content,
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
