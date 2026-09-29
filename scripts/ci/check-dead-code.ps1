# the_dead_code_list_only_shrinks
#
# **"Built, not wired" is a state, and this makes every piece of code in it say
# which kind it is.** A `#[allow(dead_code)]` or `#[expect(dead_code)]` in
# product code means one of two things: the code is waiting for a ticket that
# will call it, or it was never wired and nobody noticed. The attribute alone
# cannot tell them apart, and after a while neither can its reason ("A2 gates
# the answer door on this" outlived A2). So every such site answers one of two
# ways:
#
#   * it is on `docs/plans/DEAD-CODE.tsv`, the list of the sites that were in
#     the tree when this gate was written (0.4.7, U-44), exactly as they were —
#     and that list only shrinks; or
#   * its reason is `"<TICKET-ID> until YYYY-MM-DD: <why>"`, and the date has
#     not passed. The ticket id is one or more uppercase tokens joined by `-`
#     (`U-41`, `T-KEYBOARD-RECORDS`, `S5`). On the day after the date the site
#     fails: expired — wire it, delete it, or re-ticket it.
#
# What else fails: a listed site whose reason changed (the row is the site as
# it was; a site that is now dated leaves the list), and a row whose site is
# gone (removing the site is the good direction, and its row is deleted in the
# same commit, so a row with no site is stale). The rule is `docs/CONVENTIONS.md`
# §八's paragraph on `#[allow]` / `#[expect]`.
#
# WHAT IS PRODUCT CODE. Every `.rs` under `crates/*/src`, less the test files:
# a file named `tests.rs` or `*_tests.rs`, a file under a `tests/` directory
# inside `src`, and every file the crate reaches only through a test module. A
# test module is found by a brace walk over the tokens, never by a regex over
# the file: the walk tracks the items it is inside, and an item whose own
# attributes include `#[cfg(test)]` (or `#[cfg(all(.., test, ..))]`), or a body
# opening with `#![cfg(test)]`, makes everything inside it test code. An
# out-of-line `#[cfg(test)] mod x;` makes `x.rs` (or `x/mod.rs`, or its
# `#[path]`) a test file, and so is every file declared inside one. A file is
# product code when a crate root reaches it through declarations none of which
# is a test's, and also when no declaration reaches it at all (a file the walk
# cannot place is shown rather than hidden). `vendor/` is not under
# `crates/*/src` and is never read.
#
# WHAT IS A SITE. An outer or inner attribute `allow(..)` / `expect(..)` whose
# lint list names `dead_code`, standing alone or inside a `cfg_attr(..)` at any
# depth. Comments and string literals are lexed as what they are, so a doc
# comment quoting `#[allow(dead_code)]` is not a site. The row names the item
# the attribute sits on by its path inside the file: `Type::method`,
# `Enum::Variant`, `Struct.field`, `Tuple.0`, `(module)` for an inner attribute
# at the top of a file. The reason is the `reason = "…"` string's value as
# Rust reads it (an escaped line break and the indentation after it vanish), a
# tab or line break in it written `\t` / `\n`; a site with no reason has an
# empty cell.
#
# THE COMPARISON is the migration-debt gate's (`check-migration-debt.ps1`):
# against the pull request's merge base with `origin/main`, rows compared whole
# and with multiplicity, the commented header not compared. It passes, loudly,
# when the base has no list — that is how the commit that introduces the list
# passes. It refuses (exit 2) when there is no base at all: no `origin/main` in
# the clone, or no merge base, means the comparison did not happen. CI runs it
# in jobs that check out with `fetch-depth: 0`.
#
# The walk is lexical, not a parse (no build, a tree that does not compile is
# still read), and it runs in C# compiled at start-up: the product code is some
# forty megabytes, a character loop in PowerShell takes minutes over it, and the
# same loop in C# takes a second.
#
# `-Rows` prints the rows of the tree as it stands, sorted, and checks nothing:
# it is how the list was written, and how a triage reads the sites.
#
# Prove it fires before trusting it: `scripts/ci/check-dead-code-tests.ps1`
# runs this script against scratch trees, one case per rule, and
# `gates-can-fail` in `.github/workflows/ci.yml` runs that file.

param(
    [switch]$Rows
)

$ErrorActionPreference = "Stop"

if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -Scope Global -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$relative = "docs/plans/DEAD-CODE.tsv"
$list = Join-Path $repo $relative
$columns = "crate`tfile`titem`tform`treason"

Add-Type -Language CSharp -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.Text;

namespace FolioDeadCodeGate
{
    public enum Kind { Ident, Lifetime, Str, Char, Num, Punct }

    public sealed class Tok
    {
        public Kind Kind;
        public string Text;
        public int Line;
        public override string ToString() { return Text; }
    }

    // One attribute, read as Rust's meta grammar: a path, then either a list of
    // metas and literals in parentheses or `= literal`.
    public sealed class Meta
    {
        public string Path = "";
        public List<Meta> Args;      // null when there are no parentheses
        public Tok Value;            // the literal after `=`, if any
        public Tok Literal;          // set when this entry of a list is a bare literal
    }

