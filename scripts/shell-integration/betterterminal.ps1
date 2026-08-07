# BetterTerminal OSC 133 + OSC 7 shell integration for PowerShell 7 and Windows PowerShell 5.1.
# Opt in from $PROFILE after prompt customizers (for example oh-my-posh):
#   . 'D:\path\to\BetterTerminal\scripts\shell-integration\betterterminal.ps1'

if ($Global:__BetterTerminalShellIntegration -and
    $Global:__BetterTerminalShellIntegration.Installed) {
    return
}

# Hyperlink capability declaration, honest and scoped: BetterTerminal renders OSC 8, but
# identity-allowlisted CLIs (Claude Code 2.1.220 keys on WT_SESSION / TERM_PROGRAM=iTerm.app)
# downgrade links for unknown terminals. FORCE_HYPERLINK is their documented capability override,
# so declare it — only inside BetterTerminal sessions, and never clobber a user's explicit choice.
# Known trade-off: supports-hyperlinks-family CLIs honor this even with redirected output, so a
# command piping to a file may carry OSC 8 bytes; revisit as a setting when the settings slice lands.
if ($env:TERM_PROGRAM -eq 'BetterTerminal' -and -not (Test-Path env:FORCE_HYPERLINK)) {
    $env:FORCE_HYPERLINK = '1'
}

# PSConsoleHostReadLine is the supported console-host extension point. Importing PSReadLine here
# makes its original entry point available on both supported PowerShell generations.
Import-Module PSReadLine -ErrorAction SilentlyContinue

$originalPrompt = (Get-Command prompt -CommandType Function -ErrorAction Stop).ScriptBlock
$readLineCommand = Get-Command PSConsoleHostReadLine -ErrorAction SilentlyContinue
if (-not $readLineCommand) {
    Write-Warning 'BetterTerminal shell integration requires PSReadLine; no changes were installed.'
    return
}
$originalReadLine = if ($readLineCommand -is [System.Management.Automation.FunctionInfo]) {
    $readLineCommand.ScriptBlock
} else {
    $readLineCommand
}

$Global:__BetterTerminalShellIntegration = @{
    Installed = $true
    OriginalPrompt = $originalPrompt
    OriginalReadLine = $originalReadLine
    CommandStarted = $false
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
        # Empty authority: the file-URI spelling of "this host". BetterTerminal also accepts
        # 'localhost' and this machine's name, but the empty form needs no lookup per prompt and
        # cannot go stale.
        return 'file:///' + $builder.ToString().TrimStart('/')
    }
}

# Private resize-anchor chord. ConPTY translates CSI 24;8~ into Ctrl+Alt+Shift+F12 on both Windows
# PowerShell 5.1/PSReadLine 2.0.0 and pwsh/PSReadLine 2.4.5. Only 2.4.x has the private state shape
# proven below. The real-ConPTY probe established that 2.0.0 implements InvokePrompt with ED 2,
# which clears the visible viewport, so unsupported/unproven versions consume the same chord as a
# no-op rather than leaking it into the input buffer.
$psReadLineVersion = (Get-Module PSReadLine).Version
if ($psReadLineVersion.Major -eq 2 -and $psReadLineVersion.Minor -eq 4) {
    Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F12' -ScriptBlock {
        param($key, $arg)
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
                $previousLinear = $logicalLineRepair -and $waitingToRender -and
                    $previous.lines.Length -eq 1
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
                if ($previousLinear) {
                    $paintedCursorCells =
                        ($savedPreviousCursorTop - $oldY) * $oldWidth +
                        $savedPreviousCursorLeft - $oldX
                    if ($paintedCursorCells -lt 0) {
                        throw 'The previous PSReadLine cursor precedes its input anchor.'
                    }
                    $anchor = $physicalY * $width + $physicalX - $paintedCursorCells
                    $anchorSolved = $true
                } elseif ($logicalLineRepair -and -not $waitingToRender -and $linearCurrent) {
                    $anchorX = (($physicalX - $cursor) % $width + $width) % $width
                    $cursorRows = [int][Math]::Floor(($anchorX + $cursor) / $width)
                    $anchor = ($physicalY - $cursorRows) * $width + $anchorX
                    $anchorSolved = $true
                }

                if (-not $anchorSolved) {
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
    }
} else {
    Set-PSReadLineKeyHandler -Chord 'Ctrl+Alt+Shift+F12' -ScriptBlock {
        param($key, $arg)
        # Dev-gated proof that this handler, rather than an unbound-key fallback, consumed the VT
        # input. OSC 777 is ignored by the terminal and the variable is absent in product sessions.
        if ($env:BT_PSREADLINE_NOOP_PROBE -eq '1') {
            [Console]::Write(([string][char]27) + ']777;BT_PSREADLINE_NOOP' + [char]7)
        }
    }
}

function Global:PSConsoleHostReadLine {
    $original = $Global:__BetterTerminalShellIntegration.OriginalReadLine
    $commandLine = & $original
    # [char]27: Windows PowerShell 5.1 has no `e escape; this form works on both generations.
    [Console]::Write(([string][char]27) + ']133;C' + [char]7)
    return $commandLine
}

function Global:prompt {
    # Capture status before prompt code or history inspection can overwrite it.
    $lastSucceeded = $?
    $nativeExitCode = $Global:LASTEXITCODE
    $state = $Global:__BetterTerminalShellIntegration
    $esc = [string][char]27
    $bel = [string][char]7
    $out = ''

    if ($state.CommandStarted) {
        if ($lastSucceeded) {
            $exitCode = 0
        } elseif ($null -ne $nativeExitCode) {
            $exitCode = $nativeExitCode
        } else {
            $exitCode = 1
        }
        $out += $esc + ']133;D;' + $exitCode + $bel
    }

    # OSC 7: the authoritative working directory, reported once per prompt. It is what lets
    # BetterTerminal resolve './x.png' and '../a/b.svg' in this session's output; a terminal that
    # is never told a directory deliberately leaves relative paths undetected rather than guessing
    # one. A location on a non-filesystem provider (HKLM:, Cert:, …) has no directory to resolve
    # against, so the report is sent empty, which retracts the previous one instead of leaving it
    # to answer for a place the shell has left.
    $location = $ExecutionContext.SessionState.Path.CurrentLocation
    if ($location.Provider.Name -eq 'FileSystem') {
        $out += $esc + ']7;' + (& $state.WorkingDirectoryUri $location.ProviderPath) + $bel
    } else {
        $out += $esc + ']7;' + $bel
    }

    $out += $esc + ']133;A' + $bel
    $out += (& $state.OriginalPrompt)
    $out += $esc + ']133;B' + $bel
    $state.CommandStarted = $true
    return $out
}
