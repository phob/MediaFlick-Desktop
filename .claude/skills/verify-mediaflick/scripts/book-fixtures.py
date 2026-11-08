"""Builds the self-authored EPUB fixtures the `books` drive needs.

    python3 -I .claude/skills/verify-mediaflick/scripts/book-fixtures.py OUT_DIR

Writes three EPUBs and prints each file's SHA-256:

- `MediaFlick Script Trap - MediaFlick Tests.epub`: every way a book could
  try to run script. Each probe requests `/api/book-script-trap/<probe>` and
  posts `book-script-ran:<probe>` to its parent, so a probe that ran shows up
  in `app.log` (an unknown endpoint) and in the page.
- `MediaFlick Nested Contents - MediaFlick Tests.epub`: twelve chapters under
  a three-level table of contents.
- `MediaFlick Fixed Layout - MediaFlick Tests.epub`: a pre-paginated EPUB of
  eight pages.

The files are deterministic (fixed timestamps), so the same hashes come out
on every run.
"""

import hashlib
import sys
import zipfile
from pathlib import Path

STAMP = (2026, 11, 8, 12, 0, 0)

CONTAINER = """<?xml version="1.0" encoding="UTF-8"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>
"""


def probe(name: str) -> str:
    return (
        f"new Image().src='/api/book-script-trap/{name}';"
        f"try{{parent.postMessage('book-script-ran:{name}','*')}}catch(e){{}}"
    )


def xhtml(title: str, body: str, head: str = "") -> str:
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml" xmlns:epub="http://www.idpf.org/2007/ops" lang="en">
<head><meta charset="utf-8"/><title>{title}</title>{head}</head>
<body>{body}</body>
</html>
"""


def opf(title: str, items: list[tuple[str, str, str, str]], spine: list[str], extra_meta: str = "") -> str:
    manifest = "\n    ".join(
        f'<item id="{id_}" href="{href}" media-type="{media}"{f" properties={chr(34)}{props}{chr(34)}" if props else ""}/>'
        for id_, href, media, props in items
    )
    itemrefs = "\n    ".join(f'<itemref idref="{id_}"/>' for id_ in spine)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="uid">urn:mediaflick:{title.lower().replace(" ", "-")}</dc:identifier>
    <dc:title>{title}</dc:title>
    <dc:creator>MediaFlick Tests</dc:creator>
    <dc:language>en</dc:language>
    <meta property="dcterms:modified">2026-11-08T12:00:00Z</meta>
    {extra_meta}
  </metadata>
  <manifest>
    {manifest}
  </manifest>
  <spine>
    {itemrefs}
  </spine>
</package>
"""


def write_epub(path: Path, files: dict[str, str]) -> None:
    with zipfile.ZipFile(path, "w") as archive:
        info = zipfile.ZipInfo("mimetype", STAMP)
        info.compress_type = zipfile.ZIP_STORED
        archive.writestr(info, "application/epub+zip")
        for name, text in files.items():
            info = zipfile.ZipInfo(name, STAMP)
            info.compress_type = zipfile.ZIP_DEFLATED
            archive.writestr(info, text)


def lorem(chapter: int, paragraphs: int = 6) -> str:
    return "".join(
        f"<p>Chapter {chapter}, paragraph {n}. This text was written for MediaFlick's reader tests. "
        "It is long enough to fill more than one page of a narrow window, so turning pages "
        "inside a chapter can be told apart from moving to the next chapter.</p>"
        for n in range(1, paragraphs + 1)
    )


def script_trap() -> dict[str, str]:
    body = f"""
<h1>Script trap</h1>
<p id="marker">If any script in this book ran, MediaFlick's sandbox failed.</p>
<script>{probe("inline")}</script>
<script src="/app.js"></script>
<script src="trap.js"></script>
<img src="missing.png" alt="" onerror="{probe("onerror")}"/>
<p><a id="js-link" href="javascript:{probe("javascript-link")}">javascript: link</a></p>
<iframe title="nested" srcdoc="&lt;script&gt;{probe("nested-iframe")}&lt;/script&gt;"></iframe>
<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10" onload="{probe("svg-onload")}"><rect width="10" height="10"/></svg>
{lorem(1, 3)}
"""
    page = xhtml("Script trap", body, head=f'<meta http-equiv="refresh" content="0;url=/api/book-script-trap/refresh"/>')
    nav = xhtml(
        "Contents",
        '<nav epub:type="toc"><ol><li><a href="trap.xhtml">Script trap</a></li></ol></nav>',
    )
    return {
        "META-INF/container.xml": CONTAINER,
        "OEBPS/content.opf": opf(
            "MediaFlick Script Trap",
            [
                ("nav", "nav.xhtml", "application/xhtml+xml", "nav"),
                ("trap", "trap.xhtml", "application/xhtml+xml", "scripted svg"),
                ("trapjs", "trap.js", "application/javascript", ""),
            ],
            ["trap"],
        ),
        "OEBPS/nav.xhtml": nav,
        "OEBPS/trap.xhtml": page,
        "OEBPS/trap.js": probe("external-script"),
    }