    public sealed class Site
    {
        public string Item;
        public string Form;
        public string Reason;        // null when the attribute gives none
        public int Line;
        public bool Test;
    }

    public sealed class ModDecl
    {
        public string Name;
        public string PathAttr;      // null without #[path]
        public string[] Dirs;        // the inline modules around it, outermost first
        public bool Test;
        public int Line;
    }

    public sealed class FileScan
    {
        public List<Site> Sites = new List<Site>();
        public List<ModDecl> Decls = new List<ModDecl>();
    }

    enum Body { Items, Fields, Variants, TupleFields, Group }

    sealed class Frame
    {
        public Body Body;
        public string Path;          // "" at the top of a file
        public bool Test;
        public bool IsMod;
        public string Dir;           // for an inline module: its directory component
        public Frame Parent;
        public int Index;            // tuple fields: the field the next attribute sits on
        public int Angle;            // fields and tuple fields: depth inside `<…>`
    }

    public static class Scanner
    {
        static bool IdentStart(char c) { return c == '_' || char.IsLetter(c); }
        static bool IdentPart(char c) { return c == '_' || char.IsLetterOrDigit(c); }

        public static List<Tok> Lex(string s)
        {
            var toks = new List<Tok>();
            int i = 0, n = s.Length, line = 1;
            while (i < n)
            {
                char c = s[i];
                if (c == '\n') { line++; i++; continue; }
                if (char.IsWhiteSpace(c)) { i++; continue; }
                if (c == '/' && i + 1 < n && s[i + 1] == '/')
                {
                    while (i < n && s[i] != '\n') i++;
                    continue;
                }
                if (c == '/' && i + 1 < n && s[i + 1] == '*')
                {
                    int depth = 1; i += 2;
                    while (i < n && depth > 0)
                    {
                        if (s[i] == '\n') { line++; i++; }
                        else if (s[i] == '/' && i + 1 < n && s[i + 1] == '*') { depth++; i += 2; }
                        else if (s[i] == '*' && i + 1 < n && s[i + 1] == '/') { depth--; i += 2; }
                        else i++;
                    }
                    continue;
                }
                if (IdentStart(c))
                {
                    int start = i;
                    while (i < n && IdentPart(s[i])) i++;
                    string id = s.Substring(start, i - start);
                    if ((id == "r" || id == "br" || id == "cr") && i < n && (s[i] == '"' || (s[i] == '#' && RawAhead(s, i))))
                    {
                        int hashes = 0;
                        while (s[i] == '#') { hashes++; i++; }
                        i++;
                        int from = i, at = line;
                        while (i < n && !(s[i] == '"' && Closes(s, i + 1, hashes)))
                        {
                            if (s[i] == '\n') line++;
                            i++;
                        }
                        toks.Add(new Tok { Kind = Kind.Str, Text = s.Substring(from, Math.Min(i, n) - from), Line = at });
                        i += 1 + hashes;
                        continue;
                    }
                    if (id == "r" && i + 1 < n && s[i] == '#' && IdentStart(s[i + 1]))
                    {
                        i++;
                        int from = i;
                        while (i < n && IdentPart(s[i])) i++;
                        toks.Add(new Tok { Kind = Kind.Ident, Text = s.Substring(from, i - from), Line = line });
                        continue;
                    }
                    if ((id == "b" || id == "c") && i < n && s[i] == '"')
                    {
                        int at = line;
                        string value;
                        i = ReadString(s, i, ref line, out value);
                        toks.Add(new Tok { Kind = Kind.Str, Text = value, Line = at });
                        continue;
                    }
                    if (id == "b" && i < n && s[i] == '\'')
                    {
                        i = SkipChar(s, i);
                        toks.Add(new Tok { Kind = Kind.Char, Text = "'", Line = line });
                        continue;
                    }
                    toks.Add(new Tok { Kind = Kind.Ident, Text = id, Line = line });
                    continue;
                }
                if (c == '"')
                {
                    int at = line;
                    string value;
                    i = ReadString(s, i, ref line, out value);
                    toks.Add(new Tok { Kind = Kind.Str, Text = value, Line = at });
                    continue;
                }
                if (c == '\'')
                {
                    // A character literal, or a lifetime / label: `'a'` against `'a`.
                    if (i + 1 < n && s[i + 1] == '\\')
                    {
                        i = SkipChar(s, i);
                        toks.Add(new Tok { Kind = Kind.Char, Text = "'", Line = line });
                        continue;
                    }
                    int width = (i + 1 < n && char.IsHighSurrogate(s[i + 1])) ? 2 : 1;
                    if (i + 1 + width < n && s[i + 1 + width] == '\'')
                    {
                        i += 2 + width;
                        toks.Add(new Tok { Kind = Kind.Char, Text = "'", Line = line });
                        continue;
                    }
                    i++;
                    int from = i;
                    while (i < n && IdentPart(s[i])) i++;
                    toks.Add(new Tok { Kind = Kind.Lifetime, Text = "'" + s.Substring(from, i - from), Line = line });
                    continue;
                }
                if (char.IsDigit(c))
                {
                    int from = i;
                    while (i < n && IdentPart(s[i])) i++;
                    if (i + 1 < n && s[i] == '.' && char.IsDigit(s[i + 1]))
                    {
                        i++;
                        while (i < n && IdentPart(s[i])) i++;
                    }
                    toks.Add(new Tok { Kind = Kind.Num, Text = s.Substring(from, i - from), Line = line });
                    continue;
                }
                toks.Add(new Tok { Kind = Kind.Punct, Text = c.ToString(), Line = line });
                i++;
            }
            return toks;
        }

