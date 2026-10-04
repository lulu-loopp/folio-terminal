use std::fmt;

/// The four fixed bytes at the start of every elevated-host frame.
pub const FRAME_MAGIC: u32 = u32::from_le_bytes(*b"FADM");
/// The only elevated-host wire version this codec accepts.
pub const WIRE_VERSION: u16 = 1;
/// Bytes in the fixed frame header.
pub const FRAME_HEADER_LENGTH: usize = 20;
/// Maximum payload of every frame other than [`FrameKind::Output`].
pub const CONTROL_MAX_PAYLOAD: usize = 1024 * 1024;
/// Maximum payload of an [`FrameKind::Output`] frame.
pub const OUTPUT_MAX_PAYLOAD: usize = 64 * 1024;

const CAPABILITY_LENGTH: usize = 32;

/// The one-use 256-bit secret shared by one launch attempt.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Capability([u8; CAPABILITY_LENGTH]);

impl Capability {
    #[must_use]
    pub const fn new(bytes: [u8; CAPABILITY_LENGTH]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn bytes(self) -> [u8; CAPABILITY_LENGTH] {
        self.0
    }
}

/// A host process identity as represented on the wire.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct HostProcessId(pub u32);

/// The start identity paired with the parent pid outside this protocol.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ParentStartId(pub u64);

/// An address for one foreground-program request within a generation.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(pub u64);

/// UTF-16 code units kept without Unicode scalar-value normalization.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WireString(Vec<u16>);

impl WireString {
    #[must_use]
    pub fn new(units: Vec<u16>) -> Self {
        Self(units)
    }

    #[must_use]
    pub fn from_text(text: &str) -> Self {
        Self(text.encode_utf16().collect())
    }

    #[must_use]
    pub fn units(&self) -> &[u16] {
        &self.0
    }

    #[must_use]
    pub fn into_units(self) -> Vec<u16> {
        self.0
    }
}

/// The program and argument vector of a child birth.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnSpec {
    pub program: WireString,
    pub arguments: Vec<WireString>,
}

/// One environment replacement, in the order supplied by the parent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvironmentEntry {
    pub name: WireString,
    pub value: WireString,
}

/// The nonzero terminal dimensions handed to a child.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Grid {
    pub rows: u16,
    pub columns: u16,
}

/// Everything the host needs to create one child generation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpawnRequest {
    pub spec: SpawnSpec,
    pub cwd: Option<WireString>,
    pub grid: Grid,
    pub environment: Vec<EnvironmentEntry>,
}

/// The two ConPTY implementations an elevated Windows host can report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConPtyKind {
    Shipped,
    Inbox,
}

/// A structured child-start failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ErrorPayload {
    pub code: u32,
    pub message: WireString,
}

/// The optional status returned by the child API.
pub type ExitStatus = Option<u32>;

/// The normalized answer to a foreground-program query.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ForegroundProcess {
    Unknown,
    Known(WireString),
}

/// Which endpoint writes a frame kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Direction {
    HostToParent,
    ParentToHost,
}

/// Which endpoint is decoding a stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReaderRole {
    Parent,
    Host,
}

/// Stable numeric tags in design-note Revision (c).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[repr(u16)]
pub enum FrameKind {
    Hello = 1,
    Authenticate = 2,
    Spawn = 3,
    Started = 4,
    StartFailed = 5,
    Input = 6,
    Output = 7,
    Resize = 8,
    Exit = 9,
    ForegroundQuery = 10,
    ForegroundResult = 11,
    Restart = 12,
    Shutdown = 13,
    ShutdownAck = 14,
}

impl FrameKind {
    fn from_raw(raw: u16) -> Option<Self> {
        Some(match raw {
            1 => Self::Hello,
            2 => Self::Authenticate,
            3 => Self::Spawn,
            4 => Self::Started,
            5 => Self::StartFailed,
            6 => Self::Input,
            7 => Self::Output,
            8 => Self::Resize,
            9 => Self::Exit,
            10 => Self::ForegroundQuery,
            11 => Self::ForegroundResult,
            12 => Self::Restart,
            13 => Self::Shutdown,
            14 => Self::ShutdownAck,
            _ => return None,
        })
    }

    #[must_use]
    pub const fn direction(self) -> Direction {
        match self {
            Self::Hello
            | Self::Started
            | Self::StartFailed
            | Self::Output
            | Self::Exit
            | Self::ForegroundResult
            | Self::ShutdownAck => Direction::HostToParent,
            Self::Authenticate
            | Self::Spawn
            | Self::Input
            | Self::Resize
            | Self::ForegroundQuery
            | Self::Restart
            | Self::Shutdown => Direction::ParentToHost,
        }
    }

    const fn payload_limit(self) -> usize {
        match self {
            Self::Output => OUTPUT_MAX_PAYLOAD,
            _ => CONTROL_MAX_PAYLOAD,
        }
    }
}

