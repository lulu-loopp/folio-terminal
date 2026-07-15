use std::{
    collections::VecDeque,
    env, fs,
    io::{Read, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bt_corpus::{Corpus, CorpusEvent, EventKind};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

#[derive(Clone, Copy, Debug)]
struct ScheduledResize {
    at_ms: u64,
    cols: u16,
    rows: u16,
}

#[derive(Clone, Debug)]
struct ScheduledInput {
    at_ms: u64,
    bytes: Vec<u8>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let output = args.next().context("usage: bt-record OUTPUT.btcr [--size COLSxROWS] [--resize MS:COLSxROWS] [--stdin FILE] [--input-plan FILE] -- COMMAND [ARG...]")?;
    let mut cols = 80;
    let mut rows = 24;
    let mut resizes = Vec::new();
    let mut stdin_file = None;
    let mut input_plan = Vec::new();
    let mut command = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--size" => (cols, rows) = parse_size(&args.next().context("--size needs COLSxROWS")?)?,
            "--resize" => resizes.push(parse_resize(
                &args.next().context("--resize needs MS:COLSxROWS")?,
            )?),
            "--stdin" => stdin_file = Some(args.next().context("--stdin needs FILE")?),
            "--input-plan" => {
                input_plan = parse_input_plan(&args.next().context("--input-plan needs FILE")?)?
            }
            "--" => {
                command.extend(args);
                break;
            }
            _ => bail!("unexpected argument {arg}; put the command after --"),
        }
    }
    if command.is_empty() {
        bail!("missing command after --");
    }
    resizes.sort_by_key(|resize| resize.at_ms);
    input_plan.sort_by_key(|input| input.at_ms);

    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    })?;
    let mut builder = CommandBuilder::new(&command[0]);
    builder.cwd(env::current_dir()?);
    for arg in &command[1..] {
        builder.arg(arg);
    }
    let mut child = pair.slave.spawn_command(builder)?;
    drop(pair.slave);

    // Keep the input side so the recorder can answer terminal status reports emitted by ConPTY.
    let mut writer = pair.master.take_writer()?;
    let mut pending_input = stdin_file.map(fs::read).transpose()?;
    let mut output_tail = VecDeque::with_capacity(4);

    let mut reader = pair.master.try_clone_reader()?;
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    thread::spawn(move || {
        let mut buffer = [0; 16 * 1024];
        while let Ok(count) = reader.read(&mut buffer) {
            if count == 0 {
                break;
            }
            if tx.send(buffer[..count].to_vec()).is_err() {
                break;
            }
        }
    });

    let started = Instant::now();
    let mut events = Vec::new();
    let mut next_resize = 0;
    let mut next_input = 0;
    let exit_code;
    loop {
        while next_resize < resizes.len()
            && started.elapsed().as_millis() as u64 >= resizes[next_resize].at_ms
        {
            let resize = resizes[next_resize];
            pair.master.resize(PtySize {
                rows: resize.rows,
                cols: resize.cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
            events.push(CorpusEvent {
                at_micros: started.elapsed().as_micros() as u64,
                kind: EventKind::Resize {
                    cols: resize.cols,
                    rows: resize.rows,
                },
            });
            next_resize += 1;
        }
        while next_input < input_plan.len()
            && started.elapsed().as_millis() as u64 >= input_plan[next_input].at_ms
        {
            writer.write_all(&input_plan[next_input].bytes)?;
            writer.flush()?;
            next_input += 1;
        }
        match rx.recv_timeout(Duration::from_millis(2)) {
            Ok(bytes) => {
                std::io::stdout().write_all(&bytes)?;
                for byte in &bytes {
                    output_tail.push_back(*byte);
                    while output_tail.len() > 4 {
                        output_tail.pop_front();
                    }
                    if output_tail.iter().copied().eq(b"\x1b[6n".iter().copied()) {
                        writer.write_all(b"\x1b[1;1R")?;
                        if let Some(input) = pending_input.take() {
                            writer.write_all(&input)?;
                        }
                    }
                }
                events.push(CorpusEvent {
                    at_micros: started.elapsed().as_micros() as u64,
                    kind: EventKind::Output(bytes),
                });
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {}
        }
        if let Some(status) = child.try_wait()? {
            // ConPTY output can trail process exit; drain until EOF or a bounded grace period.
            let drain_deadline = Instant::now() + Duration::from_millis(250);
            while Instant::now() < drain_deadline {
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(bytes) => {
                        std::io::stdout().write_all(&bytes)?;
                        events.push(CorpusEvent {
                            at_micros: started.elapsed().as_micros() as u64,
                            kind: EventKind::Output(bytes),
                        });
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
            exit_code = status.exit_code();
            break;
        }
    }
    events.push(CorpusEvent {
        at_micros: started.elapsed().as_micros() as u64,
        kind: EventKind::Exit { code: exit_code },
    });
    Corpus {
        initial_cols: cols,
        initial_rows: rows,
        events,
    }
    .write_to(fs::File::create(&output).with_context(|| format!("create {output}"))?)?;
    Ok(())
}

fn parse_input_plan(path: &str) -> Result<Vec<ScheduledInput>> {
    fs::read_to_string(path)?
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|(index, line)| {
            let (at_ms, hex) = line
                .split_once(':')
                .with_context(|| format!("{}:{} needs MS:HEX", path, index + 1))?;
            let hex = hex.trim();
            if hex.len() % 2 != 0 {
                bail!("{}:{} has odd-length HEX", path, index + 1);
            }
            let bytes = (0..hex.len())
                .step_by(2)
                .map(|offset| u8::from_str_radix(&hex[offset..offset + 2], 16))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(ScheduledInput {
                at_ms: at_ms.trim().parse()?,
                bytes,
            })
        })
        .collect()
}

fn parse_size(value: &str) -> Result<(u16, u16)> {
    let (cols, rows) = value.split_once('x').context("size must be COLSxROWS")?;
    Ok((cols.parse()?, rows.parse()?))
}

fn parse_resize(value: &str) -> Result<ScheduledResize> {
    let (at_ms, size) = value
        .split_once(':')
        .context("resize must be MS:COLSxROWS")?;
    let (cols, rows) = parse_size(size)?;
    Ok(ScheduledResize {
        at_ms: at_ms.parse()?,
        cols,
        rows,
    })
}
