// Regenerates `bundle.fileAssociations` in tauri.conf.json from the language
// registry, so the installer claims exactly the file types the editor supports.
//
// One entry per extension rather than one lump entry, for two reasons:
//
//  - The ProgID the installer writes is the entry's `name`. Naming each one
//    `JustCode.<ext>` makes it identical to what the in-app File Associations
//    screen writes, so the two agree instead of leaving competing ProgIDs
//    behind for the same extension.
//  - `description` becomes the type name Explorer shows. Per-extension entries
//    give "Object Pascal unit" and "Terraform config" instead of the same
//    "Source file edited with JustCode" against every type.
//
//     node tools/sync-associations.mjs          # rewrite
//     node tools/sync-associations.mjs --check   # report drift, change nothing

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const confPath = path.join(root, "src-tauri", "tauri.conf.json");

const { associationGroups } = await import(
  new URL("../src/languages.js", import.meta.url).href
);

const associations = associationGroups()
  .flatMap((group) => group.extensions)
  .map((entry) => ({
    ext: [entry.extension],
    name: `JustCode.${entry.extension}`,
    description: entry.label,
    role: "Editor",
  }))
  .sort((a, b) => a.ext[0].localeCompare(b.ext[0]));

const conf = JSON.parse(fs.readFileSync(confPath, "utf8"));
const before = conf.bundle.fileAssociations ?? [];
const beforeExts = new Set(before.flatMap((a) => a.ext));
const afterExts = new Set(associations.map((a) => a.ext[0]));

const added = [...afterExts].filter((e) => !beforeExts.has(e));
const removed = [...beforeExts].filter((e) => !afterExts.has(e));

if (process.argv.includes("--check")) {
  const drift = added.length || removed.length || before.length !== associations.length;
  console.log(`extensions: ${beforeExts.size} in config, ${afterExts.size} in registry`);
  if (added.length) console.log(`  missing from config: ${added.join(", ")}`);
  if (removed.length) console.log(`  in config but not supported: ${removed.join(", ")}`);
  if (!drift) console.log("  in sync");
  process.exit(drift ? 1 : 0);
}

conf.bundle.fileAssociations = associations;
fs.writeFileSync(confPath, `${JSON.stringify(conf, null, 2)}\n`, "utf8");

// --------------------------------------------------------------- installer fix
//
// Tauri's NSIS association macro writes the open command *unquoted*:
//
//     D:\Program Files\JustCode\justcode.exe "%1"
//
// With a space in the path, CreateProcess tries `D:\Program.exe` before the real
// executable. The root of a non-system drive is usually writable without
// elevation, so anything running as the user could drop a `Program.exe` there
// and capture every double-click on the types below. This hook rewrites each
// command (and icon) properly quoted once the installer has finished.
const hookPath = path.join(root, "src-tauri", "installer-hooks.nsh");
const lines = associations.flatMap(({ ext: [extension], name }) => [
  `  WriteRegStr SHELL_CONTEXT "Software\\Classes\\${name}\\shell\\open\\command" "" '"$INSTDIR\\\${MAINBINARYNAME}.exe" "%1"'`,
  `  WriteRegStr SHELL_CONTEXT "Software\\Classes\\${name}\\DefaultIcon" "" '"$INSTDIR\\\${MAINBINARYNAME}.exe",0'`,
  // Listing the ProgID here is what actually makes the shell offer and use it.
  // Without it a type Windows has never seen before simply does nothing when
  // double-clicked, even though Software\Classes is registered correctly.
  `  WriteRegStr HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.${extension}\\OpenWithProgids" "${name}" ""`,
]);