/// Every payload in both directions of the elevated-host protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Message {
    Hello {
        capability: Capability,
        host_pid: HostProcessId,
    },
    Authenticate {
        capability: Capability,
        parent_start_id: ParentStartId,
    },
    Spawn(SpawnRequest),
    Started {
        child_pid: HostProcessId,
        conpty: ConPtyKind,
    },
    StartFailed(ErrorPayload),
    Input(Vec<u8>),
    Output(Vec<u8>),
    Resize(Grid),
    Exit(ExitStatus),
    ForegroundQuery(RequestId),
    ForegroundResult {
        request_id: RequestId,
        process: ForegroundProcess,
    },
    Restart(SpawnRequest),
    Shutdown(WireString),
    ShutdownAck,
}

impl Message {
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
        match self {
            Self::Hello { .. } => FrameKind::Hello,
            Self::Authenticate { .. } => FrameKind::Authenticate,
            Self::Spawn(_) => FrameKind::Spawn,
            Self::Started { .. } => FrameKind::Started,
            Self::StartFailed(_) => FrameKind::StartFailed,
            Self::Input(_) => FrameKind::Input,
            Self::Output(_) => FrameKind::Output,
            Self::Resize(_) => FrameKind::Resize,
            Self::Exit(_) => FrameKind::Exit,
            Self::ForegroundQuery(_) => FrameKind::ForegroundQuery,
            Self::ForegroundResult { .. } => FrameKind::ForegroundResult,
            Self::Restart(_) => FrameKind::Restart,
            Self::Shutdown(_) => FrameKind::Shutdown,
            Self::ShutdownAck => FrameKind::ShutdownAck,
        }
    }
}

/// A decoded message with the child generation carried by its header.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Frame {
    pub generation: u64,
    pub message: Message,
}

/// Why a complete payload could not represent its declared kind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MalformedPayload {
    FieldEndsEarly(&'static str),
    Utf16ByteCountIsOdd { field: &'static str, bytes: u32 },
    CountExceedsPayload { field: &'static str, count: u32 },
    UnknownTag { field: &'static str, tag: u8 },
    ZeroGridDimension,
    TrailingBytes { count: usize },
}

/// A boundary rejection. Each header, framing, payload, and role failure has a
/// distinct variant so transport code never has to inspect prose.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DecodeError {
    WrongMagic {
        found: u32,
    },
    UnknownWireVersion {
        found: u16,
    },
    UnknownKind {
        found: u16,
    },
    OversizedLength {
        kind: FrameKind,
        length: u32,
        maximum: usize,
    },
    TruncatedPayload {
        expected: usize,
        received: usize,
    },
    MalformedPayload {
        kind: FrameKind,
        problem: MalformedPayload,
    },
    WrongDirection {
        kind: FrameKind,
        reader: ReaderRole,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "elevated protocol decode error: {self:?}")
    }
}

impl std::error::Error for DecodeError {}

/// A locally constructed frame that cannot be represented within the protocol.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EncodeError {
    LengthOverflow {
        field: &'static str,
    },
    PayloadTooLarge {
        kind: FrameKind,
        length: usize,
        maximum: usize,
    },
    ZeroGridDimension,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "elevated protocol encode error: {self:?}")
    }
}

impl std::error::Error for EncodeError {}

/// Encode one complete frame.
pub fn encode(frame: &Frame) -> Result<Vec<u8>, EncodeError> {
    let kind = frame.message.kind();
    let mut payload = Vec::new();
    encode_message(&frame.message, &mut payload)?;
    let maximum = kind.payload_limit();
    if payload.len() > maximum {
        return Err(EncodeError::PayloadTooLarge {
            kind,
            length: payload.len(),
            maximum,
        });
    }
    let payload_length = u32::try_from(payload.len())
        .map_err(|_| EncodeError::LengthOverflow { field: "payload" })?;
    let mut bytes = Vec::with_capacity(FRAME_HEADER_LENGTH + payload.len());
    bytes.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
    bytes.extend_from_slice(&WIRE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&(kind as u16).to_le_bytes());
    bytes.extend_from_slice(&frame.generation.to_le_bytes());
    bytes.extend_from_slice(&payload_length.to_le_bytes());
    bytes.extend_from_slice(&payload);
    Ok(bytes)
}

/// Decode exactly one complete frame for `role`.
pub fn decode(bytes: &[u8], role: ReaderRole) -> Result<Frame, DecodeError> {
    let mut decoder = Decoder::new(role);
    let frames = decoder.push(bytes)?;
    decoder.finish()?;
    if frames.len() != 1 {
        return Err(DecodeError::MalformedPayload {
            kind: frames
                .first()
                .map_or(FrameKind::ShutdownAck, |frame| frame.message.kind()),
            problem: MalformedPayload::TrailingBytes {
                count: frames.len(),
            },
        });
    }
    Ok(frames.into_iter().next().expect("one frame was counted"))
}

