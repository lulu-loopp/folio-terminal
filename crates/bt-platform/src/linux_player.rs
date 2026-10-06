//! GStreamer playback for Linux.
//!
//! Each player pipeline belongs to one Folio worker. Its appsink publishes the
//! newest BGRA frame to the renderer, and playbin owns the media clock and
//! audio output. The small dynamic binding below uses the system GStreamer
//! runtime directly, so a Linux build does not need GStreamer headers or a
//! development package at compile time.

#![allow(unsafe_code)]

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use libloading::Library;

use crate::ThreadPriority;
use crate::admission::{self, WaitToken, WorkerCtx, doors};
use crate::video::engine::{
    EngineError, EngineState, FRAME_POLL_INTERVAL, Frame, FrameCost, IDLE_POLL_INTERVAL,
    LedgerEntry, OPEN_BUDGET, SHUTDOWN_BUDGET, note_engine_shut_down,
};

const ENGINE_WORKER: &str = "folio-video-engine";
const GST_SECOND: u64 = 1_000_000_000;
const GST_STATE_NULL: c_int = 1;
const GST_STATE_PAUSED: c_int = 3;
const GST_STATE_PLAYING: c_int = 4;
const GST_STATE_VOID_PENDING: c_int = 0;
const GST_STATE_CHANGE_FAILURE: c_int = 0;
const GST_STATE_CHANGE_ASYNC: c_int = 2;
const GST_FORMAT_TIME: c_int = 3;
const GST_SEEK_TYPE_NONE: c_int = 0;
const GST_SEEK_TYPE_SET: c_int = 1;
const GST_SEEK_FLAG_FLUSH: u32 = 1;
const GST_SEEK_FLAG_ACCURATE: u32 = 2;
const GST_MESSAGE_EOS: c_int = 1;
const GST_MESSAGE_ERROR: c_int = 1 << 1;

#[repr(C)]
struct GError {
    domain: u32,
    code: c_int,
    message: *mut c_char,
}

/// Public prefix of GStreamer's GstMiniObject and GstMessage. The message
/// type is the first field after the mini-object header in the stable 1.x ABI.
#[repr(C)]
struct GstMiniObject {
    _type: usize,
    _refcount: c_int,
    _lockstate: c_int,
    _flags: u32,
    _copy: *mut c_void,
    _dispose: *mut c_void,
    _free: *mut c_void,
    _priv_uint: u32,
    _priv_pointer: *mut c_void,
}

#[repr(C)]
struct GstMessagePrefix {
    _mini_object: GstMiniObject,
    message_type: c_int,
}

type GstInitCheck =
    unsafe extern "C" fn(*mut c_int, *mut *mut *mut c_char, *mut *mut GError) -> c_int;
type GstElementFactoryMake = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut c_void;
type GstElementSetState = unsafe extern "C" fn(*mut c_void, c_int) -> c_int;
type GstElementGetState = unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int, u64) -> c_int;
type GstElementGetBus = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type GstElementQueryTime = unsafe extern "C" fn(*mut c_void, c_int, *mut i64) -> c_int;
type GstElementSeek =
    unsafe extern "C" fn(*mut c_void, f64, c_int, u32, c_int, i64, c_int, i64) -> c_int;
type GstBusPop = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type GstMessageParseError = unsafe extern "C" fn(*mut c_void, *mut *mut GError, *mut *mut c_char);
type GstMiniObjectUnref = unsafe extern "C" fn(*mut c_void);
type GstObjectUnref = unsafe extern "C" fn(*mut c_void);
type GstObjectRefSink = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type GstCapsFromString = unsafe extern "C" fn(*const c_char) -> *mut c_void;
type GstCapsGetStructure = unsafe extern "C" fn(*mut c_void, u32) -> *const c_void;
type GstSampleGetObject = unsafe extern "C" fn(*mut c_void) -> *mut c_void;
type GstStructureGetInt = unsafe extern "C" fn(*const c_void, *const c_char, *mut c_int) -> c_int;
type GstBufferGetSize = unsafe extern "C" fn(*mut c_void) -> usize;
type GstBufferExtract = unsafe extern "C" fn(*mut c_void, usize, *mut c_void, usize) -> usize;
type GstAppSinkTryPull = unsafe extern "C" fn(*mut c_void, u64) -> *mut c_void;
type GObjectSet = unsafe extern "C" fn(*mut c_void, *const c_char, ...);
type GObjectGet = unsafe extern "C" fn(*mut c_void, *const c_char, ...);
type GErrorFree = unsafe extern "C" fn(*mut GError);
type GFree = unsafe extern "C" fn(*mut c_void);
type GQuarkToString = unsafe extern "C" fn(u32) -> *const c_char;

/// The exact runtime entry points used by the player. The libraries outlive all
/// copied function pointers because this value is process-global.
struct GstApi {
    _gst: Library,
    _app: Library,
    _gobject: Library,
    _glib: Library,
    init_check: GstInitCheck,
    element_factory_make: GstElementFactoryMake,
    element_set_state: GstElementSetState,
    element_get_state: GstElementGetState,
    element_get_bus: GstElementGetBus,
    element_query_position: GstElementQueryTime,
    element_query_duration: GstElementQueryTime,
    element_seek: GstElementSeek,
    bus_pop: GstBusPop,
    message_parse_error: GstMessageParseError,
    mini_object_unref: GstMiniObjectUnref,
    object_unref: GstObjectUnref,
    object_ref_sink: GstObjectRefSink,
    caps_from_string: GstCapsFromString,
    caps_get_structure: GstCapsGetStructure,
    sample_get_caps: GstSampleGetObject,
    sample_get_buffer: GstSampleGetObject,
    structure_get_int: GstStructureGetInt,
    buffer_get_size: GstBufferGetSize,
    buffer_extract: GstBufferExtract,
    app_sink_try_pull_preroll: GstAppSinkTryPull,
    app_sink_try_pull_sample: GstAppSinkTryPull,
    object_set: GObjectSet,
    object_get: GObjectGet,
    error_free: GErrorFree,
    free: GFree,
    quark_to_string: GQuarkToString,
}