        static bool RawAhead(string s, int i)
        {
            while (i < s.Length && s[i] == '#') i++;
            return i < s.Length && s[i] == '"';
        }

        static bool Closes(string s, int i, int hashes)
        {
            for (int k = 0; k < hashes; k++)
                if (i + k >= s.Length || s[i + k] != '#') return false;
            return true;
        }

        // A quoted string from its opening quote; returns the index after the closing one.
        static int ReadString(string s, int i, ref int line, out string value)
        {
            var sb = new StringBuilder();
            int n = s.Length;
            i++;
            while (i < n && s[i] != '"')
            {
                char c = s[i];
                if (c == '\\' && i + 1 < n)
                {
                    char e = s[i + 1];
                    i += 2;
                    switch (e)
                    {
                        case 'n': sb.Append('\n'); break;
                        case 't': sb.Append('\t'); break;
                        case 'r': sb.Append('\r'); break;
                        case '0': sb.Append('\0'); break;
                        case '\\': sb.Append('\\'); break;
                        case '\'': sb.Append('\''); break;
                        case '"': sb.Append('"'); break;
                        case 'x':
                            sb.Append((char)Convert.ToInt32(s.Substring(i, 2), 16));
                            i += 2;
                            break;
                        case 'u':
                        {
                            int close = s.IndexOf('}', i);
                            string hex = s.Substring(i + 1, close - i - 1).Replace("_", "");
                            sb.Append(char.ConvertFromUtf32(Convert.ToInt32(hex, 16)));
                            i = close + 1;
                            break;
                        }
                        case '\r':
                        case '\n':
                            // A line continuation: the break and the whitespace after it are not in the string.
                            if (e == '\n') line++;
                            while (i < n && char.IsWhiteSpace(s[i]))
                            {
                                if (s[i] == '\n') line++;
                                i++;
                            }
                            break;
                        default: sb.Append(e); break;
                    }
                    continue;
                }
                if (c == '\n') line++;
                sb.Append(c);
                i++;
            }
            value = sb.ToString();
            return i + 1;
        }

        // A character literal from its opening quote (or the `b` before it has been read).
        static int SkipChar(string s, int i)
        {
            i++;
            if (s[i] == '\\')
            {
                i++;
                if (s[i] == 'u') i = s.IndexOf('}', i) + 1;
                else if (s[i] == 'x') i += 3;
                else i++;
            }
            else
            {
                i += char.IsHighSurrogate(s[i]) ? 2 : 1;
            }
            return i + 1;
        }

        // ── the meta grammar ────────────────────────────────────────────────────────────────────

        // Parses one meta from toks[p..end) and leaves p after it (at a comma or at end).
        static Meta ParseMeta(List<Tok> toks, ref int p, int end)
        {
            var m = new Meta();
            if (p < end && toks[p].Kind != Kind.Ident && toks[p].Kind != Kind.Punct)
            {
                m.Literal = toks[p];
                p++;
                SkipToComma(toks, ref p, end);
                return m;
            }
            var path = new StringBuilder();
            while (p < end)
            {
                var t = toks[p];
                if (t.Kind == Kind.Ident) { path.Append(t.Text); p++; }
                else if (t.Kind == Kind.Punct && t.Text == ":" ) { path.Append(':'); p++; }
                else break;
            }
            m.Path = path.ToString();
            if (p < end && toks[p].Kind == Kind.Punct && toks[p].Text == "(")
            {
                int close = Match(toks, p, end);
                m.Args = new List<Meta>();
                int q = p + 1;
                while (q < close)
                {
                    m.Args.Add(ParseMeta(toks, ref q, close));
                    if (q < close) q++; // the comma
                }
                p = close + 1;
            }
            else if (p < end && toks[p].Kind == Kind.Punct && toks[p].Text == "=")
            {
                p++;
                if (p < end) { m.Value = toks[p]; p++; }
            }
            SkipToComma(toks, ref p, end);
            return m;
        }

        static void SkipToComma(List<Tok> toks, ref int p, int end)
        {
            while (p < end)
            {
                var t = toks[p];
                if (t.Kind == Kind.Punct && t.Text == ",") return;
                if (t.Kind == Kind.Punct && (t.Text == "(" || t.Text == "[" || t.Text == "{")) { p = Match(toks, p, end) + 1; continue; }
                p++;
            }
        }

