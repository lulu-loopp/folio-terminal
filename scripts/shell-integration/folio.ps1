# Folio OSC 133 + OSC 7 shell integration for PowerShell 7 and Windows PowerShell 5.1.
# Opt in from $PROFILE after prompt customizers (for example oh-my-posh):
#   . 'D:\path\to\folio\scripts\shell-integration\folio.ps1'

if ($Global:__FolioShellIntegration -and
    $Global:__FolioShellIntegration.Installed) {
    return
}

# Hyperlink capability declaration, honest and scoped: Folio renders OSC 8, but
# identity-allowlisted CLIs (Claude Code 2.1.220 keys on WT_SESSION / TERM_PROGRAM=iTerm.app)
# downgrade links for unknown terminals. FORCE_HYPERLINK is their documented capability override,
# so declare it — only inside Folio sessions, and never clobber a user's explicit choice.
# Known trade-off: supports-hyperlinks-family CLIs honor this even with redirected output, so a
# command piping to a file may carry OSC 8 bytes; revisit as a setting when the settings slice lands.
#
# The literal below is the terminal's own TERM_PROGRAM declaration, character for character —
# `bt_pty::TERM_PROGRAM`, pinned by `the_integration_script_knows_the_name_this_terminal_announces`.
if ($env:TERM_PROGRAM -eq 'Folio' -and -not (Test-Path env:FORCE_HYPERLINK)) {
    $env:FORCE_HYPERLINK = '1'
}

# PSConsoleHostReadLine is the supported console-host extension point. Importing PSReadLine here
# makes its original entry point available on both supported PowerShell generations.
Import-Module PSReadLine -ErrorAction SilentlyContinue

$originalPrompt = (Get-Command prompt -CommandType Function -ErrorAction Stop).ScriptBlock
$readLineCommand = Get-Command PSConsoleHostReadLine -ErrorAction SilentlyContinue
if (-not $readLineCommand) {
    Write-Warning 'Folio shell integration requires PSReadLine; no changes were installed.'
    return
}
$originalReadLine = if ($readLineCommand -is [System.Management.Automation.FunctionInfo]) {
    $readLineCommand.ScriptBlock
} else {
    $readLineCommand
}