impl GstApi {
    fn load() -> Result<Self, &'static str> {
        // SAFETY: these are the stable sonames of the system GStreamer runtime
        // and its GLib support libraries. The handles are retained in `GstApi`.
        let gst = unsafe { Library::new("libgstreamer-1.0.so.0") }
            .map_err(|_| "the GStreamer runtime is unavailable")?;
        // SAFETY: the appsink ABI is provided by the matching GStreamer install.
        let app = unsafe { Library::new("libgstapp-1.0.so.0") }
            .map_err(|_| "the GStreamer appsink runtime is unavailable")?;
        // SAFETY: GObject and GLib are GStreamer's required runtime dependencies.
        let gobject = unsafe { Library::new("libgobject-2.0.so.0") }
            .map_err(|_| "the GObject runtime is unavailable")?;
        // SAFETY: GObject's error and allocation functions are supplied by GLib.
        let glib = unsafe { Library::new("libglib-2.0.so.0") }
            .map_err(|_| "the GLib runtime is unavailable")?;

        macro_rules! symbol {
            ($library:ident, $name:literal, $kind:ty) => {{
                // SAFETY: the declared C ABI and symbol name match the installed
                // GStreamer 1.x headers; the owning library remains in this struct.
                unsafe {
                    *$library
                        .get::<$kind>(concat!($name, "\0").as_bytes())
                        .map_err(|_| "a required GStreamer symbol is unavailable")?
                }
            }};
        }

        Ok(Self {
            init_check: symbol!(gst, "gst_init_check", GstInitCheck),
            element_factory_make: symbol!(gst, "gst_element_factory_make", GstElementFactoryMake),
            element_set_state: symbol!(gst, "gst_element_set_state", GstElementSetState),
            element_get_state: symbol!(gst, "gst_element_get_state", GstElementGetState),
            element_get_bus: symbol!(gst, "gst_element_get_bus", GstElementGetBus),
            element_query_position: symbol!(gst, "gst_element_query_position", GstElementQueryTime),
            element_query_duration: symbol!(gst, "gst_element_query_duration", GstElementQueryTime),
            element_seek: symbol!(gst, "gst_element_seek", GstElementSeek),
            bus_pop: symbol!(gst, "gst_bus_pop", GstBusPop),
            message_parse_error: symbol!(gst, "gst_message_parse_error", GstMessageParseError),
            mini_object_unref: symbol!(gst, "gst_mini_object_unref", GstMiniObjectUnref),
            object_unref: symbol!(gst, "gst_object_unref", GstObjectUnref),
            object_ref_sink: symbol!(gst, "gst_object_ref_sink", GstObjectRefSink),
            caps_from_string: symbol!(gst, "gst_caps_from_string", GstCapsFromString),
            caps_get_structure: symbol!(gst, "gst_caps_get_structure", GstCapsGetStructure),
            sample_get_caps: symbol!(gst, "gst_sample_get_caps", GstSampleGetObject),
            sample_get_buffer: symbol!(gst, "gst_sample_get_buffer", GstSampleGetObject),
            structure_get_int: symbol!(gst, "gst_structure_get_int", GstStructureGetInt),
            buffer_get_size: symbol!(gst, "gst_buffer_get_size", GstBufferGetSize),
            buffer_extract: symbol!(gst, "gst_buffer_extract", GstBufferExtract),
            app_sink_try_pull_preroll: symbol!(
                app,
                "gst_app_sink_try_pull_preroll",
                GstAppSinkTryPull
            ),
            app_sink_try_pull_sample: symbol!(
                app,
                "gst_app_sink_try_pull_sample",
                GstAppSinkTryPull
            ),
            object_set: symbol!(gobject, "g_object_set", GObjectSet),
            object_get: symbol!(gobject, "g_object_get", GObjectGet),
            error_free: symbol!(glib, "g_error_free", GErrorFree),
            free: symbol!(glib, "g_free", GFree),
            quark_to_string: symbol!(glib, "g_quark_to_string", GQuarkToString),
            _gst: gst,
            _app: app,
            _gobject: gobject,
            _glib: glib,
        })
    }
}

fn gst_api() -> Result<&'static GstApi, EngineError> {
    static API: OnceLock<Result<GstApi, &'static str>> = OnceLock::new();
    match API.get_or_init(GstApi::load) {
        Ok(api) => Ok(api),
        Err(_) => Err(EngineError::Unsupported),
    }
}

fn initialize_gstreamer(api: &GstApi) -> Result<(), EngineError> {
    static INITIALIZED: OnceLock<bool> = OnceLock::new();
    if *INITIALIZED.get_or_init(|| {
        let mut error = ptr::null_mut();
        // SAFETY: null argument pointers ask GStreamer to use the current
        // process arguments, which it does not modify when they are null.
        let initialized = unsafe { (api.init_check)(ptr::null_mut(), ptr::null_mut(), &mut error) };
        if !error.is_null() {
            // SAFETY: GStreamer returned this owned GError for this call.
            unsafe { (api.error_free)(error) };
        }
        initialized != 0
    }) {
        Ok(())
    } else {
        Err(EngineError::Unsupported)
    }
}

fn file_uri(_ctx: &WorkerCtx, path: &Path) -> Result<CString, EngineError> {
    use std::os::unix::ffi::OsStrExt;

    let path = std::fs::canonicalize(path)
        .map_err(|_| EngineError::Other("the video source could not be opened"))?;
    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'/' | b'-' | b'.' | b'_' | b'~') {
            uri.push(char::from(*byte));
        } else {
            uri.push('%');
            uri.push(char::from(b"0123456789ABCDEF"[usize::from(byte >> 4)]));
            uri.push(char::from(b"0123456789ABCDEF"[usize::from(byte & 0x0f)]));
        }
    }
    CString::new(uri).map_err(|_| EngineError::Other("the video path has no URI"))
}

