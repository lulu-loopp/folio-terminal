use std::{
    error::Error,
    num::{NonZeroU16, NonZeroU32},
    sync::Arc,
    time::{Duration, Instant},
};

use bt_pty::{OutputWake, PtyCommand, PtySession, PtySize};
use bt_term::DualPlaneSession;

const COLUMNS: u16 = 80;
const ROWS: u16 = 24;
const FAMILY: &str = "👨\u{200d}👩\u{200d}👧\u{200d}👦";

fn main() -> Result<(), Box<dyn Error>> {
    let width_script = r#"
$utf8 = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = $utf8
$esc = [char]27
$zwj = [char]0x200D
$vs15 = [char]0xFE0E
$vs16 = [char]0xFE0F
$combiningAcute = [char]0x0301
function CodePoint([int]$value) { [char]::ConvertFromUtf32($value) }
function Probe([string]$id, [string]$text) {
    [Console]::Write($esc + '[2J' + $esc + '[H')
    $before = [Console]::CursorLeft
    [Console]::Write($text)
    $after = [Console]::CursorLeft
    [Console]::Write("`r`nBT_WIDTH id=$id before=$before after=$after cells=$($after - $before)`r`n")
}
function Suite([string]$mode) {
    Probe "$mode-family" ((CodePoint 0x1F468) + $zwj + (CodePoint 0x1F469) + $zwj + (CodePoint 0x1F467) + $zwj + (CodePoint 0x1F466))
    Probe "$mode-skin-tone" ((CodePoint 0x1F44D) + (CodePoint 0x1F3FD))
    Probe "$mode-combining" ('e' + $combiningAcute)
    Probe "$mode-umbrella-vs15" ([char]0x2602 + $vs15)
    Probe "$mode-umbrella-vs16" ([char]0x2602 + $vs16)
    Probe "$mode-flag" ((CodePoint 0x1F1FA) + (CodePoint 0x1F1F8))
    Probe "$mode-ambiguous-star" ([char]0x2606)
}
Suite 'legacy'
[Console]::Write($esc + '[?2027h')
Suite 'mode2027'
[Console]::Write($esc + '[?2027l')
"#;
    let width_bytes = capture_powershell(width_script)?;

    println!("width_raw_len={}", width_bytes.len());
    println!("width_raw_hex={}", hex(&width_bytes));
    println!(
        "width_raw_debug={:?}",
        String::from_utf8_lossy(&width_bytes)
    );
    for observation in width_observations(&width_bytes) {
        println!("conpty_{observation}");
    }

    // Keep this equivalent to the first-round width-acceptance script. It intentionally uses the
    // old 400-character pressure write: this probe exists to settle the reported `||` result from
    // that exact sequence, while the human-facing script can move to a compact CSI 999G probe.
    let acceptance_script = r#"
$utf8 = New-Object System.Text.UTF8Encoding($false)
[Console]::OutputEncoding = $utf8
$e = [char]27
function cp { param([int[]]$points) ($points | ForEach-Object { [char]::ConvertFromUtf32($_) }) -join '' }
$family = cp 0x1F468,0x200D,0x1F469,0x200D,0x1F467,0x200D,0x1F466
$thumbs = cp 0x1F44D,0x1F3FD
$flag = cp 0x1F1FA,0x1F1F8
$eacute = 'e' + [char]0x0301
$mixed = 'A' + [char]0x2606 + [char]0x4E2D + [char]0x2502 + [char]0xFF22
$umbrVS = [char]0x2602 + [char]0xFE0F
function Show { param($label, $s, [int]$cells)
  Write-Host ('|' + $s + '|')
  Write-Host ('|' + ('#' * $cells) + '| <- ' + $label + ' ' + $cells)
  Write-Host ''
}
Write-Host '===== legacy ====='
Show 'family-legacy' $family 8
Write-Host '===== 2027 ====='
Write-Host "$e[?2027h" -NoNewline
Show 'family' $family 2
Show 'skin' $thumbs 2
Show 'flag' $flag 2
Show 'combining' $eacute 1
Show 'mixed' $mixed 7
Show 'vs16' $umbrVS 2
Write-Host '===== pressure ====='
Write-Host "$e[?7l" -NoNewline
Write-Host ('x' * 400) -NoNewline
Write-Host ([char]0x2602) -NoNewline
Write-Host ([char]0xFE0F)
Write-Host "$e[?7h" -NoNewline
Write-Host ''
Write-Host 'BT_PANIC_SURVIVED'
Write-Host ''
Write-Host '===== reset ====='
Write-Host "$e[?2027l" -NoNewline
Show 'BT_RESET_FAMILY' $family 8
Write-Host 'BT_ACCEPTANCE_DONE'
"#;
    let acceptance_bytes = capture_powershell(acceptance_script)?;
    println!("acceptance_raw_len={}", acceptance_bytes.len());
    println!("acceptance_raw_hex={}", hex(&acceptance_bytes));
    println!(
        "acceptance_raw_debug={:?}",
        String::from_utf8_lossy(&acceptance_bytes)
    );

    let mixed_marker = b"|#######| <- mixed 7";
    let mixed_end = find_subslice(&acceptance_bytes, mixed_marker)
        .map(|start| start + mixed_marker.len())
        .ok_or("ConPTY output omitted the mixed-width marker")?;
    let mut mixed_replay = DualPlaneSession::new(nz32(COLUMNS), nz32(ROWS));
    mixed_replay.feed(&acceptance_bytes[..mixed_end])?;
    let mixed_actual = find_visible_row(&mixed_replay, "|A☆中│Ｂ|")
        .ok_or("DualPlaneSession replay lost the mixed test row")?;
    let mixed_ruler = find_visible_row(&mixed_replay, "|#######|")
        .ok_or("DualPlaneSession replay lost the mixed ruler row")?;
    let mixed_close =
        closing_ascii_bar_column(&mixed_actual).ok_or("mixed test row has no closing ASCII bar")?;
    let ruler_close =
        closing_ascii_bar_column(&mixed_ruler).ok_or("mixed ruler row has no closing ASCII bar")?;
    println!("replay_mixed_close_column={mixed_close}");
    println!("replay_mixed_ruler_close_column={ruler_close}");
    if (mixed_close, ruler_close) != (8, 8) {
        return Err("DualPlaneSession placed the mixed row or ruler in the wrong column".into());
    }
    println!("verdict_mixed_grid=correct");

    let reset = find_subslice(&acceptance_bytes, b"\x1b[?2027l")
        .ok_or("ConPTY did not forward DECRST 2027 in acceptance sequence")?;
    let family = FAMILY.as_bytes();
    let family_after_reset = acceptance_bytes[reset..]
        .windows(family.len())
        .any(|window| window == family);
    println!("acceptance_raw_family_after_reset={family_after_reset}");
    if family_after_reset {
        return Err("the old acceptance script unexpectedly emitted a post-DECRST family".into());
    }

    let mut replay = DualPlaneSession::new(nz32(COLUMNS), nz32(ROWS));
    replay.feed(&acceptance_bytes)?;
    let visible_rows = (0..u32::from(ROWS))
        .filter_map(|row| replay.terminal().visible_row(row))
        .map(|row| {
            row.cells
                .iter()
                .filter(|cell| !cell.wide_spacer)
                .map(|cell| cell.text.as_str())
                .collect::<String>()
                .trim_end()
                .to_owned()
        })
        .collect::<Vec<_>>();
    println!("replay_visible_rows={visible_rows:?}");
    let reset_family_visible = visible_rows.iter().any(|row| row.contains(FAMILY));
    let empty_family_visible = visible_rows.iter().any(|row| row == "||");
    println!("replay_reset_family_visible={reset_family_visible}");
    println!("replay_empty_family_visible={empty_family_visible}");
    if reset_family_visible || !empty_family_visible {
        return Err(
            "DualPlaneSession replay did not preserve ConPTY's literal empty family row".into(),
        );
    }
    println!("verdict_empty_family=PowerShell_cp_alias_produced_literal_double_bar");
    if !visible_rows
        .iter()
        .any(|row| row.contains("BT_ACCEPTANCE_DONE"))
    {
        return Err("DualPlaneSession replay did not reach the acceptance sentinel".into());
    }
    Ok(())
}