$Global:__FolioShellIntegration = @{
    Installed = $true
    # The prompts this one stands in front of, outermost first. Index 0 is what `prompt` was when
    # Folio wrapped it; a customizer that wraps *us* afterwards is adopted onto the front of this
    # list by `prompt` itself (see the re-hoist there), so the chain is always "every prompt in the
    # session, in the order they must run", and Folio is always the one the host calls first.
    PromptChain = @($originalPrompt)
    # How deep in that chain this session currently is. Only depth 0 — the invocation the host
    # itself made — reads `$?` and writes markers; a copy of this same function reached again
    # through a wrapper forwards and stays silent.
    PromptDepth = 0
    OriginalReadLine = $originalReadLine
    # True between the `133;C` written for a submitted command and the `133;D` that answers it.
    # It is armed by `PSConsoleHostReadLine` when a line that will actually run something is
    # submitted, and disarmed by the `D` — so a prompt drawn for any other reason (PSReadLine's
    # `InvokePrompt` after a resize, a nested prompt) reports no status, and a line that runs
    # nothing gets neither marker.
    CommandStarted = $false
    # `$LASTEXITCODE` as it stood when the submitted line was about to run. A native command that
    # fails moves it; that movement is the one signal for "this command exited non-zero" that no
    # prompt customizer running before us can launder, because `$?` can be.
    ExitCodeAtLineStart = $null
    # False for a submitted line PowerShell's parser refused. Such a line runs nothing, so the two
    # variables above keep the previous command's answer and cannot speak for this one.
    LineParsed = $true
    # OSC 7 working-directory URI builder, kept in this table rather than as a global function so
    # the integration adds no command name to the user's session.
    WorkingDirectoryUri = {
        param([string]$literalPath)
        # RFC 3986 path characters that never need escaping: unreserved + sub-delims + ':' + '@',
        # plus '/', which is the separator itself. Everything else — space, '%', '#', '?' and every
        # non-ASCII character — is percent-encoded byte by byte from UTF-8, the only encoding a
        # file URI has. This is minimal and correct: 'D:/My Pictures/图片' becomes
        # 'D:/My%20Pictures/%E5%9B%BE%E7%89%87' and nothing else moves.
        $safe = '-._~!$&''()*+,;=:@/'
        $builder = New-Object System.Text.StringBuilder
        foreach ($byte in [System.Text.Encoding]::UTF8.GetBytes($literalPath.Replace('\', '/'))) {
            if (($byte -ge 0x41 -and $byte -le 0x5A) -or
                ($byte -ge 0x61 -and $byte -le 0x7A) -or
                ($byte -ge 0x30 -and $byte -le 0x39) -or
                $safe.IndexOf([char]$byte) -ge 0) {
                [void]$builder.Append([char]$byte)
            } else {
                [void]$builder.AppendFormat('%{0:X2}', $byte)
            }
        }
        # Empty authority: the file-URI spelling of "this host". Folio also accepts
        # 'localhost' and this machine's name, but the empty form needs no lookup per prompt and
        # cannot go stale.
        return 'file:///' + $builder.ToString().TrimStart('/')
    }
    # A private PSReadLine key handler is still a dispatched editing command. Both supported
    # versions terminate an active history walk after any command that leaves their history
    # command counters unchanged. Capture the proven 2.4.5/2.0.0 state before the resize chord and
    # restore it after the handler body. Advancing only the counters of an already-active history
    # session is PSReadLine's own continuation protocol; it prevents InputLoop's post-dispatch
    # cleanup from clearing `_savedCurrentLine`/`_hashedHistory` and resetting the index to the end.
    HistoryNavigationCapture = {
        try {
            $type = [Microsoft.PowerShell.PSConsoleReadLine]
            $static = [Reflection.BindingFlags]'Static,NonPublic'
            $instance = [Reflection.BindingFlags]'Instance,NonPublic'
            $singletonField = $type.GetField('_singleton', $static)
            $fieldNames = @(
                '_currentHistoryIndex',
                '_getNextHistoryIndex',
                '_recallHistoryCommandCount',
                '_searchHistoryCommandCount',
                '_anyHistoryCommandCount',
                '_searchHistoryPrefix',
                '_searchHistoryBackward',
                '_previousHistoryItem',
                '_savedCurrentLine',
                '_hashedHistory'
            )
            if ($null -eq $singletonField) {
                throw 'PSReadLine _singleton was not found.'
            }
            $singleton = $singletonField.GetValue($null)
            $fields = @{}
            $values = @{}
            foreach ($name in $fieldNames) {
                $field = $type.GetField($name, $instance)
                if ($null -eq $field) {
                    throw "PSReadLine $name was not found."
                }
                $fields[$name] = $field
                $values[$name] = $field.GetValue($singleton)
            }
            return @{
                Singleton = $singleton
                Fields = $fields
                Values = $values
            }
        } catch {
            if ($env:BT_PSREADLINE_HISTORY_PROBE -eq '1') {
                [Console]::Write(
                    ([string][char]27) + ']777;BT_PSREADLINE_HISTORY_DEGRADED=capture:' +
                    $_.Exception.Message + [char]7)
            }
            return $null
        }
    }
    HistoryNavigationRestore = {
        param($snapshot)
        if ($null -eq $snapshot) {
            return
        }
        if ($env:BT_PSREADLINE_HISTORY_TRANSPARENCY_RED_PROBE -eq '1') {
            if ($env:BT_PSREADLINE_HISTORY_PROBE -eq '1') {
                [Console]::Write(
                    ([string][char]27) + ']777;BT_PSREADLINE_HISTORY=red-skipped' + [char]7)
            }
            return
        }

        $fields = $snapshot.Fields
        $values = $snapshot.Values
        $singleton = $snapshot.Singleton
        try {
            # `_savedCurrentLine` is readonly in both proven versions. Its object is mutated only by
            # the post-dispatch cleanup we are preventing, so verify its identity instead of trying
            # to write a readonly field through reflection.
            if (-not [object]::ReferenceEquals(
                $fields['_savedCurrentLine'].GetValue($singleton),
                $values['_savedCurrentLine'])) {
                throw 'PSReadLine _savedCurrentLine changed inside the resize handler.'
            }
            foreach ($name in @(
                '_currentHistoryIndex',
                '_getNextHistoryIndex',
                '_searchHistoryPrefix',
                '_searchHistoryBackward',
                '_previousHistoryItem',
                '_hashedHistory'
            )) {
                $fields[$name].SetValue($singleton, $values[$name])
            }

            $anyCount = [int]$values['_anyHistoryCommandCount']
            $recallCount = [int]$values['_recallHistoryCommandCount']
            $searchCount = [int]$values['_searchHistoryCommandCount']
            if ($anyCount -gt 0) {
                # InputLoop snapshots these counters before dispatch. A changed counter tells it the
                # current key remains part of the live history operation. Do not start a history
                # session at an ordinary empty/current line where all counters were zero.
                $fields['_anyHistoryCommandCount'].SetValue($singleton, $anyCount + 1)
                $fields['_recallHistoryCommandCount'].SetValue(
                    $singleton,
                    $(if ($recallCount -gt 0) { $recallCount + 1 } else { $recallCount }))
                $fields['_searchHistoryCommandCount'].SetValue(
                    $singleton,
                    $(if ($searchCount -gt 0) { $searchCount + 1 } else { $searchCount }))
                $mode = 'active'
            } else {
                $fields['_anyHistoryCommandCount'].SetValue($singleton, $anyCount)
                $fields['_recallHistoryCommandCount'].SetValue($singleton, $recallCount)
                $fields['_searchHistoryCommandCount'].SetValue($singleton, $searchCount)
                $mode = 'inactive'
            }
            if ($env:BT_PSREADLINE_HISTORY_PROBE -eq '1') {
                [Console]::Write(
                    ([string][char]27) + ']777;BT_PSREADLINE_HISTORY=' + $mode +
                    ',index=' + $values['_currentHistoryIndex'] +
                    ',any=' + $anyCount + ',recall=' + $recallCount +
                    ',search=' + $searchCount + [char]7)
            }
        } catch {
            $restoreError = $_
            # If the proven private shape ever changes mid-handler, roll every writable field back
            # to its entry value and let PSReadLine take its normal post-dispatch path. That is the
            # pre-fix behavior, rather than a half-restored navigation session.
            try {
                foreach ($name in @(
                    '_currentHistoryIndex',
                    '_getNextHistoryIndex',
                    '_recallHistoryCommandCount',
                    '_searchHistoryCommandCount',
                    '_anyHistoryCommandCount',
                    '_searchHistoryPrefix',
                    '_searchHistoryBackward',
                    '_previousHistoryItem',
                    '_hashedHistory'
                )) {
                    $fields[$name].SetValue($singleton, $values[$name])
                }
            } catch {
                # The shape is already untrusted; there is no safer private-state action left.
            }
            if ($env:BT_PSREADLINE_HISTORY_PROBE -eq '1') {
                [Console]::Write(
                    ([string][char]27) + ']777;BT_PSREADLINE_HISTORY_DEGRADED=restore:' +
                    $restoreError.Exception.Message + [char]7)
            }
        }
    }
}

# Private resize-anchor chord. ConPTY translates CSI 24;8~ into Ctrl+Alt+Shift+F12 on both Windows
# PowerShell 5.1/PSReadLine 2.0.0 and pwsh/PSReadLine 2.4.5. Only 2.4.x has the private state shape
# proven below. The real-ConPTY probe established that 2.0.0 implements InvokePrompt with ED 2,
# which clears the visible viewport, so unsupported/unproven versions consume the same chord as a
# no-op rather than leaking it into the input buffer.
$psReadLineVersion = (Get-Module PSReadLine).Version
# A PSReadLine that derives its edit anchor from the prompt's own cell width — the
# Folio fork, 2.4.6-bt.anchorfix and later — keeps its coordinates true across
# every resize (the `%=` quotient loss upstream never fixed, probed 2026-08-13), so the
# reflection repair below is dead weight there. The no-op branch still consumes the
# chord, exactly as it does for unproven versions, so it cannot leak into the buffer.
$psReadLineSelfAnchors = $psReadLineVersion -ge [version]'2.4.6'
if (-not $psReadLineSelfAnchors -and $psReadLineVersion.Major -eq 2 -and $psReadLineVersion.Minor -eq 4) {
    Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F12' -ScriptBlock {
        param($key, $arg)
        $historyNavigation = & $Global:__FolioShellIntegration.HistoryNavigationCapture
        try {
        $line = $null
        $cursor = 0
        [Microsoft.PowerShell.PSConsoleReadLine]::GetBufferState([ref]$line, [ref]$cursor)
        # InvokePrompt erases only `_initialY`, then prints the complete prompt and input again.
        # After ConPTY has already reflowed those cells correctly, that output can only duplicate
        # them. Repair PSReadLine's cached B coordinate instead. With an empty buffer the physical
        # cursor is B. With text, the cursor is D display cells after B; use PSReadLine's own cell
        # width routine so CJK and the editor's ^X rendering of controls stay exactly in agreement.
        # A non-empty buffer must also retain the render lines already on screen: an empty
        # `_previousRender` makes the next history/edit diff forget which glyphs it must erase.
        # Reflection is deliberately version-gated with the handler. If the private shape changes,
        # or the derived B coordinate cannot describe the physical cursor, retain InvokePrompt as
        # the known fallback instead of installing a guessed anchor.
        $savedInitialX = $null
        $savedInitialY = $null
        $savedPrevious = $null
        $savedPreviousBufferWidth = $null
        $savedPreviousBufferHeight = $null
        $savedPreviousCursorLeft = $null
        $savedPreviousCursorTop = $null
        $savedPreviousInitialY = $null
        $repairMode = 'unknown'
        $singleton = $null
        $initialXField = $null
        $initialYField = $null
        $previousField = $null
        $waitingField = $null
        try {
            $type = [Microsoft.PowerShell.PSConsoleReadLine]
            $static = [Reflection.BindingFlags]'Static,NonPublic'
            $instance = [Reflection.BindingFlags]'Instance,NonPublic'
            $singleton = $type.GetField('_singleton', $static).GetValue($null)
            $initialXField = $type.GetField('_initialX', $instance)
            $initialYField = $type.GetField('_initialY', $instance)
            $previousField = $type.GetField('_previousRender', $instance)
            $waitingField = $type.GetField('_waitingToRender', $instance)
            $previous = $previousField.GetValue($singleton)
            $waitingToRender = [bool]$waitingField.GetValue($singleton)
            $savedInitialX = $initialXField.GetValue($singleton)
            $savedInitialY = $initialYField.GetValue($singleton)
            $savedPrevious = $previous
            $savedPreviousBufferWidth = $previous.bufferWidth
            $savedPreviousBufferHeight = $previous.bufferHeight
            $savedPreviousCursorLeft = $previous.cursorLeft
            $savedPreviousCursorTop = $previous.cursorTop
            $savedPreviousInitialY = $previous.initialY
            if ($env:BT_PSREADLINE_REANCHOR_FORCE_FALLBACK_PROBE -eq '1') {
                throw 'Dev probe forced the PSReadLine reanchor fallback.'
            }
            $width = [Console]::BufferWidth
            $height = [Console]::BufferHeight
            $physicalX = [Console]::CursorLeft
            $physicalY = [Console]::CursorTop
            if ($width -le 0 -or $height -le 0 -or
                $physicalX -lt 0 -or $physicalX -ge $width -or
                $physicalY -lt 0 -or $physicalY -ge $height -or
                $cursor -lt 0 -or $cursor -gt $line.Length) {
                throw 'Invalid console or PSReadLine cursor state.'
            }

            if ($line.Length -eq 0) {
                $anchor = $physicalY * $width + $physicalX
                $repairMode = 'empty'
            } else {
                # B belongs to the input's logical line, not to the whole screen's rectangular cell
                # array. A hard-terminated row before the prompt loses its right-hand padding when
                # the pane widens, so its old whole-screen cell ordinal cannot be carried through a
                # reflow. The simple single-line path below therefore solves B directly from D.
                $oldWidth = [int]$previous.bufferWidth
                $oldX = [int]$initialXField.GetValue($singleton)
                $oldY = [int]$initialYField.GetValue($singleton)
                if ($oldWidth -le 0 -or $oldX -lt 0 -or $oldX -ge $oldWidth -or $oldY -lt 0) {
                    throw 'Invalid previous PSReadLine anchor state.'
                }
                $cellMethod = $type.GetMethod(
                    'LengthInBufferCells',
                    $static,
                    $null,
                    [type[]]@([char]),
                    $null)
                if ($null -eq $cellMethod) {
                    throw 'PSReadLine LengthInBufferCells(char) was not found.'
                }

                # PSReadLine can defer rendering while keys are queued. In that window GetBufferState
                # already exposes the new history entry, but the physical cursor and `_previousRender`
                # still describe the old entry. Carry the painted B..D distance in that case; using
                # the new buffer against the old cursor is what made InvokePrompt weld the recall to
                # the preceding banner in anchor-glide-verify.vt.
                $anchorSolved = $false
                $linearCurrent = $physicalX -ne ($width - 1)
                for ($index = 0; $linearCurrent -and $index -lt $cursor; $index++) {
                    $character = $line[$index]
                    $linearCurrent = $character -ne "`n" -and
                        [int]$cellMethod.Invoke($null, @($character)) -eq 1
                }
                $logicalLineRepair =
                    $env:BT_PSREADLINE_REANCHOR_WHOLE_SCREEN_PROBE -ne '1'
                $previousLinear = $logicalLineRepair -and $previous.lines.Length -eq 1
                if ($previousLinear) {
                    $rendered = $previous.lines[0].Line
                    for ($index = 0; $previousLinear -and $index -lt $rendered.Length; $index++) {
                        $character = $rendered[$index]
                        if ($character -eq [char]27 -and $index + 1 -lt $rendered.Length -and
                            $rendered[$index + 1] -eq '[') {
                            $index += 2
                            while ($index -lt $rendered.Length -and $rendered[$index] -ne 'm') {
                                $index++
                            }
                            continue
                        }
                        $previousLinear = [int]$cellMethod.Invoke($null, @($character)) -eq 1
                    }
                }
                $paintedCursorCells = $null
                if ($previousLinear) {
                    $paintedCursorCells = ($savedPreviousCursorTop - $oldY) * $oldWidth +
                        $savedPreviousCursorLeft - $oldX
                    if ($paintedCursorCells -lt 0) {
                        throw 'The previous PSReadLine cursor precedes its input anchor.'
                    }
                }
                # At an exact old-width boundary PSReadLine represents D as the next row, column 0.
                # ConPTY can retain that wrap-pending cursor cell while widening the already-painted
                # input to a different B. In line-anchor-verify.vt, 54 -> 56 left physical D at
                # (12,1); `D - 64` guessed B=(10,49), four cells after the reflowed B=(10,45), and
                # the next full repaint produced `echoecho`. The previous render still states both
                # the real B column and its B..D row span, so carry those across this one ambiguous
                # sentinel instead of treating physical D as a content-tail coordinate.
                $exactRightEdgeWiden =
                    $env:BT_PSREADLINE_REANCHOR_EXACT_EDGE_PROBE -ne '1' -and
                    $previousLinear -and $width -gt $oldWidth -and
                    $savedPreviousCursorLeft -eq 0 -and $physicalX -eq 0 -and
                    $paintedCursorCells -gt 0 -and
                    (($oldX + $paintedCursorCells) % $oldWidth) -eq 0
                if ($exactRightEdgeWiden) {
                    $paintedCursorRows = $savedPreviousCursorTop - $oldY
                    $anchorY = $physicalY - $paintedCursorRows
                    if ($anchorY -lt 0 -or $oldX -ge $width) {
                        throw 'The exact-boundary PSReadLine anchor is invalid.'
                    }
                    $anchor = $anchorY * $width + $oldX
                    $anchorSolved = $true
                    $repairMode = 'exact-right-edge-widen'
                } elseif ($previousLinear -and $waitingToRender) {
                    $anchor = $physicalY * $width + $physicalX - $paintedCursorCells
                    $anchorSolved = $true
                    $repairMode = 'waiting-previous-render'
                } elseif ($logicalLineRepair -and -not $waitingToRender -and $linearCurrent) {
                    $anchorX = (($physicalX - $cursor) % $width + $width) % $width
                    $cursorRows = [int][Math]::Floor(($anchorX + $cursor) / $width)
                    $anchor = ($physicalY - $cursorRows) * $width + $anchorX
                    $anchorSolved = $true
                    $repairMode = 'current-linear'
                }

                if (-not $anchorSolved) {
                    $repairMode = 'complex'
                    # Complex/multiline and right-margin input retains the established
                    # PSReadLine-width calculation, including its continuation-prompt and
                    # wide-character padding rules.
                    $anchorX = (($oldY * $oldWidth + $oldX) % $width + $width) % $width
                $continuationCells = 0
                foreach ($character in [Microsoft.PowerShell.PSConsoleReadLine]::GetOptions().ContinuationPrompt.ToCharArray()) {
                    $continuationCells += [int]$cellMethod.Invoke($null, @($character))
                }
                if ($continuationCells -lt 0 -or $continuationCells -ge $width) {
                    throw 'Invalid PSReadLine continuation prompt width.'
                }

                # This is PSReadLine 2.4.x ConvertOffsetToPoint's cell movement, separated from its
                # stale cached origin. Newline begins a logical row; all other character widths
                # come from PSReadLine itself rather than a duplicated Unicode width table.
                $x = $anchorX
                $rows = 0
                for ($index = 0; $index -lt $cursor; $index++) {
                    $character = $line[$index]
                    if ($character -eq "`n") {
                        $rows++
                        $x = $continuationCells
                        continue
                    }
                    $cells = [int]$cellMethod.Invoke($null, @($character))
                    if ($cells -le 0 -or $cells -gt $width) {
                        throw 'Invalid PSReadLine character cell width.'
                    }
                    $x += $cells
                    if ($x -ge $width) {
                        $x = if ($x -eq $width) { 0 } else { $cells }
                        if ($x -ne 0 -or $index + 1 -ge $cursor -or $line[$index + 1] -ne "`n") {
                            $rows++
                        }
                    }
                }
                # A wide character that cannot fit in the remaining cell is rendered wholly on
                # the following row, and the insertion cursor before it moves there too.
                if ($cursor -lt $line.Length -and $line[$cursor] -ne "`n") {
                    $nextCells = [int]$cellMethod.Invoke($null, @($line[$cursor]))
                    if ($nextCells -le 0 -or $nextCells -gt $width) {
                        throw 'Invalid PSReadLine character cell width.'
                    }
                    if ($x + $nextCells -gt $width) {
                        $x = 0
                        $rows++
                    }
                }
                # ConPTY reflow can retain a padding cell created when a wide character did not fit
                # at the old right edge. That cell is real screen state but is not present in the
                # text buffer, so the physical cursor column — not a second rendering prediction —
                # is authoritative for the final partial row of D.
                $displayCells = $rows * $width + $physicalX - $anchorX
                $anchor = $physicalY * $width + $physicalX - $displayCells
                if ($anchor -lt 0 -or $anchor % $width -ne $anchorX) {
                    throw 'The derived PSReadLine anchor is invalid.'
                }
                }
            }

            if ($anchor -lt 0 -or $anchor -ge $width * $height) {
                throw 'The derived PSReadLine anchor is outside the console buffer.'
            }
            $anchorX = $anchor % $width
            $anchorY = [int][Math]::Floor($anchor / $width)
            $initialXField.SetValue($singleton, $anchorX)
            $initialYField.SetValue($singleton, $anchorY)

            # Empty input really does have an empty render. For non-empty input, retain PSReadLine's
            # existing lines: they are the exact glyphs ConPTY just reflowed and are therefore the
            # right diff baseline, including wide-character edge padding. Only their console
            # geometry is stale. Updating it prevents RecomputeInitialCoords from interpreting the
            # old width on the next history/edit render, without emitting a byte here. The probe
            # switch preserves the retired empty-baseline behavior as the real-ConPTY red arm.
            $baseline = $type.GetField('_initialPrevRender', $static).GetValue($null)
            if ($line.Length -eq 0 -or
                $env:BT_PSREADLINE_REANCHOR_EMPTY_BASELINE_PROBE -eq '1') {
                $previousField.SetValue($singleton, $baseline)
                $baseline.UpdateConsoleInfo($width, $height, $physicalX, $physicalY)
                $baseline.initialY = $anchorY
            } else {
                if ([object]::ReferenceEquals($previous, $baseline) -or
                    $null -eq $previous.lines -or $previous.lines.Length -eq 0) {
                    throw 'The non-empty PSReadLine render data is invalid.'
                }
                $previous.UpdateConsoleInfo($width, $height, $physicalX, $physicalY)
                $previous.initialY = $anchorY
            }
            if ($env:BT_PSREADLINE_REANCHOR_PROBE -eq '1') {
                [Console]::Write(
                    ([string][char]27) + ']777;BT_PSREADLINE_REANCHOR=' + $repairMode +
                    ',waiting=' + $waitingToRender + ',width=' + $width +
                    ',anchor=' + $anchorY + ':' + $anchorX + [char]7)
            }
        } catch {
            $reflectionError = $_
            # InvokePrompt uses the old Y coordinate to erase the old prompt. If reflection failed
            # after a field write, put the complete old cache back before taking that fallback.
            try {
                if ($null -ne $singleton -and $null -ne $savedPrevious) {
                    $initialXField.SetValue($singleton, $savedInitialX)
                    $initialYField.SetValue($singleton, $savedInitialY)
                    $savedPrevious.UpdateConsoleInfo(
                        $savedPreviousBufferWidth,
                        $savedPreviousBufferHeight,
                        $savedPreviousCursorLeft,
                        $savedPreviousCursorTop)
                    $savedPrevious.initialY = $savedPreviousInitialY
                    $previousField.SetValue($singleton, $savedPrevious)
                }
            } catch {
                # The private shape is already untrusted; InvokePrompt remains the only safe exit.
            }
            if ($env:BT_PSREADLINE_REANCHOR_PROBE -eq '1') {
                [Console]::Write(
                    ([string][char]27) + ']777;BT_PSREADLINE_REANCHOR_FALLBACK=' +
                    $reflectionError.Exception.Message + [char]7)
            }
            [Microsoft.PowerShell.PSConsoleReadLine]::InvokePrompt($key, $arg)
        }
        } finally {
            & $Global:__FolioShellIntegration.HistoryNavigationRestore $historyNavigation
        }
    }
} else {
    Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F12' -ScriptBlock {
        param($key, $arg)
        $historyNavigation = & $Global:__FolioShellIntegration.HistoryNavigationCapture
        try {
        # Dev-gated proof that this handler, rather than an unbound-key fallback, consumed the VT
        # input. OSC 777 is ignored by the terminal and the variable is absent in product sessions.
        if ($env:BT_PSREADLINE_NOOP_PROBE -eq '1') {
            [Console]::Write(([string][char]27) + ']777;BT_PSREADLINE_NOOP' + [char]7)
        }
        } finally {
            & $Global:__FolioShellIntegration.HistoryNavigationRestore $historyNavigation
        }
    }
}

