use std::path::PathBuf;
use std::sync::Mutex;

use e830::runner::{validate_nand_image, FirmwarePaths, Worker, WorkerEvent};
use jni::objects::{JObject, JString};
use jni::sys::{jboolean, jbyteArray, jlong, jstring};
use jni::JNIEnv;

struct Session {
    worker: Worker,
    frame: Option<Vec<u8>>,
    status: String,
    finished: bool,
}

fn path(value: String) -> PathBuf {
    PathBuf::from(value)
}

fn firmware_paths(
    cla: String,
    eeprom: String,
    tfs: String,
    onenand: String,
) -> Result<FirmwarePaths, String> {
    if cla.is_empty() || eeprom.is_empty() {
        return Err("Select both the .cla firmware and matching .nsf EEPROM.".into());
    }
    Ok(FirmwarePaths {
        cla: path(cla),
        eeprom: path(eeprom),
        tfs: (!tfs.is_empty()).then(|| path(tfs)),
        onenand: (!onenand.is_empty()).then(|| path(onenand)),
    })
}

fn read_string(env: &mut JNIEnv<'_>, value: &JString<'_>) -> Result<String, String> {
    env.get_string(value)
        .map(|value| value.into())
        .map_err(|error| error.to_string())
}

fn new_string(env: &mut JNIEnv<'_>, value: &str) -> jstring {
    env.new_string(value)
        .map(|value| value.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

fn session(handle: jlong) -> Option<&'static Mutex<Session>> {
    if handle == 0 {
        return None;
    }
    Some(unsafe { &*(handle as *const Mutex<Session>) })
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativeValidate<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    cla: JString<'local>,
    eeprom: JString<'local>,
    tfs: JString<'local>,
    onenand: JString<'local>,
) -> jstring {
    let result = (|| {
        let paths = firmware_paths(
            read_string(&mut env, &cla)?,
            read_string(&mut env, &eeprom)?,
            read_string(&mut env, &tfs)?,
            read_string(&mut env, &onenand)?,
        )?;
        if let Some(nand) = paths.onenand_path() {
            validate_nand_image(&nand)?;
        }
        paths.load().map(|_| ()).map_err(|error| error.to_string())
    })();
    new_string(&mut env, &result.err().unwrap_or_default())
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativeStart<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    cla: JString<'local>,
    eeprom: JString<'local>,
    tfs: JString<'local>,
    onenand: JString<'local>,
) -> jlong {
    let result = (|| {
        let paths = firmware_paths(
            read_string(&mut env, &cla)?,
            read_string(&mut env, &eeprom)?,
            read_string(&mut env, &tfs)?,
            read_string(&mut env, &onenand)?,
        )?;
        let worker = Worker::start(paths).map_err(|error| error.to_string())?;
        Ok::<_, String>(Box::into_raw(Box::new(Mutex::new(Session {
            worker,
            frame: None,
            status: "Starting Samsung SGH-E830 firmware…".into(),
            finished: false,
        }))) as jlong)
    })();
    match result {
        Ok(handle) => handle,
        Err(error) => {
            let _ = env.throw_new("java/lang/IllegalStateException", error);
            0
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativePollStatus<'local>(
    mut env: JNIEnv<'local>,
    _this: JObject<'local>,
    handle: jlong,
) -> jstring {
    let Some(session) = session(handle) else {
        return new_string(&mut env, "Emulator is not running.");
    };
    let mut session = session.lock().unwrap_or_else(|error| error.into_inner());
    loop {
        match session.worker.events.try_recv() {
            Ok(WorkerEvent::Frame(frame)) => {
                session.frame = Some(frame);
                if !session.finished {
                    session.status = "Running. The E830 menu and keypad are not emulated.".into();
                }
            }
            Ok(WorkerEvent::InputApplied(_)) => {}
            Ok(WorkerEvent::Finished(status)) => {
                session.status = status;
                session.finished = true;
                break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if !session.finished {
                    session.status = "Emulator worker stopped unexpectedly.".into();
                    session.finished = true;
                }
                break;
            }
        }
    }
    new_string(&mut env, &session.status)
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativeTakeFrame<'local>(
    env: JNIEnv<'local>,
    _this: JObject<'local>,
    handle: jlong,
) -> jbyteArray {
    let Some(session) = session(handle) else {
        return std::ptr::null_mut();
    };
    let mut session = session.lock().unwrap_or_else(|error| error.into_inner());
    let Some(frame) = session.frame.take() else {
        return std::ptr::null_mut();
    };
    env.byte_array_from_slice(&frame)
        .map(|array| array.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativeIsRunning(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) -> jboolean {
    session(handle)
        .and_then(|session| session.lock().ok().map(|session| !session.finished))
        .unwrap_or(false) as jboolean
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativeStop(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if let Some(session) = session(handle) {
        let mut session = session.lock().unwrap_or_else(|error| error.into_inner());
        session.worker.stop();
        session.finished = true;
        if session.status.starts_with("Running") || session.status.starts_with("Starting") {
            session.status = "Stopped.".into();
        }
    }
}

#[no_mangle]
pub extern "system" fn Java_com_samsung_e830_NativeBridge_nativeDestroy(
    _env: JNIEnv<'_>,
    _this: JObject<'_>,
    handle: jlong,
) {
    if handle != 0 {
        unsafe {
            drop(Box::from_raw(handle as *mut Mutex<Session>));
        }
    }
}
