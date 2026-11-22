import unittest

from check_comments import ratchet_failures, scan_source


class CommentLexerTests(unittest.TestCase):
    def test_detects_rust_line_doc_and_block_comments_but_exempts_safety(self) -> None:
        src = "\n".join(
            [
                "/// A doc comment.",
                "pub fn demo() {",
                "    // regular comment",
                "    /* block comment */",
                "    // SAFETY: pointer is valid for this call.",
                "    // Continuation of the same SAFETY justification.",
                "    unsafe { std::ptr::null::<u8>(); }",
                '    let url = "https://example.com//not-a-comment";',
                '    let raw = r#"// also not a comment /* nope */"#;',
                "    let slash = '/';",
                "}",
            ]
        )
        found = scan_source("src/demo.rs", src)
        self.assertEqual(
            [(b.start, b.end, b.kind) for b in found],
            [(1, 1, "doc"), (3, 3, "line"), (4, 4, "block")],
        )

    def test_detects_ts_comments_without_tripping_on_strings_regex_templates_or_vite_ignore(self) -> None:
        src = "\n".join(
            [
                "// top comment",
                "const a = 'https://example.com//x';",
                "const b = `prefix ${/* inline */ 1} // inside template`;",
                "const re = /https?:\\/\\/[^/]+/g;",
                "const mod = import(/* @vite-ignore */ path);",
                "/** JSDoc comment",
                " * second line",
                " */",
                "export const ok = 1;",
            ]
        )
        found = scan_source("ui/src/demo.ts", src)
        self.assertEqual(
            [(b.start, b.end) for b in found],
            [(1, 1), (3, 3), (6, 8)],
        )

    def test_exempts_generated_files_and_license_headers(self) -> None:
        self.assertEqual(
            scan_source("ui/src/lib/api/wire.gen.ts", "// Generated\n/* doc */\nexport type X = 1;\n"),
            [],
        )
        self.assertEqual(
            scan_source("ui/src/lib/api/wire/api.gen.ts", "// Generated\nexport type Y = 2;\n"),
            [],
        )
        self.assertEqual(
            scan_source("distribution/libarchive-wasm/bundle/worker-bundle.js", "/* @__PURE__ */ x();\n"),
            [],
        )
        license_src = "// SPDX-License-Identifier: GPL-2.0-or-later\n// Copyright 2026 Example\nexport const x = 1;\n"
        self.assertEqual(scan_source("ui/src/icon.tsx", license_src), [])

    def test_detects_python_hash_comments_only_and_ignores_strings_and_shebang(self) -> None:
        src = "\n".join(
            [
                "#!/usr/bin/env python3",
                '"""Runtime module docstring used via __doc__."""',
                "SQL = '''",
                "# not a python comment inside sql",
                "'''",
                "# real comment",
                "x = 1  # trailing comment",
            ]
        )
        found = scan_source("scripts/demo.py", src)
        self.assertEqual([(b.start, b.end) for b in found], [(6, 6), (7, 7)])

    def test_detects_shell_comments_exempts_shebang_shellcheck_and_param_expansion(self) -> None:
        src = "\n".join(
            [
                "#!/usr/bin/env bash",
                "# shellcheck disable=SC1091",
                "source ./env.sh",
                "count=${#arr[@]}",
                "echo $# '# literal'",
                "# real shell comment",
                "cat <<'EOF'",
                "# inside heredoc",
                "EOF",
            ]
        )
        found = scan_source("distribution/demo.sh", src)
        self.assertEqual([(b.start, b.end) for b in found], [(6, 6)])

    def test_detects_csharp_and_css_comments_and_ignores_verbatim_and_raw_strings(self) -> None:
        cs = "\n".join(
            [
                "/// <summary>Tautology</summary>",
                'var a = @"// verbatim";',
                'var b = """',
                "// inside raw string",
                '""";',
                "// real csharp comment",
            ]
        )
        self.assertEqual([(b.start, b.end) for b in scan_source("plugin/Demo.cs", cs)], [(1, 1), (6, 6)])
        css = "/* css comment */\n.a { background: url(https://example.com/a.png); }\n"
        self.assertEqual([(b.start, b.end) for b in scan_source("ui/src/demo.css", css)], [(1, 1)])

    def test_detects_powershell_line_and_block_comments(self) -> None:
        ps = "\n".join(
            [
                "<#",
                "block comment",
                "#>",
                "$s = @'",
                "# inside here-string",
                "'@",
                "# real ps comment",
            ]
        )
        self.assertEqual([(b.start, b.end) for b in scan_source("scripts/demo.ps1", ps)], [(1, 3), (7, 7)])


class RatchetTests(unittest.TestCase):
    def test_allows_unchanged_or_shrinking_counts_and_follows_renames(self) -> None:
        self.assertEqual(
            ratchet_failures({"a.rs": 3, "old.rs": 2}, {"a.rs": 2, "new.rs": 2}, {"new.rs": "old.rs"}),
            [],
        )

    def test_fails_when_existing_file_or_new_file_adds_comments(self) -> None:
        self.assertEqual(
            ratchet_failures({"a.rs": 1}, {"a.rs": 2, "brand_new.ts": 1}, {}),
            [("a.rs", 1, 2), ("brand_new.ts", 0, 1)],
        )

    def test_fails_even_if_global_total_drops_when_one_file_grows(self) -> None:
        self.assertEqual(
            ratchet_failures({"a.rs": 5, "b.rs": 0}, {"a.rs": 0, "b.rs": 1}, {}),
            [("b.rs", 0, 1)],
        )


if __name__ == "__main__":
    unittest.main()