function Global:PSConsoleHostReadLine {
    $state = $Global:__FolioShellIntegration
    $commandLine = & $state.OriginalReadLine

    # This is the last moment before the host runs the line, and the only one at which
    # `$LASTEXITCODE` still holds the *previous* command's code. Reading it here is what lets the
    # prompt tell "this command exited 3" from "some command three lines ago exited 3": the
    # variable is a session-wide leftover, not a per-command fact, and PowerShell offers nothing
    # else that says which command last wrote it. Recorded after the read-line returns, so a
    # customizer that shells out to a native helper while the user is typing (oh-my-posh's
    # transient prompt does) is part of the baseline rather than mistaken for the command.
    $state.ExitCodeAtLineStart = $Global:LASTEXITCODE

    # `C` opens an output region that only `D` closes, so it is owed only by a line that is
    # actually going to run something. An empty line, a line of whitespace, a line that is nothing
    # but a comment, and the empty string PSReadLine returns when Ctrl+C abandons what was typed
    # all run no command at all: PowerShell's own parser is asked, and an input that parses cleanly
    # into no statements gets no markers rather than a zero-length command in the ledger carrying
    # the status of whatever ran before it. An input that does *not* parse is a command as far as
    # this contract goes — the host answers it with an error, and that error is a status.
    $runsSomething = $false
    $state.LineParsed = $true
    if (-not [string]::IsNullOrWhiteSpace($commandLine)) {
        $parseErrors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseInput(
            $commandLine, [ref]$null, [ref]$parseErrors)
        if ($parseErrors.Count -gt 0) {
            # A line the parser rejects still owes a region: the host answers it with an error
            # under the prompt, and that error is the command's whole output. Nothing runs, so
            # neither `$?` nor `$LASTEXITCODE` will move — the prompt is told here, because this is
            # the only place that knows.
            $runsSomething = $true
            $state.LineParsed = $false
        } else {
            # The three blocks a top-level input can be spelled with on both supported generations.
            # `clean` and `dynamicparam` exist only inside a function body, which is a statement in
            # one of these, and naming a property 5.1's AST does not have would throw under a
            # profile that runs `Set-StrictMode`.
            foreach ($block in $ast.BeginBlock, $ast.ProcessBlock, $ast.EndBlock) {
                if ($null -ne $block -and $block.Statements.Count -gt 0) {
                    $runsSomething = $true
                }
            }
        }
    }

    if ($runsSomething) {
        $state.CommandStarted = $true
        # [char]27: Windows PowerShell 5.1 has no `e escape; this form works on both generations.
        [Console]::Write(([string][char]27) + ']133;C' + [char]7)
    }
    return $commandLine
}

