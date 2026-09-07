# `winget install --manifest` on a clean machine, 2026-09-07

The evidence for `winget.md`'s "Validation, as run". `winget validate` reads
YAML; this is the part that finds out whether `ArchiveBinariesDependOnPath`
actually does what §1 of that plan says it does, on a Windows that has never had
this product on it.

## Where it ran, and what had to be added

`D:\VMs\folio-cleanvm\folio-win10`, the gate-5 machine of `clean-vm.md`,
reverted to its `clean` snapshot and driven by `vmrun` with the documented
guest account. **Windows 10 Pro 22H2, 10.0.19045.3803** — the oldest Windows
this product claims support for, so the manifest's
`MinimumOSVersion: 10.0.17763.0` is being asked a real question rather than a
rhetorical one.

Not the Windows 11 machine: it is encrypted to carry its vTPM and `vmrun`
answers `A password is required for this operation`, and §2.2 of `clean-vm.md`
keeps that password out of the repository on purpose. Not Windows Sandbox
either — the feature is not enabled on the development machine and enabling it
needs an administrator and a restart.

Two things the clean machine does not have had to be given to it, and both are
undone by the revert this run ends with:

- **its network adapter.** The gate-5 build sets `ethernet0.present = "FALSE"`
  so that the archive's offline behaviour can be measured; a `winget install`
  downloads the installer from the release page, so the adapter was switched on
  for this run and switched off again afterwards.
- **a current App Installer.** The clean image carries
  `Microsoft.DesktopAppInstaller 1.0.30251.0` and no `winget` command at all, so
  winget-cli **v1.29.290** was installed from `microsoft/winget-cli`'s own
  release — the same version the development machine runs, and comfortably past
  the 1.9 that introduced `ArchiveBinariesDependOnPath`.

## `winget validate`

On the development machine, winget-cli v1.29.290:

```
> winget validate --manifest packaging\winget\manifests\w\WeiyiShi\Folio\0.2.2
Manifest validation succeeded.
EXITCODE=0
```

No warnings.

## The install

```
Found Folio [WeiyiShi.Folio] Version 0.2.2
Successfully verified installer hash
Extracting archive...
Successfully extracted archive
Starting package install...
Path environment variable modified; restart your shell to use the new value.
Command line alias added: "folio"
Successfully installed
exit code: 0
```

**`PATH` gained the extracted folder itself**, which is the whole point:

```
…\AppData\Local\Microsoft\WinGet\Packages\WeiyiShi.Folio__DefaultSource\folio-0.2.2
```

and `…\Microsoft\WinGet\Links` stayed **empty** — no symlink was made, before or
after. All nine files are in that one directory:

```
     109,920  WeiyiShi.Folio__DefaultSource\folio-0.2.2\conpty.dll
          31  WeiyiShi.Folio__DefaultSource\folio-0.2.2\folio-here.cmd
  74,275,584  WeiyiShi.Folio__DefaultSource\folio-0.2.2\folio.exe
      16,098  WeiyiShi.Folio__DefaultSource\folio-0.2.2\folio.msix
      10,854  WeiyiShi.Folio__DefaultSource\folio-0.2.2\LICENSE-APACHE
       1,089  WeiyiShi.Folio__DefaultSource\folio-0.2.2\LICENSE-MIT
   1,063,224  WeiyiShi.Folio__DefaultSource\folio-0.2.2\OpenConsole.exe
     524,787  WeiyiShi.Folio__DefaultSource\folio-0.2.2\THIRD-PARTY-NOTICES.md
         428  WeiyiShi.Folio__DefaultSource\folio-0.2.2\TRADEMARK.md
```

A **new** shell, started with the `PATH` the installer had just written rather
than with the one this process inherited, because "on `PATH`" is a claim about
the next shell and not about this one:

```
resolved: C:\Users\folio\AppData\Local\Microsoft\WinGet\Packages\WeiyiShi.Folio__DefaultSource\folio-0.2.2\folio.exe
folio --version exit 0
folio --version: Folio 0.2.2 (95f9be87f3)
conpty.dll beside it: True
OpenConsole.exe beside it: True
folio.msix beside it: True
```

The name `folio` resolves to the real executable in the real folder, the version
it prints is the commit this release was cut from, and the three files it cannot
work without are its siblings.

## The uninstall

```
> winget uninstall --product-code WeiyiShi.Folio__DefaultSource
Found Folio [ARP\User\X64\WeiyiShi.Folio__DefaultSource]
Starting package uninstall...
Successfully uninstalled
exit code: 0
```

The folder is gone, the `PATH` entry is gone, and the registry entry under
`HKCU\…\Uninstall` is gone. `winget list Folio` afterwards: "No installed
package found matching input criteria."

## Two things this test found that a reader should not misread

**1. `winget uninstall WeiyiShi.Folio` does not match, and that is an artefact
of installing from a local manifest.** A package installed with `--manifest` has
no source behind it, so winget records it as
`ARP\User\X64\WeiyiShi.Folio__DefaultSource` and writes its own
`UninstallString` as `winget uninstall --product-code
WeiyiShi.Folio__DefaultSource`. That is the command above, and it works. Once
the package is in `winget-pkgs`, an installation correlates to the source entry
and `winget uninstall WeiyiShi.Folio` is the command a person types. Nothing in
the manifest causes this and nothing in the manifest can change it.

**2. `winget install --manifest` refuses the archive on its own malware scan,
and Windows Defender does not.** Without
`--ignore-local-archive-malware-scan` the install stops with `Archive scan
detected malware`; the log is more precise — `Archive malware scan failed`,
`0x8a150060` in `ArchiveFlow.cpp`. Against the same bytes on the same machine,
with Defender engine 1.1.26080.3 and signatures from the same day:

```
Scanning …\folio-0.2.2-windows-x64.zip found no threats.
Scanning …\folio-0.2.2\folio.exe found no threats.
Scanning …\folio-0.2.2 found no threats.
Get-MpThreat: (none)
```

**This scan only runs for local manifests.** winget's own help for the flag says
so — "the malware scan performed as part of installing an archive type package
**from local manifest**" — and the control run on the same machine agrees: a
package already in `winget-pkgs` that is also `zip` + `portable`
(`sharkdp.fd`, installed from the winget source, no override flag) installed
with no scan step at all. So this is a step in the path a submitter walks and
not a step in the path a user walks; it does not affect anyone who installs
`WeiyiShi.Folio` after the pull request merges.

It is still worth carrying into the submission, because the moderation pipeline
runs an antivirus and security scan of its own on the installer. What can be
said in front of it is what is above: a current Defender, asked directly about
these bytes, finds nothing in the archive, in `folio.exe`, or in the extracted
folder — and `folio.exe` and `folio.msix` are Authenticode-signed by a real
certificate rather than a self-signed one.

To reproduce the failure and the override, both admin settings have to be on:
`winget settings --enable LocalManifestFiles` and
`winget settings --enable LocalArchiveMalwareScanOverride`.