/// Allocation facts exposed so hostile-length tests can assert the safety
/// property directly rather than infer it from elapsed time or survival.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DecoderAllocationAccounting {
    pub largest_payload_reservation: usize,
    pub payload_reservations: usize,
}

#[derive(Debug)]
struct PendingPayload {
    kind: FrameKind,
    generation: u64,
    expected: usize,
    bytes: Vec<u8>,
}

/// A decoder that accepts any stream chunking, including one byte per call.
#[derive(Debug)]
pub struct Decoder {
    role: ReaderRole,
    header: [u8; FRAME_HEADER_LENGTH],
    header_length: usize,
    payload: Option<PendingPayload>,
    accounting: DecoderAllocationAccounting,
}

impl Decoder {
    #[must_use]
    pub const fn new(role: ReaderRole) -> Self {
        Self {
            role,
            header: [0; FRAME_HEADER_LENGTH],
            header_length: 0,
            payload: None,
            accounting: DecoderAllocationAccounting {
                largest_payload_reservation: 0,
                payload_reservations: 0,
            },
        }
    }

    #[must_use]
    pub const fn allocation_accounting(&self) -> DecoderAllocationAccounting {
        self.accounting
    }

    pub fn push(&mut self, mut chunk: &[u8]) -> Result<Vec<Frame>, DecodeError> {
        let mut frames = Vec::new();
        while !chunk.is_empty() {
            if let Some(pending) = self.payload.as_mut() {
                let needed = pending.expected - pending.bytes.len();
                let taken = needed.min(chunk.len());
                pending.bytes.extend_from_slice(&chunk[..taken]);
                chunk = &chunk[taken..];
                if pending.bytes.len() == pending.expected {
                    let complete = self
                        .payload
                        .take()
                        .expect("the complete payload is present");
                    frames.push(decode_payload_rebound(
                        complete.kind,
                        complete.generation,
                        &complete.bytes,
                    )?);
                }
                continue;
            }

            let needed = FRAME_HEADER_LENGTH - self.header_length;
            let taken = needed.min(chunk.len());
            self.header[self.header_length..self.header_length + taken]
                .copy_from_slice(&chunk[..taken]);
            self.header_length += taken;
            chunk = &chunk[taken..];
            if self.header_length == FRAME_HEADER_LENGTH {
                let (kind, generation, payload_length) = validate_header(&self.header, self.role)?;
                self.header_length = 0;
                if payload_length == 0 {
                    frames.push(decode_payload_rebound(kind, generation, &[])?);
                } else {
                    self.accounting.largest_payload_reservation = self
                        .accounting
                        .largest_payload_reservation
                        .max(payload_length);
                    self.accounting.payload_reservations += 1;
                    self.payload = Some(PendingPayload {
                        kind,
                        generation,
                        expected: payload_length,
                        bytes: Vec::with_capacity(payload_length),
                    });
                }
            }
        }
        Ok(frames)
    }

    pub fn finish(&self) -> Result<(), DecodeError> {
        if let Some(pending) = &self.payload {
            return Err(DecodeError::TruncatedPayload {
                expected: pending.expected,
                received: pending.bytes.len(),
            });
        }
        if self.header_length != 0 {
            return Err(DecodeError::TruncatedPayload {
                expected: FRAME_HEADER_LENGTH,
                received: self.header_length,
            });
        }
        Ok(())
    }
}

fn validate_header(
    header: &[u8; FRAME_HEADER_LENGTH],
    role: ReaderRole,
) -> Result<(FrameKind, u64, usize), DecodeError> {
    let found_magic = u32::from_le_bytes(header[0..4].try_into().expect("fixed header slice"));
    if found_magic != FRAME_MAGIC {
        return Err(DecodeError::WrongMagic { found: found_magic });
    }
    let version = u16::from_le_bytes(header[4..6].try_into().expect("fixed header slice"));
    if version != WIRE_VERSION {
        return Err(DecodeError::UnknownWireVersion { found: version });
    }
    let raw_kind = u16::from_le_bytes(header[6..8].try_into().expect("fixed header slice"));
    let kind = FrameKind::from_raw(raw_kind).ok_or(DecodeError::UnknownKind { found: raw_kind })?;
    let payload_length = u32::from_le_bytes(header[16..20].try_into().expect("fixed header slice"));
    let maximum = kind.payload_limit();
    if u64::from(payload_length) > maximum as u64 {
        return Err(DecodeError::OversizedLength {
            kind,
            length: payload_length,
            maximum,
        });
    }
    let expected = match role {
        ReaderRole::Parent => Direction::HostToParent,
        ReaderRole::Host => Direction::ParentToHost,
    };
    if kind.direction() != expected {
        return Err(DecodeError::WrongDirection { kind, reader: role });
    }
    let generation = u64::from_le_bytes(header[8..16].try_into().expect("fixed header slice"));
    Ok((kind, generation, payload_length as usize))
}