fn decode_error(api: &GstApi, error: *mut GError) -> EngineError {
    if error.is_null() {
        return EngineError::Decode;
    }
    // SAFETY: `error` is the owned GError filled by GStreamer and its fields
    // retain their GLib C ABI layout until it is freed below.
    let (domain, code, message) = unsafe {
        let details = &*error;
        let domain_name = (api.quark_to_string)(details.domain);
        let domain = if domain_name.is_null() {
            ""
        } else {
            CStr::from_ptr(domain_name).to_str().unwrap_or("")
        };
        let message = if details.message.is_null() {
            String::new()
        } else {
            CStr::from_ptr(details.message)
                .to_string_lossy()
                .into_owned()
        };
        (domain.to_owned(), details.code, message)
    };
    eprintln!("video: GStreamer {domain} error {code}: {message}");
    // SAFETY: GStreamer transferred this GError to the caller.
    unsafe { (api.error_free)(error) };
    if domain == "gst-stream-error-quark" && (4..=6).contains(&code)
        || domain == "gst-core-error-quark" && code == 12
    {
        EngineError::Unsupported
    } else if domain == "gst-resource-error-quark" && code == 3 {
        EngineError::Other("the video source could not be opened")
    } else {
        EngineError::Decode
    }
}

fn unref(api: &GstApi, object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: every object passed here is a full reference returned by a
        // GStreamer constructor, getter, or pull operation.
        unsafe { (api.object_unref)(object) };
    }
}

fn mini_unref(api: &GstApi, object: *mut c_void) {
    if !object.is_null() {
        // SAFETY: every object passed here is a full GstMiniObject reference.
        unsafe { (api.mini_object_unref)(object) };
    }
}

#[derive(Clone, Copy)]
enum AudioOutput {
    System,
    #[cfg(test)]
    Silent,
}

/// Native objects stay on the worker that created them. The marker also keeps
/// a player from being sent to another thread after a pipeline is built.
struct GstPlayer {
    api: &'static GstApi,
    pipeline: *mut c_void,
    sink: *mut c_void,
    audio_sink: *mut c_void,
    bus: *mut c_void,
    state: EngineState,
    generation: u64,
    preroll_taken: bool,
    requested_playing: bool,
    _worker_only: PhantomData<*mut ()>,
}

impl GstPlayer {
    fn open(_ctx: &WorkerCtx, path: &Path, audio_output: AudioOutput) -> Result<Self, EngineError> {
        let api = gst_api()?;
        initialize_gstreamer(api)?;
        let uri = file_uri(_ctx, path)?;

        // SAFETY: GStreamer returns new element references for these factory
        // calls. Null names let it choose unique names within this pipeline.
        let pipeline = unsafe { (api.element_factory_make)(c"playbin".as_ptr(), ptr::null()) };
        let sink = unsafe { (api.element_factory_make)(c"appsink".as_ptr(), ptr::null()) };
        if pipeline.is_null() || sink.is_null() {
            unref(api, sink);
            unref(api, pipeline);
            return Err(EngineError::Unsupported);
        }
        // Factory-created elements carry a floating reference. Sink it before
        // assigning GObject properties so this player retains its own reference
        // in addition to the reference held by playbin.
        // SAFETY: both values are newly created GstObjects with floating refs.
        unsafe {
            (api.object_ref_sink)(pipeline);
            (api.object_ref_sink)(sink);
        }

        // SAFETY: the caps string is static and valid GStreamer caps syntax.
        let caps = unsafe { (api.caps_from_string)(c"video/x-raw,format=(string)BGRA".as_ptr()) };
        if caps.is_null() {
            unref(api, sink);
            unref(api, pipeline);
            return Err(EngineError::Unsupported);
        }

        // SAFETY: these property names and values match GstAppSink and playbin
        // properties. Each `g_object_set` call ends with its required null name.
        unsafe {
            (api.object_set)(
                sink,
                c"caps".as_ptr(),
                caps,
                c"max-buffers".as_ptr(),
                1_u32,
                c"drop".as_ptr(),
                1_i32,
                c"async".as_ptr(),
                0_i32,
                c"sync".as_ptr(),
                1_i32,
                c"wait-on-eos".as_ptr(),
                0_i32,
                c"enable-last-sample".as_ptr(),
                0_i32,
                ptr::null::<c_char>(),
            );
            (api.object_set)(
                pipeline,
                c"uri".as_ptr(),
                uri.as_ptr(),
                c"video-sink".as_ptr(),
                sink,
                ptr::null::<c_char>(),
            );
        }
        mini_unref(api, caps);

        let audio_name = match audio_output {
            AudioOutput::System => c"autoaudiosink",
            #[cfg(test)]
            AudioOutput::Silent => c"fakesink",
        };
        // SAFETY: the factory returns a new element reference; playbin keeps its
        // own reference after the `audio-sink` property is set below.
        let audio = unsafe { (api.element_factory_make)(audio_name.as_ptr(), ptr::null()) };
        if audio.is_null() {
            unref(api, sink);
            unref(api, pipeline);
            return Err(EngineError::Unsupported);
        }
        // SAFETY: the factory-created audio sink also has a floating reference.
        unsafe { (api.object_ref_sink)(audio) };
        #[cfg(test)]
        if matches!(audio_output, AudioOutput::Silent) {
            // SAFETY: `fakesink` exposes `GstBaseSink::async` as a boolean property.
            unsafe {
                (api.object_set)(audio, c"async".as_ptr(), 0_i32, ptr::null::<c_char>());
            }
        }
        // SAFETY: `audio-sink` accepts a GstElement and this setter retains it.
        unsafe {
            (api.object_set)(
                pipeline,
                c"audio-sink".as_ptr(),
                audio,
                ptr::null::<c_char>(),
            );
        }
        // SAFETY: `pipeline` is a live playbin and returns an owned bus ref.
        let bus = unsafe { (api.element_get_bus)(pipeline) };
        if bus.is_null() {
            unref(api, audio);
            unref(api, sink);
            unref(api, pipeline);
            return Err(EngineError::Other("the GStreamer player has no bus"));
        }

        // Preroll while paused. `VideoSeat::open` sends Play immediately after
        // this worker is started, and the same pipeline then continues playing.
        // SAFETY: `pipeline` is a live GstElement and PAUSED is a GstState value.
        let state_change = unsafe { (api.element_set_state)(pipeline, GST_STATE_PAUSED) };
        if state_change == GST_STATE_CHANGE_FAILURE {
            unref(api, bus);
            unref(api, audio);
            unref(api, sink);
            unref(api, pipeline);
            return Err(EngineError::Decode);
        }

        Ok(Self {
            api,
            pipeline,
            sink,
            audio_sink: audio,
            bus,
            state: EngineState {
                volume: 1.0,
                rate: 1.0,
                ..EngineState::default()
            },
            generation: 0,
            preroll_taken: false,
            requested_playing: false,
            _worker_only: PhantomData,
        })
    }