        // The index of the delimiter that closes the one at `open`, or `end` if it never closes.
        static int Match(List<Tok> toks, int open, int end)
        {
            int depth = 0;
            for (int q = open; q < end; q++)
            {
                var t = toks[q];
                if (t.Kind != Kind.Punct) continue;
                if (t.Text == "(" || t.Text == "[" || t.Text == "{") depth++;
                else if (t.Text == ")" || t.Text == "]" || t.Text == "}") { depth--; if (depth == 0) return q; }
            }
            return end;
        }

        static bool TestPredicate(Meta m)
        {
            if (m.Path == "test" && m.Args == null && m.Value == null) return true;
            if (m.Path == "all" && m.Args != null)
                foreach (var a in m.Args) if (TestPredicate(a)) return true;
            return false;
        }

        static bool IsCfgTest(Meta m)
        {
            return m.Path == "cfg" && m.Args != null && m.Args.Count == 1 && TestPredicate(m.Args[0]);
        }

        // The allow/expect naming dead_code inside this attribute, at any depth of cfg_attr.
        static Meta DeadCode(Meta m)
        {
            if ((m.Path == "allow" || m.Path == "expect") && m.Args != null)
            {
                foreach (var a in m.Args)
                    if (a.Path == "dead_code" && a.Args == null && a.Value == null) return m;
                return null;
            }
            if (m.Path == "cfg_attr" && m.Args != null)
            {
                for (int k = 1; k < m.Args.Count; k++)
                {
                    var found = DeadCode(m.Args[k]);
                    if (found != null) return found;
                }
            }
            return null;
        }

        static string ReasonOf(Meta lint)
        {
            foreach (var a in lint.Args)
                if (a.Path == "reason" && a.Value != null && a.Value.Kind == Kind.Str) return a.Value.Text;
            return null;
        }

        static string PathAttr(List<Meta> attrs)
        {
            foreach (var a in attrs)
                if (a.Path == "path" && a.Value != null && a.Value.Kind == Kind.Str) return a.Value.Text;
            return null;
        }

        // ── the walk ────────────────────────────────────────────────────────────────────────────

        static readonly HashSet<string> ItemWords = new HashSet<string> {
            "fn", "mod", "struct", "enum", "union", "trait", "impl", "type", "const", "static", "use", "macro_rules", "let"
        };

        static readonly HashSet<string> Qualifiers = new HashSet<string> { "fn", "unsafe", "async", "extern" };

        sealed class Walker
        {
            public List<Tok> T;
            public FileScan Out = new FileScan();

            bool IsP(int p, string text) { return p < T.Count && T[p].Kind == Kind.Punct && T[p].Text == text; }
            bool IsI(int p, string text) { return p < T.Count && T[p].Kind == Kind.Ident && T[p].Text == text; }

            // Reads the attribute starting at `#`; returns its meta and leaves p after `]`.
            Meta ReadAttribute(ref int p, out bool inner)
            {
                p++;
                inner = IsP(p, "!");
                if (inner) p++;
                int close = Match(T, p, T.Count);
                int q = p + 1;
                var m = ParseMeta(T, ref q, close);
                p = close + 1;
                return m;
            }

            static string Join(string path, string name, string sep)
            {
                return path.Length == 0 ? name : path + sep + name;
            }

            // What an item header (the tokens before its `{`, `(` or `;`) declares.
            // kind: the item word, or "" for none; name: the item's name.
            void Classify(List<Tok> header, out string kind, out string name)
            {
                kind = ""; name = "";
                for (int k = 0; k < header.Count; k++)
                {
                    var t = header[k];
                    if (t.Kind != Kind.Ident || !ItemWords.Contains(t.Text)) continue;
                    // `const fn`, `const unsafe fn`: the qualifier, not a constant.
                    if (t.Text == "const" && k + 1 < header.Count && header[k + 1].Kind == Kind.Ident && Qualifiers.Contains(header[k + 1].Text)) continue;
                    kind = t.Text;
                    if (kind == "impl") { name = ImplSelf(header, k + 1); return; }
                    int q = k + 1;
                    if (q < header.Count && header[q].Kind == Kind.Ident && header[q].Text == "mut") q++;
                    if (q < header.Count && header[q].Kind == Kind.Punct && header[q].Text == "!") q++;
                    if (q < header.Count && header[q].Kind == Kind.Ident) name = header[q].Text;
                    else if (q < header.Count && header[q].Kind == Kind.Punct && header[q].Text == "_") name = "_";
                    return;
                }
            }

            // The self type of `impl<…> [Trait for] Type<…> [where …]`: its last path segment.
            static string ImplSelf(List<Tok> h, int from)
            {
                int q = from, angle = 0;
                if (q < h.Count && h[q].Text == "<")
                {
                    for (; q < h.Count; q++)
                    {
                        if (h[q].Text == "<") angle++;
                        else if (h[q].Text == ">" && !(q > 0 && h[q - 1].Text == "-")) { angle--; if (angle == 0) { q++; break; } }
                    }
                }
                int start = q, stop = h.Count;
                angle = 0;
                for (int k = q; k < h.Count; k++)
                {
                    var t = h[k];
                    if (t.Text == "<") angle++;
                    else if (t.Text == ">" && !(k > 0 && h[k - 1].Text == "-")) angle--;
                    else if (angle == 0 && t.Kind == Kind.Ident && t.Text == "for") start = k + 1;
                    else if (angle == 0 && t.Kind == Kind.Ident && t.Text == "where") { stop = k; break; }
                }
                string last = "";
                angle = 0;
                for (int k = start; k < stop; k++)
                {
                    var t = h[k];
                    if (t.Text == "<") angle++;
                    else if (t.Text == ">" && !(k > 0 && h[k - 1].Text == "-")) angle--;
                    else if (angle == 0 && t.Kind == Kind.Ident && t.Text != "dyn" && t.Text != "mut" && t.Text != "const")
                        last = t.Text;
                }
                return last;
            }