fn encode_message(message: &Message, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    match message {
        Message::Hello {
            capability,
            host_pid,
        } => {
            out.extend_from_slice(&capability.bytes());
            put_u32(out, host_pid.0);
        }
        Message::Authenticate {
            capability,
            parent_start_id,
        } => {
            out.extend_from_slice(&capability.bytes());
            put_u64(out, parent_start_id.0);
        }
        Message::Spawn(request) | Message::Restart(request) => encode_spawn(request, out)?,
        Message::Started { child_pid, conpty } => {
            put_u32(out, child_pid.0);
            out.push(match conpty {
                ConPtyKind::Shipped => 1,
                ConPtyKind::Inbox => 2,
            });
        }
        Message::StartFailed(error) => {
            put_u32(out, error.code);
            put_string(out, &error.message, "start error")?;
        }
        Message::Input(bytes) | Message::Output(bytes) => out.extend_from_slice(bytes),
        Message::Resize(grid) => put_grid(out, *grid)?,
        Message::Exit(status) => match status {
            None => out.push(0),
            Some(status) => {
                out.push(1);
                put_u32(out, *status);
            }
        },
        Message::ForegroundQuery(request_id) => put_u64(out, request_id.0),
        Message::ForegroundResult {
            request_id,
            process,
        } => {
            put_u64(out, request_id.0);
            match process {
                ForegroundProcess::Unknown => out.push(0),
                ForegroundProcess::Known(image) => {
                    out.push(1);
                    put_string(out, image, "foreground image")?;
                }
            }
        }
        Message::Shutdown(reason) => put_string(out, reason, "shutdown reason")?,
        Message::ShutdownAck => {}
    }
    Ok(())
}

fn encode_spawn(request: &SpawnRequest, out: &mut Vec<u8>) -> Result<(), EncodeError> {
    put_string(out, &request.spec.program, "program")?;
    put_count(out, request.spec.arguments.len(), "arguments")?;
    for argument in &request.spec.arguments {
        put_string(out, argument, "argument")?;
    }
    match &request.cwd {
        None => out.push(0),
        Some(cwd) => {
            out.push(1);
            put_string(out, cwd, "working directory")?;
        }
    }
    put_grid(out, request.grid)?;
    put_count(out, request.environment.len(), "environment")?;
    for entry in &request.environment {
        put_string(out, &entry.name, "environment name")?;
        put_string(out, &entry.value, "environment value")?;
    }
    Ok(())
}

fn put_grid(out: &mut Vec<u8>, grid: Grid) -> Result<(), EncodeError> {
    if grid.rows == 0 || grid.columns == 0 {
        return Err(EncodeError::ZeroGridDimension);
    }
    put_u16(out, grid.rows);
    put_u16(out, grid.columns);
    Ok(())
}

fn put_count(out: &mut Vec<u8>, count: usize, field: &'static str) -> Result<(), EncodeError> {
    let count = u32::try_from(count).map_err(|_| EncodeError::LengthOverflow { field })?;
    put_u32(out, count);
    Ok(())
}