    fn seek(&mut self, _ctx: &WorkerCtx, secs: f64, rate: f64) -> bool {
        if !secs.is_finite() || !rate.is_finite() || rate <= 0.0 {
            return false;
        }
        let secs = secs
            .max(0.0)
            .min(self.state.duration_secs.unwrap_or(f64::MAX));
        let Some(nanos) = (secs * GST_SECOND as f64)
            .is_finite()
            .then_some((secs * GST_SECOND as f64).round())
            .and_then(|nanos| {
                (0.0..=i64::MAX as f64)
                    .contains(&nanos)
                    .then_some(nanos as i64)
            })
        else {
            return false;
        };
        // SAFETY: the values are valid GstSeek arguments, expressed in time;
        // the positive rate and start offset were checked above.
        let accepted = unsafe {
            (self.api.element_seek)(
                self.pipeline,
                rate,
                GST_FORMAT_TIME,
                GST_SEEK_FLAG_FLUSH | GST_SEEK_FLAG_ACCURATE,
                GST_SEEK_TYPE_SET,
                nanos,
                GST_SEEK_TYPE_NONE,
                -1,
            )
        } != 0;
        if accepted {
            self.state.position_secs = secs;
            self.state.ended = false;
            self.state.rate = rate;
            self.preroll_taken = false;
        }
        accepted
    }

    fn play(&mut self, ctx: &WorkerCtx) {
        if self.state.ended {
            let _ = self.seek(ctx, 0.0, self.state.rate);
        }
        self.requested_playing = true;
        // SAFETY: this is the worker's pipeline and PLAYING is a GstState value.
        if unsafe { (self.api.element_set_state)(self.pipeline, GST_STATE_PLAYING) }
            == GST_STATE_CHANGE_FAILURE
        {
            self.state.error.get_or_insert(EngineError::Decode);
        }
    }

    fn pause(&mut self, _ctx: &WorkerCtx) {
        // SAFETY: this is the worker's pipeline and PAUSED is a GstState value.
        let result = unsafe { (self.api.element_set_state)(self.pipeline, GST_STATE_PAUSED) };
        self.requested_playing = false;
        if result == GST_STATE_CHANGE_FAILURE {
            self.state.error.get_or_insert(EngineError::Decode);
        } else {
            self.preroll_taken = false;
        }
    }

    fn set_muted(&mut self, _ctx: &WorkerCtx, muted: bool) {
        // SAFETY: playbin exposes the GstStreamVolume `mute` boolean property.
        unsafe {
            (self.api.object_set)(
                self.pipeline,
                c"mute".as_ptr(),
                i32::from(muted),
                ptr::null::<c_char>(),
            );
        }
    }

    fn set_volume(&mut self, _ctx: &WorkerCtx, volume: f64) {
        if !volume.is_finite() {
            return;
        }
        // SAFETY: playbin exposes the GstStreamVolume `volume` double property.
        unsafe {
            (self.api.object_set)(
                self.pipeline,
                c"volume".as_ptr(),
                volume.clamp(0.0, 1.0),
                ptr::null::<c_char>(),
            );
        }
    }

    fn refresh(&mut self, _ctx: &WorkerCtx) -> (c_int, c_int) {
        self.poll_bus();
        let mut current = GST_STATE_NULL;
        let mut pending = GST_STATE_VOID_PENDING;
        // SAFETY: both output pointers name writable GstState-sized integers.
        let _ =
            unsafe { (self.api.element_get_state)(self.pipeline, &mut current, &mut pending, 0) };
        self.state.playing = self.requested_playing
            && current == GST_STATE_PLAYING
            && pending != GST_STATE_PAUSED
            && !self.state.ended;
        let mut duration = -1_i64;
        // SAFETY: the query writes a time value to this live integer.
        if unsafe {
            (self.api.element_query_duration)(self.pipeline, GST_FORMAT_TIME, &mut duration)
        } != 0
            && duration >= 0
        {
            self.state.duration_secs = Some(duration as f64 / GST_SECOND as f64);
        }
        let mut position = -1_i64;
        // SAFETY: the query writes a time value to this live integer.
        if unsafe {
            (self.api.element_query_position)(self.pipeline, GST_FORMAT_TIME, &mut position)
        } != 0
            && position >= 0
        {
            self.state.position_secs = position as f64 / GST_SECOND as f64;
        }

        let (mut video_count, mut audio_count) = (0_i32, 0_i32);
        // SAFETY: playbin defines both track-count properties as gint values.
        unsafe {
            (self.api.object_get)(
                self.pipeline,
                c"n-video".as_ptr(),
                &mut video_count,
                c"n-audio".as_ptr(),
                &mut audio_count,
                ptr::null::<c_char>(),
            );
        }
        self.state.has_video = video_count > 0 || self.state.natural_size.is_some();
        self.state.has_audio = audio_count > 0;

        let (mut muted, mut volume) = (0_i32, 1.0_f64);
        // SAFETY: GstStreamVolume defines these two writable output properties.
        unsafe {
            (self.api.object_get)(
                self.pipeline,
                c"mute".as_ptr(),
                &mut muted,
                c"volume".as_ptr(),
                &mut volume,
                ptr::null::<c_char>(),
            );
        }
        self.state.muted = muted != 0;
        if volume.is_finite() {
            self.state.volume = volume.clamp(0.0, 1.0);
        }
        if self.state.ended {
            self.state.playing = false;
            if let Some(duration) = self.state.duration_secs {
                self.state.position_secs = duration;
            }
        }
        (current, pending)
    }