fn capture_powershell(script: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let command = PtyCommand::new("powershell.exe")
        .arg("-NoLogo")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script);
    let size = PtySize::cells(
        NonZeroU16::new(COLUMNS).unwrap(),
        NonZeroU16::new(ROWS).unwrap(),
    );
    let wake: OutputWake = Arc::new(|| {});
    let mut session = PtySession::spawn(command, size, wake)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut bytes = Vec::new();
    let mut child_exited = false;
    let mut answered_cursor_query = false;
    while Instant::now() < deadline {
        bytes.extend(session.read_output());
        if !answered_cursor_query && find_subslice(&bytes, b"\x1b[6n").is_some() {
            session.write(b"\x1b[1;1R")?;
            answered_cursor_query = true;
        }
        child_exited |= session.try_wait()?.is_some();
        if child_exited && session.output_is_drained() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    bytes.extend(session.read_output());
    session.shutdown()?;

    if !child_exited {
        return Err("ConPTY probe child did not exit before the deadline".into());
    }
    Ok(bytes)
}

fn nz32(value: u16) -> NonZeroU32 {
    NonZeroU32::new(u32::from(value)).unwrap()
}

fn find_visible_row(
    session: &DualPlaneSession,
    needle: &str,
) -> Option<bt_transcript::CapturedRow> {
    (0..u32::from(ROWS)).find_map(|row| {
        let row = session.terminal().visible_row(row)?;
        row_text(&row).starts_with(needle).then_some(row)
    })
}

fn row_text(row: &bt_transcript::CapturedRow) -> String {
    row.cells
        .iter()
        .filter(|cell| !cell.wide_spacer)
        .map(|cell| cell.text.as_str())
        .collect::<String>()
        .trim_end()
        .to_owned()
}

fn closing_ascii_bar_column(row: &bt_transcript::CapturedRow) -> Option<usize> {
    row.cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.text == "|")
        .map(|(column, _)| column)
        .nth(1)
}

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn width_observations(bytes: &[u8]) -> Vec<&str> {
    let text = std::str::from_utf8(bytes).unwrap_or_default();
    text.match_indices("BT_WIDTH ")
        .filter_map(|(start, _)| {
            let tail = &text[start..];
            let end = tail.find(['\r', '\n', '\u{1b}']).unwrap_or(tail.len());
            (end > 0).then_some(&tail[..end])
        })
        .collect()
}