            // The item an outer attribute at `p` sits on, with whether one of the item's
            // later attributes gates it on cfg(test).
            string Describe(int p, Frame f, out bool test)
            {
                test = false;
                bool dummy;
                while (IsP(p, "#") && IsP(p + 1, "["))
                {
                    var m = ReadAttribute(ref p, out dummy);
                    if (IsCfgTest(m)) test = true;
                }
                if (f.Body == Body.TupleFields) return Join(f.Path, f.Index.ToString(), ".");
                if (IsI(p, "pub"))
                {
                    p++;
                    if (IsP(p, "(")) p = Match(T, p, T.Count) + 1;
                }
                var header = new List<Tok>();
                for (int q = p; q < T.Count && header.Count < 64; q++)
                {
                    var t = T[q];
                    if (t.Kind == Kind.Punct && (t.Text == "{" || t.Text == ";" || t.Text == "=" || t.Text == ",")) break;
                    if (t.Kind == Kind.Punct && t.Text == "(" && header.Count > 0) break;
                    if (t.Kind == Kind.Punct && t.Text == ":" && !IsP(q + 1, ":") && !(q > 0 && T[q - 1].Text == ":")) { header.Add(t); break; }
                    header.Add(t);
                }
                string kind, name;
                Classify(header, out kind, out name);
                if (kind == "impl") return Join(f.Path, "impl " + name, "::");
                if (kind == "use") return Join(f.Path, "use", "::");
                if (kind == "let") return Join(f.Path, "let " + name, "::");
                if (kind.Length > 0) return Join(f.Path, kind == "macro_rules" ? name + "!" : name, "::");
                string first = p < T.Count ? T[p].Text : "";
                if (f.Body == Body.Fields) return Join(f.Path, first, ".");
                return Join(f.Path, first, "::");
            }

            public void Walk()
            {
                var root = new Frame { Body = Body.Items, Path = "", Test = false, IsMod = true };
                int p = 0;
                Run(ref p, root, null);
            }

            string[] Dirs(Frame f)
            {
                var dirs = new List<string>();
                for (var g = f; g != null; g = g.Parent)
                    if (g.IsMod && g.Dir != null) dirs.Insert(0, g.Dir);
                return dirs.ToArray();
            }

            // Walks one body until its closing delimiter (or the end of the file).
            void Run(ref int p, Frame f, string close)
            {
                var header = new List<Tok>();
                var attrs = new List<Meta>();
                while (p < T.Count)
                {
                    var t = T[p];
                    if (t.Kind == Kind.Punct && close != null && t.Text == close) { p++; return; }
                    if (t.Kind == Kind.Punct && t.Text == "#" && (IsP(p + 1, "[") || (IsP(p + 1, "!") && IsP(p + 2, "["))))
                    {
                        int at = p;
                        bool inner;
                        var m = ReadAttribute(ref p, out inner);
                        if (inner)
                        {
                            if (IsCfgTest(m)) f.Test = true;
                            var lint = DeadCode(m);
                            if (lint != null)
                                Out.Sites.Add(new Site {
                                    Item = f.Path.Length == 0 ? "(module)" : f.Path, Form = lint.Path,
                                    Reason = ReasonOf(lint), Line = T[at].Line, Test = f.Test });
                        }
                        else
                        {
                            var lint = DeadCode(m);
                            if (lint != null)
                            {
                                bool later;
                                string item = Describe(p, f, out later);
                                bool earlier = false;
                                foreach (var a in attrs) if (IsCfgTest(a)) earlier = true;
                                Out.Sites.Add(new Site {
                                    Item = item, Form = lint.Path, Reason = ReasonOf(lint), Line = T[at].Line,
                                    Test = f.Test || earlier || later });
                            }
                            attrs.Add(m);
                        }
                        continue;
                    }
                    if (t.Kind == Kind.Punct && (t.Text == "{" || t.Text == "(" || t.Text == "["))
                    {
                        var child = Open(t.Text, header, attrs, f);
                        p++;
                        string closer = t.Text == "{" ? "}" : t.Text == "(" ? ")" : "]";
                        Run(ref p, child, closer);
                        if (t.Text == "{") { header.Clear(); attrs.Clear(); }
                        else header.Add(new Tok { Kind = Kind.Punct, Text = t.Text + closer, Line = t.Line });
                        continue;
                    }
                    if (t.Kind == Kind.Punct && t.Text == ";")
                    {
                        string kind, name;
                        Classify(header, out kind, out name);
                        if (kind == "mod" && header[header.Count - 1].Text == name)
                        {
                            bool test = f.Test;
                            foreach (var a in attrs) if (IsCfgTest(a)) test = true;
                            Out.Decls.Add(new ModDecl { Name = name, PathAttr = PathAttr(attrs), Dirs = Dirs(f), Test = test, Line = t.Line });
                        }
                        header.Clear(); attrs.Clear();
                        p++;
                        continue;
                    }
                    if (f.Body == Body.Fields || f.Body == Body.TupleFields)
                    {
                        if (t.Kind == Kind.Punct && t.Text == "<") f.Angle++;
                        else if (t.Kind == Kind.Punct && t.Text == ">" && !(p > 0 && T[p - 1].Text == "-") && f.Angle > 0) f.Angle--;
                    }
                    if (t.Kind == Kind.Punct && t.Text == "," &&
                        (f.Body == Body.Variants || ((f.Body == Body.Fields || f.Body == Body.TupleFields) && f.Angle == 0)))
                    {
                        if (f.Body == Body.TupleFields) f.Index++;
                        header.Clear(); attrs.Clear();
                        p++;
                        continue;
                    }
                    header.Add(t);
                    p++;
                }
            }