function Global:prompt {
    # These two statements are first, and nothing may move ahead of them: `$?` is overwritten by
    # the very next thing this session does, and `$LASTEXITCODE` by the next native command.
    $lastSucceeded = $?
    $nativeExitCode = $Global:LASTEXITCODE
    $state = $Global:__FolioShellIntegration
    $esc = [string][char]27
    $bel = [string][char]7
    $out = ''

    # Where in the chain this invocation is, and which prompt it owes a call to.
    $depth = $state.PromptDepth
    $inner = $depth

    # Folio's prompt has to be the *outermost* one, because everything a prompt customizer runs —
    # conda's `Write-Host` of `(base) `, oh-my-posh's own executable — overwrites the two variables
    # above before we could read them. Installation makes it outermost; a customizer initialised
    # afterwards (`conda init powershell` writes into the all-hosts profile, which loads *before*
    # the per-host one that usually carries the `.` of this script) renames this function out of
    # the way and puts its own in front, and from then on every status Folio reports is that
    # customizer's own success. So the moment this function notices it is no longer what `prompt`
    # resolves to, it takes the name back and keeps the customizer in the chain — one prompt later
    # the order is right again and stays right, without either prompt losing its output.
    if ($depth -eq 0) {
        $installed = Get-Command prompt -CommandType Function -ErrorAction SilentlyContinue
        if ($null -ne $installed -and
            -not [object]::ReferenceEquals($installed.ScriptBlock, $state.SelfPrompt)) {
            $alreadyChained = $false
            foreach ($link in $state.PromptChain) {
                if ([object]::ReferenceEquals($link, $installed.ScriptBlock)) {
                    $alreadyChained = $true
                }
            }
            if (-not $alreadyChained) {
                $state.PromptChain = @($installed.ScriptBlock) + $state.PromptChain
            }
            Set-Item -LiteralPath Function:Global:prompt -Value $state.SelfPrompt
            # This invocation is already running inside the wrapper just adopted — it called us —
            # so the prompt still owed is the one after it. Its text is written once, and the
            # status reported for this one prompt is the laundered one; the next is honest.
            $inner = $depth + 1
        }
    }

    if ($state.CommandStarted -and $depth -eq 0) {
        # Three facts, in the order of how much they can be trusted.
        #
        # `$LASTEXITCODE` having *moved* since the line was submitted is the only one no prompt
        # code can fake: a native command wrote it, and if what it wrote is non-zero then this
        # line's own program failed, whatever `$?` was talked into saying afterwards.
        #
        # `$?` is next: it is the shell's own verdict, false for a failed cmdlet that never
        # touches `$LASTEXITCODE`, and true for a command that succeeded after an older native
        # failure left a non-zero code lying around.
        #
        # `$LASTEXITCODE` on its own is last, and only as a number: when `$?` says the command
        # failed and the variable is non-zero, it is the code the shell itself would show, so it
        # is the code reported — even though a cmdlet failure standing behind an older native
        # failure cannot be told apart from that native command being run again, which is the one
        # thing this derivation still cannot resolve. It costs the digits, never the verdict:
        # nothing that failed is reported as 0, and nothing that succeeded is reported as failed.
        if (-not $state.LineParsed) {
            $exitCode = 1
        } elseif ($nativeExitCode -is [int] -and $nativeExitCode -ne 0 -and
            -not ($nativeExitCode -eq $state.ExitCodeAtLineStart)) {
            $exitCode = $nativeExitCode
        } elseif ($lastSucceeded) {
            $exitCode = 0
        } elseif ($nativeExitCode -is [int] -and $nativeExitCode -ne 0) {
            $exitCode = $nativeExitCode
        } else {
            $exitCode = 1
        }
        $out += $esc + ']133;D;' + $exitCode + $bel
        $state.CommandStarted = $false
    }

    # OSC 7: the authoritative working directory, reported once per prompt. It is what lets
    # Folio resolve './x.png' and '../a/b.svg' in this session's output; a terminal that
    # is never told a directory deliberately leaves relative paths undetected rather than guessing
    # one. A location on a non-filesystem provider (HKLM:, Cert:, …) has no directory to resolve
    # against, so the report is sent empty, which retracts the previous one instead of leaving it
    # to answer for a place the shell has left.
    if ($depth -eq 0) {
        $location = $ExecutionContext.SessionState.Path.CurrentLocation
        if ($location.Provider.Name -eq 'FileSystem') {
            $out += $esc + ']7;' + (& $state.WorkingDirectoryUri $location.ProviderPath) + $bel
        } else {
            $out += $esc + ']7;' + $bel
        }
        $out += $esc + ']133;A' + $bel
    }

    # The rest of the chain, still returning its text to the host through us. Depth is restored
    # even if a customizer's prompt throws, because a prompt that failed once must not leave the
    # session reporting every later prompt as a nested one.
    $state.PromptDepth = $inner + 1
    try {
        $next = $state.PromptChain[$inner]
        if ($null -ne $next) {
            $out += (& $next)
        }
    } finally {
        $state.PromptDepth = $depth
    }

    if ($depth -eq 0) {
        $out += $esc + ']133;B' + $bel
    }
    return $out
}