fn put_string(
    out: &mut Vec<u8>,
    string: &WireString,
    field: &'static str,
) -> Result<(), EncodeError> {
    let byte_length = string
        .units()
        .len()
        .checked_mul(2)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or(EncodeError::LengthOverflow { field })?;
    put_u32(out, byte_length);
    for unit in string.units() {
        put_u16(out, *unit);
    }
    Ok(())
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn decode_payload(kind: FrameKind, generation: u64, payload: &[u8]) -> Result<Frame, DecodeError> {
    let mut cursor = Cursor::new(payload);
    let message = match kind {
        FrameKind::Hello => Message::Hello {
            capability: cursor.capability("capability")?,
            host_pid: HostProcessId(cursor.u32("host pid")?),
        },
        FrameKind::Authenticate => Message::Authenticate {
            capability: cursor.capability("capability")?,
            parent_start_id: ParentStartId(cursor.u64("parent start identity")?),
        },
        FrameKind::Spawn => Message::Spawn(cursor.spawn()?),
        FrameKind::Started => Message::Started {
            child_pid: HostProcessId(cursor.u32("child pid")?),
            conpty: match cursor.u8("ConPTY kind")? {
                1 => ConPtyKind::Shipped,
                2 => ConPtyKind::Inbox,
                tag => {
                    return malformed(
                        kind,
                        MalformedPayload::UnknownTag {
                            field: "ConPTY kind",
                            tag,
                        },
                    );
                }
            },
        },
        FrameKind::StartFailed => Message::StartFailed(ErrorPayload {
            code: cursor.u32("error code")?,
            message: cursor.string("start error")?,
        }),
        FrameKind::Input => Message::Input(payload.to_vec()),
        FrameKind::Output => Message::Output(payload.to_vec()),
        FrameKind::Resize => Message::Resize(cursor.grid()?),
        FrameKind::Exit => Message::Exit(match cursor.u8("exit option")? {
            0 => None,
            1 => Some(cursor.u32("exit status")?),
            tag => {
                return malformed(
                    kind,
                    MalformedPayload::UnknownTag {
                        field: "exit option",
                        tag,
                    },
                );
            }
        }),
        FrameKind::ForegroundQuery => {
            Message::ForegroundQuery(RequestId(cursor.u64("request id")?))
        }
        FrameKind::ForegroundResult => {
            let request_id = RequestId(cursor.u64("request id")?);
            let process = match cursor.u8("foreground process")? {
                0 => ForegroundProcess::Unknown,
                1 => ForegroundProcess::Known(cursor.string("foreground image")?),
                tag => {
                    return malformed(
                        kind,
                        MalformedPayload::UnknownTag {
                            field: "foreground process",
                            tag,
                        },
                    );
                }
            };
            Message::ForegroundResult {
                request_id,
                process,
            }
        }
        FrameKind::Restart => Message::Restart(cursor.spawn()?),
        FrameKind::Shutdown => Message::Shutdown(cursor.string("shutdown reason")?),
        FrameKind::ShutdownAck => Message::ShutdownAck,
    };
    if !matches!(kind, FrameKind::Input | FrameKind::Output) && cursor.remaining() != 0 {
        return malformed(
            kind,
            MalformedPayload::TrailingBytes {
                count: cursor.remaining(),
            },
        );
    }
    Ok(Frame {
        generation,
        message,
    })
}

fn malformed<T>(kind: FrameKind, problem: MalformedPayload) -> Result<T, DecodeError> {
    Err(DecodeError::MalformedPayload { kind, problem })
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take(&mut self, count: usize, field: &'static str) -> Result<&'a [u8], DecodeError> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or(DecodeError::MalformedPayload {
                kind: FrameKind::ShutdownAck,
                problem: MalformedPayload::FieldEndsEarly(field),
            })?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn u8(&mut self, field: &'static str) -> Result<u8, DecodeError> {
        Ok(self.take(1, field)?[0])
    }

    fn u16(&mut self, field: &'static str) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(
            self.take(2, field)?
                .try_into()
                .expect("two bytes were taken"),
        ))
    }

    fn u32(&mut self, field: &'static str) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(
            self.take(4, field)?
                .try_into()
                .expect("four bytes were taken"),
        ))
    }

    fn u64(&mut self, field: &'static str) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(
            self.take(8, field)?
                .try_into()
                .expect("eight bytes were taken"),
        ))
    }

    fn capability(&mut self, field: &'static str) -> Result<Capability, DecodeError> {
        Ok(Capability::new(
            self.take(CAPABILITY_LENGTH, field)?
                .try_into()
                .expect("the capability length was taken"),
        ))
    }

    fn string(&mut self, field: &'static str) -> Result<WireString, DecodeError> {
        let byte_length = self.u32(field)?;
        if byte_length % 2 != 0 {
            return Err(DecodeError::MalformedPayload {
                kind: FrameKind::ShutdownAck,
                problem: MalformedPayload::Utf16ByteCountIsOdd {
                    field,
                    bytes: byte_length,
                },
            });
        }
        let bytes = self.take(byte_length as usize, field)?;
        let mut units = Vec::with_capacity(bytes.len() / 2);
        for pair in bytes.chunks_exact(2) {
            units.push(u16::from_le_bytes([pair[0], pair[1]]));
        }
        Ok(WireString::new(units))
    }

    fn count(&mut self, field: &'static str, minimum_each: usize) -> Result<usize, DecodeError> {
        let count = self.u32(field)?;
        if u64::from(count).saturating_mul(minimum_each as u64) > self.remaining() as u64 {
            return Err(DecodeError::MalformedPayload {
                kind: FrameKind::ShutdownAck,
                problem: MalformedPayload::CountExceedsPayload { field, count },
            });
        }
        Ok(count as usize)
    }

    fn grid(&mut self) -> Result<Grid, DecodeError> {
        let grid = Grid {
            rows: self.u16("grid rows")?,
            columns: self.u16("grid columns")?,
        };
        if grid.rows == 0 || grid.columns == 0 {
            return Err(DecodeError::MalformedPayload {
                kind: FrameKind::ShutdownAck,
                problem: MalformedPayload::ZeroGridDimension,
            });
        }
        Ok(grid)
    }

    fn spawn(&mut self) -> Result<SpawnRequest, DecodeError> {
        let program = self.string("program")?;
        let argument_count = self.count("arguments", 4)?;
        let mut arguments = Vec::with_capacity(argument_count);
        for _ in 0..argument_count {
            arguments.push(self.string("argument")?);
        }
        let cwd = match self.u8("working directory option")? {
            0 => None,
            1 => Some(self.string("working directory")?),
            tag => {
                return Err(DecodeError::MalformedPayload {
                    kind: FrameKind::ShutdownAck,
                    problem: MalformedPayload::UnknownTag {
                        field: "working directory option",
                        tag,
                    },
                });
            }
        };
        let grid = self.grid()?;
        let environment_count = self.count("environment", 8)?;
        let mut environment = Vec::with_capacity(environment_count);
        for _ in 0..environment_count {
            environment.push(EnvironmentEntry {
                name: self.string("environment name")?,
                value: self.string("environment value")?,
            });
        }
        Ok(SpawnRequest {
            spec: SpawnSpec { program, arguments },
            cwd,
            grid,
            environment,
        })
    }
}