            Frame Open(string open, List<Tok> header, List<Meta> attrs, Frame f)
            {
                bool test = f.Test;
                foreach (var a in attrs) if (IsCfgTest(a)) test = true;
                var child = new Frame { Parent = f, Path = f.Path, Test = test, Body = Body.Group };
                string kind, name;
                Classify(header, out kind, out name);
                if (f.Body == Body.Variants && kind.Length == 0 && header.Count > 0 && header[header.Count - 1].Kind == Kind.Ident)
                {
                    string variant = Join(f.Path, header[header.Count - 1].Text, "::");
                    if (open == "{") { child.Body = Body.Fields; child.Path = variant; }
                    else if (open == "(") { child.Body = Body.TupleFields; child.Path = variant; }
                    return child;
                }
                if (open == "(")
                {
                    // `struct Name<…>(` opens the tuple fields; any other parenthesis is a group.
                    if (kind == "struct" && !HasGroup(header)) { child.Body = Body.TupleFields; child.Path = Join(f.Path, name, "::"); }
                    return child;
                }
                if (open == "[") return child;
                child.Body = Body.Items;
                switch (kind)
                {
                    case "mod":
                        child.Path = Join(f.Path, name, "::");
                        child.IsMod = true;
                        child.Dir = PathAttr(attrs) ?? name;
                        break;
                    case "impl":
                        child.Path = Join(f.Path, name, "::");
                        break;
                    case "trait":
                    case "fn":
                        child.Path = Join(f.Path, name, "::");
                        break;
                    case "struct":
                    case "union":
                        child.Body = Body.Fields;
                        child.Path = Join(f.Path, name, "::");
                        break;
                    case "enum":
                        child.Body = Body.Variants;
                        child.Path = Join(f.Path, name, "::");
                        break;
                }
                return child;
            }

            static bool HasGroup(List<Tok> header)
            {
                foreach (var t in header) if (t.Kind == Kind.Punct && t.Text.Length == 2) return true;
                return false;
            }
        }

        public static FileScan Scan(string text)
        {
            var w = new Walker { T = Lex(text) };
            w.Walk();
            return w.Out;
        }
    }
}
'@

# ── which files are product code ───────────────────────────────────────────────────────────────