    // The pipeline can settle before a non-async appsink produces its first
    // frame, so video readiness also waits for the negotiated dimensions.
    fn publish_ready_if_metadata_arrived(&mut self, current: c_int, pending: c_int) {
        if self.state.ready
            || self.state.error.is_some()
            || current < GST_STATE_PAUSED
            || pending != GST_STATE_VOID_PENDING
        {
            return;
        }
        let metadata_arrived = if self.state.has_video {
            self.state.natural_size.is_some()
        } else {
            self.state.has_audio
        };
        self.state.ready = metadata_arrived;
    }

    fn poll_bus(&mut self) {
        loop {
            // SAFETY: gst_bus_pop is nonblocking and returns one owned message.
            let message = unsafe { (self.api.bus_pop)(self.bus) };
            if message.is_null() {
                break;
            }
            // SAFETY: the public GstMessage prefix is repr(C); its type field
            // follows the public GstMiniObject header in GStreamer 1.x.
            let message_type = unsafe { (*message.cast::<GstMessagePrefix>()).message_type };
            if message_type == GST_MESSAGE_ERROR {
                let mut error = ptr::null_mut();
                let mut debug = ptr::null_mut();
                // SAFETY: this live message is ERROR and both outputs are writable.
                unsafe { (self.api.message_parse_error)(message, &mut error, &mut debug) };
                let engine_error = decode_error(self.api, error);
                if !debug.is_null() {
                    // SAFETY: parse_error allocated this debug string with GLib.
                    unsafe { (self.api.free)(debug.cast()) };
                }
                self.state.error.get_or_insert(engine_error);
            } else if message_type == GST_MESSAGE_EOS {
                self.state.ended = true;
                self.state.playing = false;
                self.requested_playing = false;
            }
            mini_unref(self.api, message);
        }
    }

