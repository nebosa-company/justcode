// Starting content for a new file of each type.
//
// These are deliberately small: enough that the file runs, or at least parses,
// and shows the shape the language expects — not a project scaffold. A type
// with no entry here simply starts empty, which is right for plain text and for
// data formats where any content would be a guess.
//
// `{name}` is replaced with the file's base name.

const TEMPLATES = {
  html: `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>{name}</title>
    <link rel="stylesheet" href="style.css" />
  </head>
  <body>
    <h1>{name}</h1>

    <script src="app.js"></script>
  </body>
</html>
`,

  css: `:root {
  --text: #222;
  --background: #fff;
}

body {
  margin: 0;
  font-family: system-ui, sans-serif;
  color: var(--text);
  background: var(--background);
}
`,

  javascript: `"use strict";

function main() {
  console.log("{name}");
}

main();
`,

  typescript: `export function main(): void {
  console.log("{name}");
}

main();
`,

  jsx: `export default function App() {
  return <h1>{name}</h1>;
}
`,

  tsx: `type Props = {
  title: string;
};

export default function App({ title }: Props) {
  return <h1>{title}</h1>;
}
`,

  markdown: `# {name}

Write here.

## Section

- First
- Second
`,

  python: `"""{name}."""


def main() -> None:
    print("{name}")


if __name__ == "__main__":
    main()
`,

  rust: `fn main() {
    println!("{name}");
}
`,

  // main has one signature (spec §13): the root arena and the arguments arrive
  // as parameters, and `ok` is the success value of the `err` return.
  neper: `use e.mem
use e.io

fn main(a: *mem.Arena, args: []str) -> err {
    try io.print("{name}\\n")
    ret ok
}
`,

  dart: `void main() {
  print('{name}');
}
`,

  go: `package main

import "fmt"

func main() {
	fmt.Println("{name}")
}
`,

  java: `public class {name} {
    public static void main(String[] args) {
        System.out.println("{name}");
    }
}
`,

  csharp: `using System;

internal static class Program
{
    private static void Main()
    {
        Console.WriteLine("{name}");
    }
}
`,

  cpp: `#include <iostream>

int main() {
    std::cout << "{name}" << '\\n';
    return 0;
}
`,

  kotlin: `fun main() {
    println("{name}")
}
`,

  swift: `import Foundation

print("{name}")
`,

  r: `# {name}

main <- function() {
  cat("{name}\\n")
}

main()
`,

  objectpascal: `unit {name};

interface

type
  T{name} = class(TObject)
  public
    procedure Execute;
  end;

implementation

procedure T{name}.Execute;
begin
end;

end.
`,

  json: `{
  "name": "{name}",
  "version": "1.0.0"
}
`,

  yaml: `# {name}
name: {name}
version: 1.0.0
items:
  - first
  - second
`,

  toml: `# {name}

[package]
name = "{name}"
version = "1.0.0"
`,

  xml: `<?xml version="1.0" encoding="UTF-8"?>
<{name}>
  <item name="first" />
</{name}>
`,

  sql: `-- {name}

CREATE TABLE items (
    id   INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

SELECT id, name FROM items ORDER BY name;
`,

  powershell: `<#
.SYNOPSIS
    {name}
#>

[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Write-Host '{name}'
`,

  shell: `#!/usr/bin/env bash
set -euo pipefail

main() {
  echo "{name}"
}

main "$@"
`,

  batch: `@echo off
setlocal enabledelayedexpansion

echo {name}

endlocal
`,

  // GNU as (AT&T) syntax, which is the .s/.S dialect. Intel-syntax assembly
  // (.asm/.nasm) has no template: it would have to pick NASM or MASM, and
  // the two disagree from the first line.
  // Assembles and links as-is on Linux x86-64:
  //   as -o out.o file.s && ld -o out out.o
  assembly: `        .section .rodata
msg:    .ascii  "{name}\\n"
        .set    msg_len, . - msg

        .text
        .globl  _start
_start:
        movq    $1, %rax            # write
        movq    $1, %rdi            # stdout
        leaq    msg(%rip), %rsi
        movq    $msg_len, %rdx
        syscall

        movq    $60, %rax           # exit
        xorq    %rdi, %rdi
        syscall
`,

  terraform: `terraform {
  required_version = ">= 1.5.0"
}

variable "name" {
  type    = string
  default = "{name}"
}

output "name" {
  value = var.name
}
`,

  protobuf: `syntax = "proto3";

package {name};

message Item {
  int32 id = 1;
  string name = 2;
}
`,
};

// The SQL dialects deliberately have no template of their own: they would be
// three more rows in the New File list holding the same script, and `.sqlite`
// names a binary database rather than a script. Plain SQL covers it, and the
// dialect can still be picked from the status bar afterwards.

/**
 * The starting content for a new file, with `{name}` filled in. Returns "" for
 * types with no template, which is what a blank document uses too.
 */
export function templateFor(languageId, fileName) {
  const template = TEMPLATES[languageId];
  if (!template) return "";
  const stem = fileName.replace(/\.[^.]*$/, "");
  return template.replaceAll("{name}", () =>
    NEEDS_IDENTIFIER.has(languageId) ? identifierFrom(stem) : escapeFor(languageId, stem),
  );
}

// Languages where {name} lands somewhere that must be a legal identifier —
// a Java class, a Pascal unit, a proto package, an XML element name.
const NEEDS_IDENTIFIER = new Set(["java", "objectpascal", "protobuf", "xml"]);

/**
 * Turns a file stem into something the languages above will accept: illegal
 * characters become underscores, and a leading digit (or an empty result) gets
 * a prefix, since no identifier may start with one.
 */
function identifierFrom(stem) {
  const cleaned = stem.replace(/[^A-Za-z0-9_]/g, "_");
  return /^[A-Za-z_]/.test(cleaned) ? cleaned : `file_${cleaned}`;
}

/** Keeps a name from breaking out of the string it is inserted into. */
// GNU as strings escape the same two characters JSON strings do.
function escapeFor(languageId, stem) {
  if (languageId === "json" || languageId === "assembly") return stem.replace(/["\\]/g, "\\$&");
  return stem;
}

/** Whether a type offers anything beyond an empty document. */
export function hasTemplate(languageId) {
  return Boolean(TEMPLATES[languageId]);
}
