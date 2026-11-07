"""Render a Rust file as an editor-like HTML page (VS Code Dark+ colours).

Chrome then screenshots it at several device scale factors, which gives a
1x capture, a Retina capture and true high-resolution references to score
the upscalers against. Usage: python3 code_html.py <file.rs> <first_line> <out.html>
"""

import html
import re
import sys

KEYWORDS = r"\b(pub|fn|let|mut|use|struct|enum|impl|for|in|if|else|match|return|self|Self|const|mod|crate|where|as|ref|true|false|Some|None|Ok|Err)\b"
TOKEN = re.compile(
    r"(?P<comment>//.*$)|(?P<string>\"(?:\\.|[^\"\\])*\")|(?P<number>\b\d[\d_]*(?:\.\d+)?\b)"
    r"|(?P<kw>" + KEYWORDS[2:-2] + r")\b|(?P<type>\b[A-Z][A-Za-z0-9]+\b)|(?P<call>\b[a-z_][a-z0-9_]*(?=\())"
)
COLOURS = {
    "comment": "#6a9955",
    "string": "#ce9178",
    "number": "#b5cea8",
    "kw": "#569cd6",
    "type": "#4ec9b0",
    "call": "#dcdcaa",
}


def highlight(line: str) -> str:
    out, pos = [], 0
    for m in TOKEN.finditer(line):
        out.append(html.escape(line[pos : m.start()]))
        kind = m.lastgroup
        out.append(f'<span style="color:{COLOURS[kind]}">{html.escape(m.group())}</span>')
        pos = m.end()
    out.append(html.escape(line[pos:]))
    return "".join(out)


def main() -> None:
    path, first, dest = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    lines = open(path, encoding="utf-8").read().splitlines()[first - 1 : first + 60]
    rows = "".join(
        f'<div><span class="ln">{first + i}</span>{highlight(l) or " "}</div>' for i, l in enumerate(lines)
    )
    page = f"""<!doctype html><meta charset="utf-8"><style>
html,body{{margin:0;background:#1e1e1e;color:#d4d4d4;overflow:hidden}}
pre{{margin:0;padding:8px 0;font:12px/18px Menlo,monospace;white-space:pre}}
.ln{{display:inline-block;width:40px;margin-right:16px;text-align:right;color:#858585}}
</style><pre>{rows}</pre>"""
    open(dest, "w", encoding="utf-8").write(page)


if __name__ == "__main__":
    main()