# The identity `prompt` is checked against on every draw. Taken after the definition above, it is
# the object itself, so a rename — which is how a prompt customizer nests one prompt inside
# another — moves the function without changing what this holds.
$Global:__FolioShellIntegration.SelfPrompt =
    (Get-Command prompt -CommandType Function).ScriptBlock

# pwsh reports its executable path as the initial console title on some hosts. Establish the
# profile's stable default only after integration is installed; later OSC/title writes from child
# programs remain authoritative. `[char]27` keeps this identical on PowerShell 5.1 and 7.
#
# It names the *edition*, and that matters now that the two are two profiles rather than two ends
# of one resolution order. Folio drops a title that only repeats the profile's own name —
# a shell agreeing with its launcher has announced nothing — and it can only do that if the two
# strings match: a 5.1 session titled "PowerShell" under a profile called "Windows PowerShell 5.1"
# would prefix every pane head in that tab with its own family name. `Desktop` is 5.1 and `Core`
# is 7; `$PSVersionTable.PSEdition` is absent on 5.0 and earlier, where `Desktop` is still right.
#
# **These two strings are `crates/bt-app/src/profiles.rs`'s `title` fields, character for
# character, and the version in each is deliberate**: the two rows used to read "PowerShell" and
# "Windows PowerShell", which left nobody able to tell which was 7 and which was 5.1. The equality
# above is exact, so the two files change together or the suppression stops firing —
# `the_integration_script_names_the_profiles_own_titles` is the pin that says so.
$btEdition = if ($PSVersionTable.PSEdition -eq 'Core') { 'PowerShell 7' } else { 'Windows PowerShell 5.1' }
[Console]::Write(([string][char]27) + ']0;' + $btEdition + [char]7)