fn rebind_payload_error(kind: FrameKind, error: DecodeError) -> DecodeError {
    match error {
        DecodeError::MalformedPayload { problem, .. } => {
            DecodeError::MalformedPayload { kind, problem }
        }
        other => other,
    }
}

// Cursor has no kind so its small readers stay reusable. Rebind their sentinel
// kind once, at the complete-frame boundary.
fn decode_payload_rebound(
    kind: FrameKind,
    generation: u64,
    payload: &[u8],
) -> Result<Frame, DecodeError> {
    decode_payload(kind, generation, payload).map_err(|error| rebind_payload_error(kind, error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability() -> Capability {
        Capability::new(std::array::from_fn(|index| index as u8))
    }

    fn grid() -> Grid {
        Grid {
            rows: 37,
            columns: 143,
        }
    }

    fn spawn_request() -> SpawnRequest {
        SpawnRequest {
            spec: SpawnSpec {
                program: WireString::from_text(r"C:\Program Files\PowerShell\7\pwsh.exe"),
                arguments: vec![
                    WireString::from_text("-NoLogo"),
                    WireString::from_text("\u{4e2d}\u{6587}-Καλημέρα-мир"),
                    WireString::new(vec![0xd800, 0x20, 0x0061]),
                ],
            },
            cwd: Some(WireString::from_text(
                "C:\\A folder\\mixed \u{4e2d}\u{6587}",
            )),
            grid: grid(),
            environment: vec![
                EnvironmentEntry {
                    name: WireString::from_text("Path"),
                    value: WireString::from_text(r"C:\One;D:\Two"),
                },
                EnvironmentEntry {
                    name: WireString::new(vec![0x0058, 0xdfff]),
                    value: WireString::from_text("δεύτερο"),
                },
            ],
        }
    }

    fn role_for(message: &Message) -> ReaderRole {
        match message.kind().direction() {
            Direction::HostToParent => ReaderRole::Parent,
            Direction::ParentToHost => ReaderRole::Host,
        }
    }

    fn bytes_for(kind: FrameKind, generation: u64, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&FRAME_MAGIC.to_le_bytes());
        bytes.extend_from_slice(&WIRE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&(kind as u16).to_le_bytes());
        bytes.extend_from_slice(&generation.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(payload);
        bytes
    }

    /// RED MUTATION: encode `WireString` with `String::from_utf16_lossy`; the
    /// unpaired-surrogate Spawn frame no longer equals its decoded frame.
    #[test]
    fn every_frame_kind_round_trips_with_empty_and_lossless_text_payloads() {
        let request = spawn_request();
        let frames = vec![
            Frame {
                generation: 0,
                message: Message::Hello {
                    capability: capability(),
                    host_pid: HostProcessId(41),
                },
            },
            Frame {
                generation: 0,
                message: Message::Authenticate {
                    capability: capability(),
                    parent_start_id: ParentStartId(0x1122_3344_5566_7788),
                },
            },
            Frame {
                generation: 1,
                message: Message::Spawn(request.clone()),
            },
            Frame {
                generation: 1,
                message: Message::Started {
                    child_pid: HostProcessId(42),
                    conpty: ConPtyKind::Shipped,
                },
            },
            Frame {
                generation: 1,
                message: Message::StartFailed(ErrorPayload {
                    code: 5,
                    message: WireString::from_text("access denied"),
                }),
            },
            Frame {
                generation: 1,
                message: Message::Input(Vec::new()),
            },
            Frame {
                generation: 1,
                message: Message::Output(Vec::new()),
            },
            Frame {
                generation: 1,
                message: Message::Resize(grid()),
            },
            Frame {
                generation: 1,
                message: Message::Exit(None),
            },
            Frame {
                generation: 1,
                message: Message::ForegroundQuery(RequestId(7)),
            },
            Frame {
                generation: 1,
                message: Message::ForegroundResult {
                    request_id: RequestId(7),
                    process: ForegroundProcess::Known(WireString::from_text(r"C:\Tools\agent.exe")),
                },
            },
            Frame {
                generation: 2,
                message: Message::Restart(request),
            },
            Frame {
                generation: 2,
                message: Message::Shutdown(WireString::new(Vec::new())),
            },
            Frame {
                generation: 2,
                message: Message::ShutdownAck,
            },
        ];
        for frame in frames {
            let bytes = encode(&frame).expect("the fixture is representable");
            assert_eq!(decode(&bytes, role_for(&frame.message)), Ok(frame));
        }
    }

    /// RED MUTATION: make `FrameKind::Output::payload_limit` return the control
    /// bound; the output-at-bound-plus-one assertion accepts it.
    #[test]
    fn each_payload_class_accepts_its_bound_and_rejects_bound_plus_one() {
        for (message, maximum) in [
            (
                Message::Input(vec![0x5a; CONTROL_MAX_PAYLOAD]),
                CONTROL_MAX_PAYLOAD,
            ),
            (
                Message::Output(vec![0xa5; OUTPUT_MAX_PAYLOAD]),
                OUTPUT_MAX_PAYLOAD,
            ),
        ] {
            let frame = Frame {
                generation: 9,
                message,
            };
            let bytes = encode(&frame).expect("the exact bound is accepted");
            assert_eq!(decode(&bytes, role_for(&frame.message)), Ok(frame.clone()));

            let kind = frame.message.kind();
            let over = match kind {
                FrameKind::Input => Message::Input(vec![0; maximum + 1]),
                FrameKind::Output => Message::Output(vec![0; maximum + 1]),
                _ => unreachable!("the fixture has the two raw payload kinds"),
            };
            assert_eq!(
                encode(&Frame {
                    generation: 9,
                    message: over,
                }),
                Err(EncodeError::PayloadTooLarge {
                    kind,
                    length: maximum + 1,
                    maximum,
                })
            );
        }
    }

    /// RED MUTATION: clear `header_length` after every `push`; the one-byte
    /// decoder differs from the whole-buffer decoder at the first header.
    #[test]
    fn arbitrary_chunk_boundaries_equal_whole_buffer_decoding() {
        let frames = [
            Frame {
                generation: 4,
                message: Message::Output(vec![1, 2, 3, 4, 5]),
            },
            Frame {
                generation: 4,
                message: Message::ForegroundResult {
                    request_id: RequestId(99),
                    process: ForegroundProcess::Unknown,
                },
            },
            Frame {
                generation: 4,
                message: Message::Exit(Some(17)),
            },
        ];
        let stream: Vec<u8> = frames
            .iter()
            .flat_map(|frame| encode(frame).expect("fixture frame"))
            .collect();

        let decode_in = |chunks: Vec<&[u8]>| {
            let mut decoder = Decoder::new(ReaderRole::Parent);
            let mut decoded = Vec::new();
            for chunk in chunks {
                decoded.extend(decoder.push(chunk).expect("valid stream"));
            }
            decoder.finish().expect("complete stream");
            decoded
        };
        let whole = decode_in(vec![&stream]);
        assert_eq!(whole, frames);
        assert_eq!(
            decode_in(stream.as_slice().chunks(1).collect()),
            whole,
            "one byte at a time"
        );

        let mut seed = 0x6a09_e667_f3bc_c909_u64;
        let mut chunks = Vec::new();
        let mut at = 0;
        while at < stream.len() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let length = ((seed as usize) % 31 + 1).min(stream.len() - at);
            chunks.push(&stream[at..at + length]);
            at += length;
        }
        assert_eq!(decode_in(chunks), whole, "deterministic random chunks");
    }

    /// RED MUTATION: remove the even-byte check in `Cursor::string`; the odd
    /// UTF-16 case is no longer its own malformed-payload rejection.
    #[test]
    fn every_decode_rejection_has_its_typed_variant() {
        let ack = encode(&Frame {
            generation: 2,
            message: Message::ShutdownAck,
        })
        .expect("fixture frame");

        let mut wrong_magic = ack.clone();
        wrong_magic[0] ^= 0xff;
        assert!(matches!(
            decode(&wrong_magic, ReaderRole::Parent),
            Err(DecodeError::WrongMagic { .. })
        ));
        let mut wrong_version = ack.clone();
        wrong_version[4..6].copy_from_slice(&(WIRE_VERSION + 1).to_le_bytes());
        assert_eq!(
            decode(&wrong_version, ReaderRole::Parent),
            Err(DecodeError::UnknownWireVersion {
                found: WIRE_VERSION + 1
            })
        );
        let mut wrong_kind = ack.clone();
        wrong_kind[6..8].copy_from_slice(&99_u16.to_le_bytes());
        assert_eq!(
            decode(&wrong_kind, ReaderRole::Parent),
            Err(DecodeError::UnknownKind { found: 99 })
        );

        let mut oversized_output = bytes_for(FrameKind::Output, 1, &[]);
        oversized_output[16..20].copy_from_slice(&((OUTPUT_MAX_PAYLOAD + 1) as u32).to_le_bytes());
        assert!(matches!(
            decode(&oversized_output, ReaderRole::Parent),
            Err(DecodeError::OversizedLength {
                kind: FrameKind::Output,
                ..
            })
        ));
        let mut oversized_control = bytes_for(FrameKind::Input, 1, &[]);
        oversized_control[16..20]
            .copy_from_slice(&((CONTROL_MAX_PAYLOAD + 1) as u32).to_le_bytes());
        assert!(matches!(
            decode(&oversized_control, ReaderRole::Host),
            Err(DecodeError::OversizedLength {
                kind: FrameKind::Input,
                ..
            })
        ));

        let mut short_header = Decoder::new(ReaderRole::Parent);
        short_header
            .push(&ack[..19])
            .expect("not rejected until EOF");
        assert_eq!(
            short_header.finish(),
            Err(DecodeError::TruncatedPayload {
                expected: FRAME_HEADER_LENGTH,
                received: 19,
            })
        );
        let mut short_payload = Decoder::new(ReaderRole::Parent);
        let declared_three = bytes_for(FrameKind::Output, 1, &[1, 2, 3]);
        short_payload
            .push(&declared_three[..declared_three.len() - 1])
            .expect("not rejected until EOF");
        assert_eq!(
            short_payload.finish(),
            Err(DecodeError::TruncatedPayload {
                expected: 3,
                received: 2,
            })
        );

        assert!(matches!(
            decode(
                &bytes_for(FrameKind::Shutdown, 1, &[10, 0, 0, 0, 0x61, 0]),
                ReaderRole::Host
            ),
            Err(DecodeError::MalformedPayload {
                problem: MalformedPayload::FieldEndsEarly("shutdown reason"),
                ..
            })
        ));
        assert!(matches!(
            decode(
                &bytes_for(FrameKind::Shutdown, 1, &[1, 0, 0, 0, 0]),
                ReaderRole::Host
            ),
            Err(DecodeError::MalformedPayload {
                problem: MalformedPayload::Utf16ByteCountIsOdd { .. },
                ..
            })
        ));
        assert!(matches!(
            decode(
                &bytes_for(FrameKind::Spawn, 1, &[0, 0, 0, 0, 2, 0, 0, 0]),
                ReaderRole::Host
            ),
            Err(DecodeError::MalformedPayload {
                problem: MalformedPayload::CountExceedsPayload {
                    field: "arguments",
                    count: 2,
                },
                ..
            })
        ));
        assert!(matches!(
            decode(&bytes_for(FrameKind::Exit, 1, &[2]), ReaderRole::Parent),
            Err(DecodeError::MalformedPayload {
                problem: MalformedPayload::UnknownTag { .. },
                ..
            })
        ));
        assert!(matches!(
            decode(
                &bytes_for(FrameKind::Resize, 1, &[0, 0, 4, 0]),
                ReaderRole::Host
            ),
            Err(DecodeError::MalformedPayload {
                problem: MalformedPayload::ZeroGridDimension,
                ..
            })
        ));
        assert!(matches!(
            decode(
                &bytes_for(FrameKind::ShutdownAck, 1, &[0]),
                ReaderRole::Parent
            ),
            Err(DecodeError::MalformedPayload {
                problem: MalformedPayload::TrailingBytes { count: 1 },
                ..
            })
        ));
        assert_eq!(
            decode(
                &encode(&Frame {
                    generation: 1,
                    message: Message::Input(Vec::new()),
                })
                .expect("fixture frame"),
                ReaderRole::Parent,
            ),
            Err(DecodeError::WrongDirection {
                kind: FrameKind::Input,
                reader: ReaderRole::Parent,
            })
        );
    }

    /// RED MUTATION: increment the payload-reservation accounting before the
    /// header's length validation; the hostile header records a reservation.
    #[test]
    fn hostile_u32_max_length_is_rejected_before_any_payload_reservation() {
        for (kind, role) in [
            (FrameKind::Output, ReaderRole::Parent),
            (FrameKind::Input, ReaderRole::Host),
        ] {
            let mut bytes = bytes_for(kind, 1, &[]);
            bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
            let mut decoder = Decoder::new(role);
            assert!(matches!(
                decoder.push(&bytes),
                Err(DecodeError::OversizedLength {
                    length: u32::MAX,
                    ..
                })
            ));
            assert_eq!(
                decoder.allocation_accounting(),
                DecoderAllocationAccounting::default()
            );
        }
    }

    /// RED MUTATION: index the first payload byte without checking it; the
    /// deterministic empty/small corpus panics.
    #[test]
    fn arbitrary_byte_strings_never_panic_the_decoder() {
        let outcome = std::panic::catch_unwind(|| {
            let mut seed = 0xbb67_ae85_84ca_a73b_u64;
            for length in 0..1024 {
                let mut bytes = vec![0; length];
                for byte in &mut bytes {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    *byte = seed as u8;
                }
                for role in [ReaderRole::Parent, ReaderRole::Host] {
                    let mut whole = Decoder::new(role);
                    let _ = whole.push(&bytes);
                    let _ = whole.finish();

                    let mut decoder = Decoder::new(role);
                    for chunk in bytes.chunks(1 + (length % 17)) {
                        if decoder.push(chunk).is_err() {
                            break;
                        }
                    }
                    let _ = decoder.finish();
                }
            }
        });
        assert!(outcome.is_ok());
    }
}