    fn take_frame(&mut self, _ctx: &WorkerCtx) -> Result<Option<(Frame, FrameCost)>, EngineError> {
        let started = Instant::now();
        let mut current = GST_STATE_NULL;
        let mut pending = GST_STATE_VOID_PENDING;
        // SAFETY: the state query writes two GstState values.
        let _ =
            unsafe { (self.api.element_get_state)(self.pipeline, &mut current, &mut pending, 0) };
        let raw_sample = if pending == GST_STATE_PAUSED && !self.preroll_taken {
            // A sink needs the pending PAUSED transition's preroll sample
            // before the pipeline can report that it has come to rest.
            // SAFETY: this is a nonblocking appsink preroll pull.
            unsafe { (self.api.app_sink_try_pull_preroll)(self.sink, 0) }
        } else if current >= GST_STATE_PLAYING {
            // SAFETY: this is a nonblocking appsink sample pull.
            unsafe { (self.api.app_sink_try_pull_sample)(self.sink, 0) }
        } else if !self.preroll_taken {
            // SAFETY: this is a nonblocking appsink preroll pull.
            unsafe { (self.api.app_sink_try_pull_preroll)(self.sink, 0) }
        } else {
            ptr::null_mut()
        };
        let transfer = started.elapsed();
        if raw_sample.is_null() {
            return Ok(None);
        }
        if current < GST_STATE_PLAYING || pending == GST_STATE_PAUSED {
            self.preroll_taken = true;
        }

        let caps = unsafe { (self.api.sample_get_caps)(raw_sample) };
        let buffer = unsafe { (self.api.sample_get_buffer)(raw_sample) };
        if caps.is_null() || buffer.is_null() {
            mini_unref(self.api, raw_sample);
            return Err(EngineError::Decode);
        }
        // SAFETY: a GstCaps contains the negotiated raw-video structure at 0.
        let structure = unsafe { (self.api.caps_get_structure)(caps, 0) };
        if structure.is_null() {
            mini_unref(self.api, raw_sample);
            return Err(EngineError::Decode);
        }
        let (mut width, mut height) = (0_i32, 0_i32);
        // SAFETY: GStreamer writes the negotiated width and height into these
        // gint outputs for the named raw-video fields.
        let dimensions_ok = unsafe {
            (self.api.structure_get_int)(structure, c"width".as_ptr(), &mut width) != 0
                && (self.api.structure_get_int)(structure, c"height".as_ptr(), &mut height) != 0
        };
        if !dimensions_ok || width <= 0 || height <= 0 {
            mini_unref(self.api, raw_sample);
            return Err(EngineError::Decode);
        }
        let Some(expected) = usize::try_from(width)
            .ok()
            .and_then(|width| {
                usize::try_from(height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .and_then(|pixels| pixels.checked_mul(4))
        else {
            mini_unref(self.api, raw_sample);
            return Err(EngineError::Decode);
        };
        // BGRA's four-byte pixel size makes GStreamer's default four-byte row
        // alignment equal to width * 4. Refuse a different layout rather than
        // guessing at padding that the negotiated caps did not describe.
        let buffer_size = unsafe { (self.api.buffer_get_size)(buffer) };
        if buffer_size != expected {
            mini_unref(self.api, raw_sample);
            return Err(EngineError::Decode);
        }
        let mut bgra = Vec::new();
        if bgra.try_reserve_exact(expected).is_err() {
            mini_unref(self.api, raw_sample);
            return Err(EngineError::Decode);
        }
        bgra.resize(expected, 0);
        let copy_started = Instant::now();
        // SAFETY: `bgra` has `expected` writable bytes and the buffer was
        // checked to contain exactly that many bytes.
        let copied =
            unsafe { (self.api.buffer_extract)(buffer, 0, bgra.as_mut_ptr().cast(), bgra.len()) };
        mini_unref(self.api, raw_sample);
        if copied != expected {
            return Err(EngineError::Decode);
        }
        let copy = copy_started.elapsed();
        let width = width as u32;
        let height = height as u32;
        self.state.natural_size = Some((width, height));
        self.state.has_video = true;
        self.generation = self.generation.wrapping_add(1).max(1);
        Ok(Some((
            Frame {
                bgra: Arc::from(bgra.into_boxed_slice()),
                width,
                height,
                generation: self.generation,
            },
            FrameCost {
                transfer,
                readback: Duration::ZERO,
                copy,
                frames: 1,
            },
        )))
    }

    fn stop(&mut self, _ctx: &WorkerCtx) {
        // The pipeline is still owned here. The first state wait is bounded by
        // SHUTDOWN_BUDGET; a decoder that takes longer remains on this worker
        // and is polled until the state is actually NULL before it is released.
        // SAFETY: setting NULL is the documented way to stop a GstElement.
        let _ = unsafe { (self.api.element_set_state)(self.pipeline, GST_STATE_NULL) };
        // SAFETY: this asks GStreamer to wait at most the existing two-second
        // engine shutdown budget for the pipeline's asynchronous state change.
        let result = unsafe {
            (self.api.element_get_state)(
                self.pipeline,
                ptr::null_mut(),
                ptr::null_mut(),
                duration_nanos(SHUTDOWN_BUDGET),
            )
        };
        if result == GST_STATE_CHANGE_ASYNC {
            eprintln!(
                "video: GStreamer retirement is still pending after {SHUTDOWN_BUDGET:?}; \
                 the player worker will keep the pipeline until NULL"
            );
        }
        loop {
            let (mut current, mut pending) = (GST_STATE_NULL, GST_STATE_VOID_PENDING);
            // SAFETY: a zero-time state query is nonblocking and writes its two
            // state outputs.
            let result = unsafe {
                (self.api.element_get_state)(self.pipeline, &mut current, &mut pending, 0)
            };
            if result != GST_STATE_CHANGE_ASYNC
                && current == GST_STATE_NULL
                && pending == GST_STATE_VOID_PENDING
            {
                break;
            }
            std::thread::sleep(FRAME_POLL_INTERVAL);
        }
    }
}

impl Drop for GstPlayer {
    fn drop(&mut self) {
        // SAFETY: this value never leaves its WorkerCtx thread, and NULL is
        // idempotent if `stop` already completed the transition.
        let _ = unsafe { (self.api.element_set_state)(self.pipeline, GST_STATE_NULL) };
        unref(self.api, self.bus);
        unref(self.api, self.pipeline);
        unref(self.api, self.sink);
        unref(self.api, self.audio_sink);
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX - 1)
}

/// **One player, on a worker that owns its complete native lifetime.** The
/// handle publishes snapshots; the GStreamer pipeline never leaves `run`.
pub struct Engine {
    commands: mpsc::Sender<Command>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    source: PathBuf,
    seen_generation: u64,
    opened_at: Instant,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Engine")
            .field("source", &self.source)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// Open a local source and start its GStreamer pipeline on a Folio worker.
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        Self::open_with_output(path, AudioOutput::System)
    }

    fn open_with_output(path: &Path, audio_output: AudioOutput) -> Result<Self, EngineError> {
        let shared = Arc::new(Shared::default());
        let (commands, inbox) = mpsc::channel();
        let source = path.to_path_buf();
        let worker_path = source.clone();
        let worker_shared = Arc::clone(&shared);
        let thread = crate::spawn_at_priority(ENGINE_WORKER, ThreadPriority::Normal, move |ctx| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_engine(ctx, &worker_path, audio_output, &worker_shared, &inbox);
            }));
            if result.is_err() {
                publish_failure(
                    &worker_shared,
                    EngineError::Other("the video worker failed"),
                );
            }
            worker_shared.stopped.store(true, Ordering::Release);
        })
        .map_err(|_| EngineError::Other("no thread for the video engine"))?;
        Ok(Self {
            commands,
            shared,
            thread: Some(thread),
            source,
            seen_generation: 0,
            opened_at: Instant::now(),
        })
    }

    /// The path this engine was opened on, in the spelling the caller supplied.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// State last published by the player worker.
    #[must_use]
    pub fn state(&self) -> EngineState {
        let state = *self
            .shared
            .state
            .lock()
            .unwrap_or_else(|held| held.into_inner());
        if state.error.is_none()
            && !self.shared.built.load(Ordering::Acquire)
            && self.opened_at.elapsed() > OPEN_BUDGET
        {
            return EngineState {
                error: Some(EngineError::Other("the video engine did not start")),
                ..state
            };
        }
        state
    }

    /// Take the newest decoded frame if its generation is newer than this
    /// handle's last returned frame.
    pub fn frame(&mut self) -> Option<Frame> {
        if self.shared.generation.load(Ordering::Acquire) <= self.seen_generation {
            return None;
        }
        let frame = self
            .shared
            .frame
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()?;
        if frame.generation <= self.seen_generation {
            return None;
        }
        self.seen_generation = frame.generation;
        Some(frame)
    }

    /// The most recent frame whether or not this handle has already returned it.
    #[must_use]
    pub fn standing_frame(&self) -> Option<Frame> {
        self.shared
            .frame
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .clone()
    }

    /// Cost of the last frame transferred to Folio.
    #[must_use]
    pub fn frame_cost(&self) -> FrameCost {
        *self
            .shared
            .cost
            .lock()
            .unwrap_or_else(|held| held.into_inner())
    }

    pub fn play(&self) {
        let _ = self.commands.send(Command::Play);
    }

    pub fn pause(&self) {
        let _ = self.commands.send(Command::Pause);
    }

    #[cfg(test)]
    fn pause_until_gstreamer_settles(&self) -> bool {
        let (acknowledge, states) = mpsc::channel();
        assert!(
            self.commands
                .send(Command::PauseAndAck(acknowledge))
                .is_ok(),
            "GStreamer worker accepts a pause request"
        );
        loop {
            let state = states.recv().expect("GStreamer worker reports its state");
            if state.0 == GST_STATE_PAUSED && state.1 == GST_STATE_VOID_PENDING {
                return true;
            }
            if state.2 {
                return false;
            }
        }
    }

    /// Seek to a time in seconds. The pipeline clamps to the declared duration.
    pub fn seek(&self, secs: f64) {
        let _ = self.commands.send(Command::Seek(secs));
    }

    pub fn set_rate(&self, rate: f64) {
        let _ = self.commands.send(Command::Rate(rate));
    }

    pub fn set_muted(&self, muted: bool) {
        let _ = self.commands.send(Command::Muted(muted));
    }

    pub fn set_volume(&self, volume: f64) {
        let _ = self.commands.send(Command::Volume(volume));
    }

    /// Wait for metadata. This is intended for worker callers and tests; the
    /// window reads [`Self::state`] while its normal event loop remains active.
    #[cfg(test)]
    pub fn wait_for_metadata(&self, budget: Duration) -> bool {
        let deadline = Instant::now() + budget;
        loop {
            let state = self.state();
            if state.ready || state.error.is_some() {
                return state.ready;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(FRAME_POLL_INTERVAL);
        }
    }

    /// Ask the worker to stop, and wait for actual pipeline release only on an
    /// admitted window-thread shutdown. Other callers let the player retire on
    /// its owner worker and can observe that completion through the engine ledger.
    pub fn shutdown(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        let Some(thread) = self.thread.take() else {
            return;
        };
        if admission::role() == admission::Role::Window {
            let shared = Arc::clone(&self.shared);
            let _ = admission::admitted::<doors::VideoShutdown, _>(|token| {
                wait_for_shutdown(token, &shared, thread);
            });
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn wait_for_shutdown(
    _token: WaitToken<'_, doors::VideoShutdown>,
    shared: &Shared,
    thread: JoinHandle<()>,
) {
    let deadline = Instant::now() + SHUTDOWN_BUDGET;
    while !shared.stopped.load(Ordering::Acquire) {
        if Instant::now() >= deadline {
            eprintln!(
                "video: GStreamer had not released its pipeline within {SHUTDOWN_BUDGET:?}; \
                 leaving retirement on the player worker"
            );
            return;
        }
        std::thread::sleep(FRAME_POLL_INTERVAL);
    }
    let _ = thread.join();
}

#[derive(Default)]
struct Shared {
    state: Mutex<EngineState>,
    frame: Mutex<Option<Frame>>,
    cost: Mutex<FrameCost>,
    generation: AtomicU64,
    built: AtomicBool,
    stopped: AtomicBool,
}

enum Command {
    Play,
    Pause,
    #[cfg(test)]
    PauseAndAck(mpsc::Sender<(c_int, c_int, bool)>),
    Seek(f64),
    Rate(f64),
    Muted(bool),
    Volume(f64),
    Shutdown,
}

fn run_engine(
    ctx: &WorkerCtx,
    path: &Path,
    audio_output: AudioOutput,
    shared: &Arc<Shared>,
    inbox: &mpsc::Receiver<Command>,
) {
    let mut player = match GstPlayer::open(ctx, path, audio_output) {
        Ok(player) => player,
        Err(error) => {
            publish_failure(shared, error);
            return;
        }
    };
    let ledger = LedgerEntry::opened();
    ledger.kept();
    shared.built.store(true, Ordering::Release);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pump_engine(ctx, &mut player, shared, inbox);
    }));
    if result.is_err() {
        publish_failure(shared, EngineError::Other("the video worker failed"));
    }
    player.stop(ctx);
    drop(player);
    note_engine_shut_down();
}

