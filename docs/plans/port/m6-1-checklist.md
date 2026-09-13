<!-- zh pending opus46 -->

# M6-1 — the clean-user walk, in order

The acceptance in `docs/plans/port/macos-plan-2026-09-12.md` § M6 is a person at
the Mac with the download a stranger gets. This file is that acceptance turned
into steps that can be followed without re-reading the plan, with the wrong
answer written beside each right one, because "it worked" is not a result unless
the failure it excludes was named first.

**Who runs it.** The owner, logged into a second macOS account on the Mac, at
the machine. Not an agent: every panel this walk exists to see is drawn by the
window server, and a session over `ssh` has none.

**What it covers and what it does not** is `docs/RELEASING.md` ▸ macOS ▸
*Clean-machine coverage*. Read that first — two of the preconditions below are
only sensible once you know which parts of Gatekeeper belong to the account and
which belong to the machine.

**How long.** Under an hour, of which the M1–M4 re-runs are most of it.

---

## 0. Before the walk

**0.1 — The build under test must not have been opened on this machine.** Not on
the everyday account, not "just to check it starts". What this machine has
already assessed and approved is recorded once for the whole machine, in
`/var/db/SystemPolicyConfiguration/ExecPolicy`, and a build that is in there gets
no first-open panel on any account. A release build carries a signature nobody
has seen before, so this costs nothing — it only has to be kept true.

**0.2 — The second account exists and is an administrator.** Created in System
Settings ▸ Users & Groups, or with `sysadminctl -addUser`; FileVault is off on
this machine, so there is no secure-token step. An administrator, because the
person downloading Folio is nearly always the administrator of their own Mac. (A
standard account additionally proves that the drag to `/Applications` asks for
credentials. That is a different test and not this one.)

**0.3 — The account has never run Folio.** In the new account,
`ls ~/Library/Application\ Support/Folio` must say the directory does not exist.
It is M1's and M2's precondition as much as this one's.

**0.4 — The two refusal artifacts are ready, and not opened.** Made on the
everyday account, handed to the clean account as files:

- **A damaged image.** Copy the disk image, flip one byte well inside it, and put
  the quarantine attribute back on the copy, because copying drops it:

  ```sh
  cp Folio-<version>-macos-arm64.dmg damaged.dmg
  printf '\xff' | dd of=damaged.dmg bs=1 seek=4000000 count=1 conv=notrunc
  xattr -w com.apple.quarantine \
    "$(xattr -p com.apple.quarantine Folio-<version>-macos-arm64.dmg)" damaged.dmg
  ```

- **An unnotarized bundle.** `Folio-unsigned.app.zip`, out of the
  `folio-macos-<version>-unsigned` artifact the release lane produces when it is
  run without the signing secrets: a real ad-hoc signature over the real bytes,
  with nobody behind it.

---

## 1. The download, and the attribute that makes it one

In **Safari**, in the clean account, from the release page:
`Folio-<version>-macos-arm64.dmg`.

Not `curl`, not AirDrop from the other account, not a copy over the network out
of a build directory. The quarantine attribute is the subject of this whole walk
and only a browser sets it. Then, in a Terminal window in that account:

```sh
cd ~/Downloads
xattr -p com.apple.quarantine Folio-<version>-macos-arm64.dmg
shasum -a 256 Folio-<version>-macos-arm64.dmg
```

**Right:** the first prints a semicolon-separated value naming Safari; the second
matches this release's line in the published `SHA256SUMS.txt`.

**Wrong:** `No such xattr` — the file did not arrive the way a reader's does, and
everything after this would be about a different file; download it again in
Safari. A hash that does not match the published one is a release that stops
here.

## 2. Pull the network **before** the first open

Wi-Fi off, or the cable out.

The reason, in one sentence: this Mac keeps one notarization ticket cache for
every account on it (`/var/db/SystemPolicyConfiguration/Tickets`), so once
anything here has assessed this build while online, an offline launch can no
longer tell a stapled ticket from that cache — and *the stapled ticket is used*
is one of the things M6 asks to see. A first open with no network is the only
ordering in which the staple is what answers.

If the network cannot be pulled at this point the walk still runs; what changes
is step 11, which then makes a weaker claim and says so.

## 3. Open the image

Double-click it in Finder.

**Right:** it mounts, and a window shows Folio beside a link to `/Applications`.

**Wrong:** **"Folio-<version>-macos-arm64.dmg" is damaged and can't be opened.**
That sentence on the real download is a release that does not go out. It is also
the sentence step 12 wants to see on a deliberately damaged copy, which is why it
is worth being able to say which file produced it.

## 4. Drag Folio to Applications

In the image's window, drag onto the link.

**Right:** it copies.

**Wrong:** a permission refusal — the account is not an administrator, and
precondition 0.2 was not met.

## 5. The first open

Double-click `Folio` in `/Applications`. **Never Control-click ▸ Open**: that is
the reader's escape hatch, it approves the build for the whole machine, and using
it here destroys the rest of the walk.

**Right:** the ordinary identified-developer confirmation — *"Folio" is an app
downloaded from the Internet. Are you sure you want to open it?* — naming the
developer the certificate carries, which is the same name
`codesign -dv --verbose=4` prints on its `Authority=Developer ID Application:`
line, and the date it was downloaded. Click Open; a window appears.