// Windows builds the "Open with" list from `Classes\Applications\<exe>` and the
// Settings ▸ Default apps page from `RegisteredApplications`. Tauri registers
// neither, so the app is missing from every picker — and a file type it is not
// already the default for opens the chooser *without JustCode in it*.
const appLines = [
  `  WriteRegStr SHELL_CONTEXT "Software\\Classes\\Applications\\\${MAINBINARYNAME}.exe" "FriendlyAppName" "JustCode"`,
  `  WriteRegStr SHELL_CONTEXT "Software\\Classes\\Applications\\\${MAINBINARYNAME}.exe\\DefaultIcon" "" '"$INSTDIR\\\${MAINBINARYNAME}.exe",0'`,
  `  WriteRegStr SHELL_CONTEXT "Software\\Classes\\Applications\\\${MAINBINARYNAME}.exe\\shell\\open\\command" "" '"$INSTDIR\\\${MAINBINARYNAME}.exe" "%1"'`,
  ...associations.map(
    ({ ext: [extension] }) =>
      `  WriteRegStr SHELL_CONTEXT "Software\\Classes\\Applications\\\${MAINBINARYNAME}.exe\\SupportedTypes" ".${extension}" ""`,
  ),
  `  WriteRegStr SHELL_CONTEXT "Software\\JustCode\\Capabilities" "ApplicationName" "JustCode"`,
  `  WriteRegStr SHELL_CONTEXT "Software\\JustCode\\Capabilities" "ApplicationDescription" "A small, fast code editor"`,
  `  WriteRegStr SHELL_CONTEXT "Software\\JustCode\\Capabilities" "ApplicationIcon" '"$INSTDIR\\\${MAINBINARYNAME}.exe",0'`,
  ...associations.map(
    ({ ext: [extension], name }) =>
      `  WriteRegStr SHELL_CONTEXT "Software\\JustCode\\Capabilities\\FileAssociations" ".${extension}" "${name}"`,
  ),
  `  WriteRegStr SHELL_CONTEXT "Software\\RegisteredApplications" "JustCode" "Software\\JustCode\\Capabilities"`,
];

// Tauri writes DisplayIcon and InstallLocation *quoted*:
//
//     DisplayIcon = "D:\Program Files\JustCode\justcode.exe"
//
// Neither value is a command line, so nothing unquotes them. Add/Remove Programs
// asks the icon loader for a file literally named `"D:\Program`, gets nothing,
// and draws the row with a blank icon — which is what makes the entry look like
// a dead placeholder with no uninstall behind it. The canonical form is an
// unquoted path with an icon index, and an unquoted directory.
const uninstallKey = "Software\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\\${PRODUCTNAME}";
const arpLines = [
  `  WriteRegStr SHELL_CONTEXT "${uninstallKey}" "DisplayIcon" "$INSTDIR\\\${MAINBINARYNAME}.exe,0"`,
  `  WriteRegStr SHELL_CONTEXT "${uninstallKey}" "InstallLocation" "$INSTDIR"`,
  // Lets Windows uninstall without showing the wizard, and gives the Start menu
  // and winget a working silent path.
  `  WriteRegStr SHELL_CONTEXT "${uninstallKey}" "QuietUninstallString" '"$INSTDIR\\uninstall.exe" /S'`,
];

// Everything above is written by us, so the uninstaller has to take it back
// down; Tauri only knows about the keys it wrote itself. Left behind, the
// ProgIDs keep pointing at a deleted executable, so double-clicking a `.go`
// file after uninstalling does nothing at all.
const unregisterLines = associations.map(
  ({ ext: [extension] }) => `  !insertmacro JC_UNREGISTER_EXT "${extension}"`,
);

// Tauri stashes the handler it displaced under `<ProgID>_backup`, but does not
// check whether that handler is *itself* — so the second install over the first
// records `JustCode.go_backup = JustCode.go`. Restoring from that on uninstall
// would aim the file type at a ProgID the uninstaller had just deleted. Copy a
// genuine previous owner into our own `JustCode.previous` (which the app never
// overwrites with itself, so it survives upgrades) and discard the self-
// referential ones.
const preserveLines = associations.map(
  ({ ext: [extension] }) => `  !insertmacro JC_PRESERVE_PRIOR_OWNER "${extension}"`,
);