NESTED = [
    ("Part One", [("Chapter 1", ["Section 1.1", "Section 1.2"]), ("Chapter 2", []), ("Chapter 3", ["Section 3.1"])]),
    ("Part Two", [("Chapter 4", []), ("Chapter 5", ["Section 5.1", "Section 5.2"]), ("Chapter 6", [])]),
    ("Part Three", [("Chapter 7", []), ("Chapter 8", []), ("Chapter 9", ["Section 9.1"])]),
    ("Part Four", [("Chapter 10", []), ("Chapter 11", []), ("Chapter 12", [])]),
]


def nested_contents() -> dict[str, str]:
    files: dict[str, str] = {"META-INF/container.xml": CONTAINER}
    items = [("nav", "nav.xhtml", "application/xhtml+xml", "nav")]
    spine = []
    toc = []
    number = 0
    for part, chapters in NESTED:
        chapter_entries = []
        for chapter, sections in chapters:
            number += 1
            href = f"ch{number:02}.xhtml"
            section_body = "".join(f'<h2 id="s{i}">{name}</h2>{lorem(number, 3)}' for i, name in enumerate(sections, 1))
            files[f"OEBPS/{href}"] = xhtml(chapter, f"<h1>{chapter}</h1>{lorem(number)}{section_body}")
            items.append((f"ch{number:02}", href, "application/xhtml+xml", ""))
            spine.append(f"ch{number:02}")
            nested = "".join(f'<li><a href="{href}#s{i}">{name}</a></li>' for i, name in enumerate(sections, 1))
            chapter_entries.append(f'<li><a href="{href}">{chapter}</a>{f"<ol>{nested}</ol>" if nested else ""}</li>')
        first = f"ch{number - len(chapters) + 1:02}.xhtml"
        toc.append(f'<li><a href="{first}">{part}</a><ol>{"".join(chapter_entries)}</ol></li>')
    files["OEBPS/nav.xhtml"] = xhtml("Contents", f'<nav epub:type="toc"><ol>{"".join(toc)}</ol></nav>')
    files["OEBPS/content.opf"] = opf("MediaFlick Nested Contents", items, spine)
    return files


def fixed_layout() -> dict[str, str]:
    files: dict[str, str] = {"META-INF/container.xml": CONTAINER}
    items = [("nav", "nav.xhtml", "application/xhtml+xml", "nav")]
    spine = []
    links = []
    for page in range(1, 9):
        href = f"page{page}.xhtml"
        head = '<meta name="viewport" content="width=600, height=800"/>'
        body = (
            f'<div style="width:600px;height:800px;box-sizing:border-box;border:8px solid #345;'
            f'display:flex;align-items:center;justify-content:center;font:bold 96px serif">Page {page}</div>'
        )
        files[f"OEBPS/{href}"] = xhtml(f"Page {page}", body, head)
        items.append((f"p{page}", href, "application/xhtml+xml", ""))
        spine.append(f"p{page}")
        links.append(f'<li><a href="{href}">Page {page}</a></li>')
    files["OEBPS/nav.xhtml"] = xhtml("Contents", f'<nav epub:type="toc"><ol>{"".join(links)}</ol></nav>')
    files["OEBPS/content.opf"] = opf(
        "MediaFlick Fixed Layout",
        items,
        spine,
        '<meta property="rendition:layout">pre-paginated</meta><meta property="rendition:spread">none</meta>',
    )
    return files


def main() -> None:
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for name, files in (
        ("MediaFlick Script Trap - MediaFlick Tests.epub", script_trap()),
        ("MediaFlick Nested Contents - MediaFlick Tests.epub", nested_contents()),
        ("MediaFlick Fixed Layout - MediaFlick Tests.epub", fixed_layout()),
    ):
        path = out / name
        write_epub(path, files)
        print(hashlib.sha256(path.read_bytes()).hexdigest(), name)


main()