**Wrong**, and each of these is a release that does not go out:

- an **unidentified developer** refusal;
- *Apple could not verify … is free of malware*;
- **damaged and can't be opened**;
- **no panel at all** — which is not a defect in the build but a broken walk:
  the machine had already assessed this build, precondition 0.1 did not hold, and
  the step proves nothing. Redo it with a build this machine has not seen.

Screenshot the panel. It is the evidence this milestone exists to produce.

## 6. Still offline: the M1 line

`docs/plans/port/macos-plan-2026-09-12.md` § 2 ▸ M1, run from this account
against this bundle rather than restated here: a `zsh` prompt; `echo $0` and
`pwd`; `ls`; a mouse selection with `Cmd+C` and `Cmd+V`; `sleep 30` interrupted
with `Ctrl+C`; Pinyin 你好; `Cmd+T` and `Cmd+W`.

Two things are different on a downloaded build, and they are the reason this is
re-run at all rather than taken from the development machine: the bundle is
read-only inside `/Applications`, and the data directory is being created for the
first time.

## 7. The M2 and M3 lines

§ 2 ▸ M2 — the files column, the preview, editing in place, the watch contracts,
the image, the typeset integral. Then § 2 ▸ M3 — two windows, `Cmd+Q`, relaunch
from Finder, the Dock icon with every window closed, a second launch handing over
rather than writing, and `~/Library/Application Support/Folio` holding
`session.json` and `settings.json`.

M3's relaunch is also the `⌘Q`-and-relaunch step M6 asks for on its own; it is
run once, here.

## 8. Reconnect, then the M4 lines

§ 2 ▸ M4 ① to ⑦. Two of them are the refusal paths this walk owes, and they are
spelled out because a denial that is silently ignored looks exactly like a
feature that works:

- **Notifications.** At the first notification Folio posts, macOS asks. **Deny
  it.** *Right:* the settings row says the grant was refused. *Wrong:* the row
  still offers the feature and nothing ever arrives — a denial the product does
  not admit to. Then grant it in System Settings and run ① again.
- **Accessibility.** With the global shortcut disabled the settings row reads
  *not authorized*. Use the in-app *Enable global shortcut* action and **deny**
  the grant: the row must still read *not authorized*. Then grant it and run ④.

## 9. Services, which is per-account, and is why this account runs it

Right-click a folder in Finder ▸ *Services ▸ Open in Folio*, and again on a
folder whose name contains a space and a CJK character.

**Right:** a tab opens in that folder.

**Wrong:** there is no *Open in Folio* entry. Before calling that a defect:
Services registration is per-account (`~/Library/Preferences/pbs.plist`), and a
freshly installed bundle is sometimes not picked up until the database is
prodded —

```sh
/System/Library/CoreServices/pbs -flush
```

— and the account logged out and back in. Still absent after that is the defect.

## 10. `⌘Q` and relaunch

Step 7's M3 line. It is listed here because § M6 lists it; it is not run twice.

## 11. The stapled ticket

If step 2 succeeded — the first open in step 5 happened with no network — then
that open **is** this step, and the ticket inside the file is what answered.
Record it that way.

If the network could not be pulled, what is left is

```sh
xcrun stapler validate /Applications/Folio.app
xcrun stapler validate ~/Downloads/Folio-<version>-macos-arm64.dmg
```

which says the ticket is in both files, plus an offline relaunch that cannot tell
that ticket from this machine's cache. Record the weaker claim rather than the
stronger one.

## 12. Refusal: the damaged copy

Open `damaged.dmg` from step 0.4 in this account.

**Right:** a refusal, either wording — **"damaged.dmg" is damaged and can't be
opened**, or an attach failure naming a checksum or an invalid image. And from a
terminal:

```sh
spctl -a -vvv -t open --context context:primary-signature damaged.dmg
```

exits non-zero and says `rejected`.

**Wrong:** it mounts and the application inside launches. That is a signature
that does not cover the bytes it is supposed to cover — the one result in this
walk that means the packaging is wrong rather than the build.

## 13. Refusal: the unnotarized copy

Unzip `Folio-unsigned.app.zip` from step 0.4 — downloaded in Safari in this
account, so that it carries the attribute too — and try to open the bundle.

**Right:** macOS refuses it, and

```sh
spctl -a -vvv Folio-unsigned.app
```

exits non-zero and says `rejected`. **Write down the reason line it prints rather
than checking it against one written here.** The exact wording of an ad-hoc
refusal has moved between macOS releases; what is being asserted is the refusal,
and the release lane asserts exactly that and no more.

**Wrong:** `accepted`. A Mac that accepts a bundle anybody could have made is a
Mac with Gatekeeper switched off — check `spctl --status`, and if that is what
happened then nothing earlier in this walk means anything either, and it is run
again from step 1.

---

## What is kept

With the notarization logs, beside the release, not published:

- the screenshot of step 5's panel and of step 12's refusal;
- the terminal transcript of steps 1, 11, 12 and 13 — the `xattr`, `shasum`,
  `spctl` and `stapler` lines with their output;
- one line per acceptance item in steps 6 to 9: what was run, and what happened;
- and, if step 2 could not be done, the sentence saying so.