const hook = `; Generated by tools/sync-associations.mjs — do not edit by hand.
;
; Four things Tauri's own installer does not get right for this app:
;
;  1. It writes the open command without quoting the executable path. With a
;     space in the path ("Program Files"), Windows tries "D:\\Program.exe"
;     first — a hijack a non-admin user can usually set up on a second drive.
;  2. It never registers the application itself, so JustCode does not appear in
;     the "Open with" picker or in Settings > Default apps.
;  3. It quotes DisplayIcon and InstallLocation in the uninstall key, so
;     Add/Remove Programs cannot load the icon and shows a blank row.
;  4. It only removes the keys it wrote, leaving ours pointing at a deleted
;     executable after an uninstall.

; Rescues the displaced handler Tauri recorded, ignoring the case where it
; "displaced" JustCode itself — see the note in tools/sync-associations.mjs.
!macro JC_PRESERVE_PRIOR_OWNER EXT
  ReadRegStr $R0 SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.\${EXT}_backup"
  \${If} $R0 == "JustCode.\${EXT}"
    DeleteRegValue SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.\${EXT}_backup"
  \${ElseIf} $R0 != ""
    ReadRegStr $R1 SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.previous"
    \${If} $R1 == ""
      WriteRegStr SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.previous" "$R0"
    \${EndIf}
  \${EndIf}
!macroend

; Undoes everything this installer wrote for one extension, restoring the
; handler that owned the type before JustCode took it where we recorded one.
!macro JC_UNREGISTER_EXT EXT
  ReadRegStr $R0 SHELL_CONTEXT "Software\\Classes\\.\${EXT}" ""
  \${If} $R0 == "JustCode.\${EXT}"
    ReadRegStr $R1 SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.previous"
    ; Fall back to Tauri's own record, unless it names us.
    \${If} $R1 == ""
      ReadRegStr $R1 SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.\${EXT}_backup"
      \${If} $R1 == "JustCode.\${EXT}"
        StrCpy $R1 ""
      \${EndIf}
    \${EndIf}
    \${If} $R1 == ""
      DeleteRegValue SHELL_CONTEXT "Software\\Classes\\.\${EXT}" ""
    \${Else}
      WriteRegStr SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "" "$R1"
    \${EndIf}
  \${EndIf}
  DeleteRegValue SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.previous"
  DeleteRegValue SHELL_CONTEXT "Software\\Classes\\.\${EXT}" "JustCode.\${EXT}_backup"
  DeleteRegValue SHELL_CONTEXT "Software\\Classes\\.\${EXT}\\OpenWithProgids" "JustCode.\${EXT}"
  DeleteRegKey SHELL_CONTEXT "Software\\Classes\\JustCode.\${EXT}"
  DeleteRegValue HKCU "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.\${EXT}\\OpenWithProgids" "JustCode.\${EXT}"
!macroend

!macro NSIS_HOOK_POSTINSTALL
${lines.join("\n")}

${appLines.join("\n")}

${arpLines.join("\n")}

  Push $R0
  Push $R1
${preserveLines.join("\n")}
  Pop $R1
  Pop $R0

  ; Tell the shell its association cache is stale. Without this a type that was
  ; double-clicked before the install can keep resolving to "no handler" until
  ; Explorer is restarted. SHCNE_ASSOCCHANGED = 0x08000000, SHCNF_IDLIST = 0.
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  Push $R0
  Push $R1
${unregisterLines.join("\n")}
  Pop $R1
  Pop $R0

  DeleteRegKey SHELL_CONTEXT "Software\\Classes\\Applications\\\${MAINBINARYNAME}.exe"
  DeleteRegValue SHELL_CONTEXT "Software\\RegisteredApplications" "JustCode"
  DeleteRegKey SHELL_CONTEXT "Software\\JustCode\\Capabilities"
  ; NSIS parks the chosen installer language under Software\<publisher>\<product>
  ; and never removes it, which also keeps the publisher key alive.
  DeleteRegKey SHELL_CONTEXT "Software\\\${MANUFACTURER}\\\${PRODUCTNAME}"
  DeleteRegKey /ifempty SHELL_CONTEXT "Software\\\${MANUFACTURER}"
  DeleteRegKey /ifempty SHELL_CONTEXT "Software\\JustCode"

  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, i 0, i 0)'
!macroend
`;
fs.writeFileSync(hookPath, hook, "utf8");

console.log(`wrote ${associations.length} file associations`);
if (added.length) console.log(`  added: ${added.join(", ")}`);
if (removed.length) console.log(`  removed: ${removed.join(", ")}`);
console.log(`wrote ${lines.length} quoting fixes to ${path.relative(root, hookPath)}`);
console.log(
  `  ${arpLines.length} uninstall-entry repairs, ${preserveLines.length} backup rescues, ${unregisterLines.length} cleanup entries`,
);