fn pump_engine(
    ctx: &WorkerCtx,
    player: &mut GstPlayer,
    shared: &Arc<Shared>,
    inbox: &mpsc::Receiver<Command>,
) {
    let mut stopping = false;
    #[cfg(test)]
    let mut pause_ack = None;
    while !stopping {
        let timeout = if player.state.playing {
            FRAME_POLL_INTERVAL
        } else {
            IDLE_POLL_INTERVAL
        };
        match inbox.recv_timeout(timeout) {
            Ok(Command::Play) => player.play(ctx),
            Ok(Command::Pause) => player.pause(ctx),
            #[cfg(test)]
            Ok(Command::PauseAndAck(acknowledge)) => {
                player.pause(ctx);
                pause_ack = Some(acknowledge);
            }
            Ok(Command::Seek(secs)) => {
                let _ = player.seek(ctx, secs, player.state.rate);
            }
            Ok(Command::Rate(rate)) => {
                let position = player.state.position_secs;
                if rate.is_finite() && rate > 0.0 && player.seek(ctx, position, rate) {
                    player.state.rate = rate;
                }
            }
            Ok(Command::Muted(muted)) => player.set_muted(ctx, muted),
            Ok(Command::Volume(volume)) => player.set_volume(ctx, volume),
            Ok(Command::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => stopping = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        let (current, pending) = player.refresh(ctx);
        match player.take_frame(ctx) {
            Ok(Some((frame, cost))) => {
                shared.generation.store(frame.generation, Ordering::Release);
                *shared.frame.lock().unwrap_or_else(|held| held.into_inner()) = Some(frame);
                *shared.cost.lock().unwrap_or_else(|held| held.into_inner()) = cost;
            }
            Ok(None) => {}
            Err(error) => {
                player.state.error.get_or_insert(error);
            }
        }
        player.publish_ready_if_metadata_arrived(current, pending);
        *shared.state.lock().unwrap_or_else(|held| held.into_inner()) = player.state;
        #[cfg(test)]
        if let Some(acknowledge) = pause_ack.take() {
            let error = player.state.error.is_some();
            let settled = current == GST_STATE_PAUSED && pending == GST_STATE_VOID_PENDING;
            let keep_waiting =
                acknowledge.send((current, pending, error)).is_ok() && !settled && !error;
            if keep_waiting {
                pause_ack = Some(acknowledge);
            }
        }
        if player.state.error.is_some() {
            stopping = true;
        }
    }
}

fn publish_failure(shared: &Shared, error: EngineError) {
    let mut state = shared.state.lock().unwrap_or_else(|held| held.into_inner());
    state.error.get_or_insert(error);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::engine::engines_started;

    const PATIENCE: Duration = Duration::from_secs(10);

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/assets")
            .join(name)
    }

    fn settle(engine: &Engine, condition: impl Fn(EngineState) -> bool) -> EngineState {
        let deadline = Instant::now() + PATIENCE;
        loop {
            let state = engine.state();
            if condition(state) || Instant::now() >= deadline {
                return state;
            }
            std::thread::sleep(FRAME_POLL_INTERVAL);
        }
    }

    fn wait_for_frame(engine: &mut Engine, after: u64) -> Option<Frame> {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Some(frame) = engine.frame()
                && frame.generation > after
            {
                return Some(frame);
            }
            std::thread::sleep(FRAME_POLL_INTERVAL);
        }
        None
    }

    fn wait_for_retirement(shared: &Shared) {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline && !shared.stopped.load(Ordering::Acquire) {
            std::thread::sleep(FRAME_POLL_INTERVAL);
        }
        // The worker publishes this after run_engine returns; its cleanup path
        // releases the player and retires its ledger entry before returning.
        // The process-wide outstanding count can move for sibling tests.
        assert!(
            shared.stopped.load(Ordering::Acquire),
            "this GStreamer worker did not finish its owned cleanup"
        );
    }

    /// A real sound-and-picture fixture exercises the installed decoders,
    /// paused preroll, video-frame publication, rate, volume, mute, seek, EOS,
    /// and worker-owned release without opening an audio device.
    #[test]
    fn gstreamer_embedded_playback_controls_and_retirement_follow_the_pipeline() {
        let started_before = engines_started();
        let mut engine =
            Engine::open_with_output(&fixture("folio-video-sound-test.mp4"), AudioOutput::Silent)
                .expect("the Folio engine worker starts");
        engine.set_muted(true);
        engine.set_volume(0.25);
        let opened = settle(&engine, |state| {
            state.ready && state.muted && (state.volume - 0.25).abs() < 0.02
        });
        assert!(opened.ready, "the pipeline prerolls: {opened:?}");
        assert_eq!(opened.natural_size, Some((160, 120)), "{opened:?}");
        assert!(opened.has_video, "{opened:?}");
        assert!(opened.has_audio, "{opened:?}");
        assert!((opened.duration_secs.unwrap_or_default() - 3.0).abs() < 0.1);
        assert!(engines_started() > started_before);

        engine.set_rate(1.5);
        let rated = settle(&engine, |state| (state.rate - 1.5).abs() < 0.01);
        assert!((rated.rate - 1.5).abs() < 0.01, "{rated:?}");
        assert!(!rated.playing, "changing rate while paused did not play");

        engine.play();
        let playing = settle(&engine, |state| state.playing && state.position_secs > 0.1);
        assert!(playing.playing, "{playing:?}");
        let first = engine
            .standing_frame()
            .expect("the appsink publishes a frame into Folio");
        assert_eq!((first.width, first.height), (160, 120));
        let delivered = wait_for_frame(&mut engine, first.generation)
            .expect("playing publishes a newer embedded frame");
        assert!(delivered.generation > first.generation);

        assert!(
            engine.pause_until_gstreamer_settles(),
            "GStreamer reports the pipeline settled at PAUSED"
        );
        let paused = engine.state();
        assert!(!paused.playing, "{paused:?}");

        let previous_generation = engine
            .standing_frame()
            .expect("the last decoded frame remains available")
            .generation;
        engine.seek(1.5);
        let sought = settle(&engine, |state| state.position_secs >= 1.49);
        assert!((sought.position_secs - 1.5).abs() < 0.05, "{sought:?}");
        let after_seek = wait_for_frame(&mut engine, previous_generation)
            .expect("the seek replaces the standing frame");
        assert!(after_seek.generation > previous_generation);

        engine.seek(2.6);
        let near_end = settle(&engine, |state| state.position_secs >= 2.59);
        assert!(near_end.position_secs >= 2.59, "{near_end:?}");
        engine.play();
        let ended = settle(&engine, |state| state.ended);
        assert!(ended.ended, "the EOS bus message reaches state: {ended:?}");
        assert!(!ended.playing);

        engine.shutdown();
        wait_for_retirement(&engine.shared);

        let bad_path =
            std::env::temp_dir().join(format!("folio-video-invalid-{}.mp4", std::process::id()));
        std::fs::write(&bad_path, b"not a media container").expect("write a local invalid fixture");
        let mut failed = Engine::open_with_output(&bad_path, AudioOutput::Silent)
            .expect("failure is reported through the engine state");
        let failure = settle(&failed, |state| state.error.is_some());
        assert!(
            failure.error.is_some(),
            "a bad container is an engine error: {failure:?}"
        );
        failed.shutdown();
        wait_for_retirement(&failed.shared);
        let _ = std::fs::remove_file(bad_path);

        let dropped =
            Engine::open_with_output(&fixture("folio-video-sound-test.mp4"), AudioOutput::Silent)
                .expect("start a player for the Drop path");
        dropped.set_muted(true);
        let ready = settle(&dropped, |state| state.ready && state.muted);
        assert!(ready.ready, "{ready:?}");
        dropped.play();
        let playing = settle(&dropped, |state| state.playing && state.position_secs > 0.1);
        assert!(playing.playing, "{playing:?}");
        let retirement = Arc::clone(&dropped.shared);
        drop(dropped);
        wait_for_retirement(&retirement);
    }

    /// A longer audio stream must keep playbin alive after its video appsink has
    /// ended. MPEG-TS does not declare a duration here, so only pipeline EOS can
    /// finish this source.
    #[test]
    fn pipeline_eos_waits_for_audio_after_video_eos_without_declared_duration() {
        let path = fixture("folio-video-unequal-streams.ts");
        let mut engine =
            Engine::open_with_output(&path, AudioOutput::Silent).expect("start the player worker");
        engine.set_muted(true);
        let ready = settle(&engine, |state| {
            state.ready && state.has_video && state.has_audio
        });
        assert!(ready.ready, "{ready:?}");
        assert!(
            ready.duration_secs.is_none(),
            "TS duration is undeclared: {ready:?}"
        );

        engine.play();
        let after_video = settle(&engine, |state| state.position_secs >= 1.2);
        assert!(after_video.position_secs >= 1.2, "{after_video:?}");
        assert!(
            !after_video.ended,
            "audio still has time to play: {after_video:?}"
        );
        assert!(after_video.playing, "audio keeps the media clock running");

        let ended = settle(&engine, |state| state.ended);
        assert!(
            ended.ended,
            "the top-level EOS message reaches state: {ended:?}"
        );
        engine.shutdown();
        wait_for_retirement(&engine.shared);
    }
}