function Get-Relative([string]$path) {
    return [IO.Path]::GetRelativePath($repo, $path).Replace('\', '/')
}

function Test-TestName([string]$inSrc) {
    $name = [IO.Path]::GetFileName($inSrc)
    return ($name -eq 'tests.rs') -or $name.EndsWith('_tests.rs') -or ("/$inSrc" -match '/tests/')
}

# Where an out-of-line `mod name;` in `declaring` resolves to (the Rust reference's rules
# for mod-rs and non-mod-rs files, and for #[path] inside and outside inline modules).
function Resolve-Decl([string]$declaring, $decl, [string[]]$roots) {
    $folder = Split-Path -Parent $declaring
    $modRs = ($roots -contains $declaring) -or ([IO.Path]::GetFileName($declaring) -eq 'mod.rs')
    $base = if ($modRs) { $folder } else { Join-Path $folder ([IO.Path]::GetFileNameWithoutExtension($declaring)) }
    if ($decl.PathAttr) {
        $at = if ($decl.Dirs.Count -eq 0) { $folder } else { $base }
        foreach ($dir in $decl.Dirs) { $at = Join-Path $at $dir }
        return @([IO.Path]::GetFullPath((Join-Path $at $decl.PathAttr)))
    }
    $at = $base
    foreach ($dir in $decl.Dirs) { $at = Join-Path $at $dir }
    return @(
        [IO.Path]::GetFullPath((Join-Path $at "$($decl.Name).rs")),
        [IO.Path]::GetFullPath((Join-Path (Join-Path $at $decl.Name) 'mod.rs'))
    )
}

$crates = @(Get-ChildItem -LiteralPath (Join-Path $repo "crates") -Directory | Where-Object {
    Test-Path -LiteralPath (Join-Path $_.FullName "src") -PathType Container
} | Sort-Object Name)

$sites = @()      # rows of product code, in crate / file / line order

foreach ($crate in $crates) {
    $src = Join-Path $crate.FullName "src"
    $files = @(Get-ChildItem -LiteralPath $src -Recurse -File -Filter *.rs | Where-Object {
        (Get-Relative $_.FullName) -notmatch '(^|/)vendor/'
    } | Sort-Object FullName)
    $byPath = @{}
    foreach ($file in $files) {
        $full = [IO.Path]::GetFullPath($file.FullName)
        $byPath[$full] = [FolioDeadCodeGate.Scanner]::Scan([IO.File]::ReadAllText($full))
    }

    # The crate roots: each is a mod-rs file, and the walk starts from them.
    $roots = @($files | Where-Object {
        $inSrc = [IO.Path]::GetRelativePath($src, $_.FullName).Replace('\', '/')
        $inSrc -eq 'lib.rs' -or $inSrc -eq 'main.rs' -or $inSrc -match '^bin/[^/]+\.rs$' -or $inSrc -match '^bin/[^/]+/main\.rs$'
    } | ForEach-Object { [IO.Path]::GetFullPath($_.FullName) })

    # Reached from a crate root through declarations that are not a test's; and reached at all.
    $product = @{}
    $reached = @{}
    $queue = [Collections.Generic.Queue[string]]::new()
    foreach ($root in $roots) { $product[$root] = $true; $reached[$root] = $true; $queue.Enqueue($root) }
    foreach ($path in $byPath.Keys) {
        foreach ($decl in $byPath[$path].Decls) {
            foreach ($target in (Resolve-Decl $path $decl $roots)) {
                if ($byPath.ContainsKey($target)) { $reached[$target] = $true }
            }
        }
    }
    while ($queue.Count -gt 0) {
        $path = $queue.Dequeue()
        foreach ($decl in $byPath[$path].Decls) {
            if ($decl.Test) { continue }
            foreach ($target in (Resolve-Decl $path $decl $roots)) {
                if ($byPath.ContainsKey($target) -and -not $product.ContainsKey($target)) {
                    $product[$target] = $true
                    $queue.Enqueue($target)
                }
            }
        }
    }

    foreach ($file in $files) {
        $full = [IO.Path]::GetFullPath($file.FullName)
        $inSrc = [IO.Path]::GetRelativePath($src, $full).Replace('\', '/')
        if (Test-TestName $inSrc) { continue }
        if ($reached.ContainsKey($full) -and -not $product.ContainsKey($full)) { continue }
        foreach ($site in $byPath[$full].Sites) {
            if ($site.Test) { continue }
            $reason = if ($null -eq $site.Reason) { "" } else { $site.Reason.Replace("`t", '\t').Replace("`r", '\r').Replace("`n", '\n') }
            $sites += [pscustomobject]@{
                Crate  = $crate.Name
                File   = "src/$inSrc"
                Item   = $site.Item
                Form   = $site.Form
                Reason = $reason
                Line   = $site.Line
                Row    = "$($crate.Name)`tsrc/$inSrc`t$($site.Item)`t$($site.Form)`t$reason"
            }
        }
    }
}

if ($Rows) {
    $sites | ForEach-Object { $_.Row }
    exit 0
}

# ── the list ───────────────────────────────────────────────────────────────────────────────────

# A row is a data line: not blank, not a comment, not the column header.
function Read-Rows([string[]]$lines, [string]$where) {
    $out = @()
    $header = $false
    foreach ($line in $lines) {
        if ($line.Length -eq 0 -or $line.StartsWith("#")) { continue }
        if (-not $header) {
            if ($line -ne $columns) { throw "$where`: the columns are '$line', not '$($columns -replace "`t", ' | ')'" }
            $header = $true
            continue
        }
        $cells = $line -split "`t"
        if ($cells.Count -ne 5) { throw "$where`: not five cells: $line" }
        if (@("allow", "expect") -notcontains $cells[3]) { throw "$where`: the form is '$($cells[3])', not allow or expect: $line" }
        $out += $line
    }
    if (-not $header) { throw "$where has no column header" }
    return , $out
}

if (-not (Test-Path -LiteralPath $list)) {
    throw "$relative is not in the tree - it is the list of the dead_code sites this gate allows undated"
}
$listed = Read-Rows ([IO.File]::ReadAllLines($list)) "the working tree's $relative"

function Get-Key([string]$row) {
    $cells = $row -split "`t"
    return ($cells[0..3] -join "`t")
}
function Format-Row([string]$row) { return ($row -replace "`t", ' | ') }

# The rows still waiting for their site, with multiplicity.
$waiting = [Collections.Generic.List[string]]::new()
foreach ($row in $listed) { $waiting.Add($row) }

$today = [DateTime]::UtcNow.Date
$ticket = '^(?<id>[A-Z][A-Z0-9]*(?:-[A-Z0-9]+)*) until (?<date>\d{4}-\d{2}-\d{2}): \S'
$failures = @()
$dated = 0

foreach ($site in $sites) {
    $where = "$($site.Crate)/$($site.File):$($site.Line) $($site.Item) ($($site.Form))"
    $date = $null
    if ($site.Reason -cmatch $ticket) {
        $parsed = [DateTime]::MinValue
        if ([DateTime]::TryParseExact($Matches['date'], 'yyyy-MM-dd', [Globalization.CultureInfo]::InvariantCulture,
                [Globalization.DateTimeStyles]::None, [ref]$parsed)) {
            $date = $parsed
            $until = "$($Matches['id']) until $($Matches['date'])"
            $dated++
        }
    }
    $expired = ($null -ne $date) -and ($date -lt $today)
    if ($waiting.Remove($site.Row)) {
        # on the list, exactly as it was
    } else {
        $same = @($waiting | Where-Object { (Get-Key $_) -eq (Get-Key $site.Row) })
        if ($same.Count -gt 0) {
            [void]$waiting.Remove($same[0])
            $was = ($same[0] -split "`t")[4]
            Write-Host "changed   $where"
            $failures += "$where is on $relative with the reason '$was' and now says '$($site.Reason)': a listed site's reason does not change - a site that is now dated leaves the list in the same commit"
            continue
        }
        if ($null -eq $date) {
            Write-Host "unlisted  $where"
            $failures += "$where is not on $relative and its reason is not '<TICKET-ID> until YYYY-MM-DD: <why>': a door that is built is wired in the same ticket, or dated (docs/CONVENTIONS.md section 8)"
            continue
        }
    }
    if ($expired) {
        Write-Host "expired   $where - $until"
        $failures += "$where is past its date ($until): expired: wire it, delete it, or re-ticket it"
    } elseif ($null -ne $date) {
        Write-Host "dated     $where - $until"
    } else {
        Write-Host "listed    $where"
    }
}
foreach ($row in $waiting) {
    Write-Host "stale     $(Format-Row $row)"
    $failures += "$relative has a row whose site is not in the tree: $(Format-Row $row) - the site went, so the row goes in the same commit"
}

# ── the list only shrinks against the merge base ───────────────────────────────────────────────

Push-Location $repo
try {
    $base = $null
    & git rev-parse --verify --quiet refs/remotes/origin/main *> $null
    if ($LASTEXITCODE -eq 0) {
        # Read git's status before anything else stands on it (check-migration-debt.ps1 says why).
        $found = & git merge-base HEAD origin/main 2>$null
        $status = $LASTEXITCODE
        if ($status -eq 0) { $base = @($found)[0] }
    }
    if ($base) {
        $text = (& git show "${base}:${relative}" 2>$null) | Out-String
        $inBase = ($LASTEXITCODE -eq 0)
    }
} finally {
    Pop-Location
}

if (-not $base) {
    if ($failures.Count -gt 0) {
        Write-Host ($failures | ForEach-Object { "  $_" } | Out-String)
    }
    Write-Host "no merge base with origin/main in this clone - $relative has $($listed.Count) rows and nothing to compare them against."
    Write-Host "This is not a pass: the comparison did not happen. Run 'git fetch origin main' and try again."
    exit 2
}

if (-not $inBase) {
    Write-Host "$relative is not in $($base.Substring(0, 12)) - this is the commit that introduces it, and it has $($listed.Count) rows."
} else {
    $before = Read-Rows ($text -split "`r?`n") "$relative at $($base.Substring(0, 12))"
    $allowed = @{}
    foreach ($row in $before) {
        if ($allowed.ContainsKey($row)) { $allowed[$row] += 1 } else { $allowed[$row] = 1 }
    }
    $added = @()
    foreach ($row in $listed) {
        if ($allowed.ContainsKey($row) -and $allowed[$row] -gt 0) { $allowed[$row] -= 1 } else { $added += $row }
    }
    foreach ($row in $allowed.Keys) {
        for ($k = 0; $k -lt $allowed[$row]; $k++) { Write-Host "removed   $(Format-Row $row)" }
    }
    foreach ($row in $added) {
        Write-Host "added     $(Format-Row $row)"
        $failures += "$relative gained a row the merge base does not have: $(Format-Row $row) - this list only shrinks; a new site is dated instead"
    }
    $removed = $before.Count - ($listed.Count - $added.Count)
    Write-Host "$relative against $($base.Substring(0, 12)): $($before.Count) rows -> $($listed.Count), $($added.Count) added, $removed removed."
}

if ($failures.Count -gt 0) {
    # Written out whole before the throw: an error record is wrapped to the console's width,
    # and a finding is one line that names a site.
    Write-Host "the dead-code gate failed:"
    foreach ($failure in $failures) { Write-Host "  $failure" }
    throw "the dead-code gate failed: $($failures.Count) finding(s), each on its own line above"
}
Write-Host "the dead-code list only shrinks: $($sites.Count) sites, $dated dated."
