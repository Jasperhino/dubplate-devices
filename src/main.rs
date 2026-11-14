mod device;
mod midi;
mod protocol;

use device::Device;
use std::{
    ffi::{c_char, CString},
    fs::{File, OpenOptions},
    io::Write,
    os::unix::{fs::OpenOptionsExt, io::AsRawFd},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        OnceLock,
    },
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

extern "C" {
    fn db_ui_run(callback: extern "C" fn(i32));
    fn db_ui_update(mode: i32, text: *const c_char, working: i32);
    fn db_ui_exit();
    fn db_accessibility() -> i32;
    fn db_usb_present(mode: i32) -> i32;
    fn db_usb_conflict() -> i32;
    fn db_jig_switch(
        mode: i32,
        profile: *const c_char,
        attach: i32,
        error: *mut c_char,
        capacity: usize,
    ) -> i32;
}
static COMMANDS: OnceLock<Sender<i32>> = OnceLock::new();
static TERMINATE: AtomicBool = AtomicBool::new(false);
extern "C" fn command(id: i32) {
    if let Some(tx) = COMMANDS.get() {
        let _ = tx.send(id);
    }
}
extern "C" fn signal(_: i32) {
    TERMINATE.store(true, Ordering::Relaxed);
}
fn ui(mode: Device, message: &str, busy: bool) {
    let s = CString::new(message.replace('\0', "")).unwrap();
    unsafe {
        db_ui_update(mode as i32, s.as_ptr(), busy.into());
    }
}
fn resources() -> PathBuf {
    let executable = std::env::current_exe().expect("executable location");
    let bundle = executable.parent().unwrap().join("../Resources");
    if bundle.join("xz.jigbp").is_file() {
        bundle
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources")
    }
}
fn switch_usb(device: Device, attach: bool) -> Result<(), String> {
    let path = CString::new(
        resources()
            .join(device.profile())
            .to_string_lossy()
            .as_bytes(),
    )
    .unwrap();
    let mut error = [0i8; 1024];
    let ok = unsafe {
        db_jig_switch(
            device as i32,
            path.as_ptr(),
            attach.into(),
            error.as_mut_ptr(),
            error.len(),
        )
    };
    if ok == 1 {
        Ok(())
    } else {
        Err(unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }
            .to_string_lossy()
            .into_owned())
    }
}
fn cleanup_usb() -> Result<(), String> {
    for device in [Device::Grv6, Device::Xz] {
        match unsafe { db_usb_present(device as i32) } {
            0 => (),
            -1 => return Err("Cannot check USB state; cleanup is unconfirmed.".into()),
            _ => switch_usb(device, false)?,
        }
    }
    Ok(())
}
fn log(file: &mut File, message: &str) {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let _ = writeln!(file, "{time} {message}");
}
fn worker(rx: Receiver<i32>, mut logfile: File) {
    let mut selected = Device::Off;
    let mut midi = None::<midi::Midi>;
    let mut last_status = String::new();
    let mut checked = Instant::now();
    let mut error = cleanup_usb().err();
    if let Some(e) = &error {
        log(&mut logfile, e);
        ui(Device::Off, &format!("USB cleanup needed: {e}"), false);
    } else {
        ui(Device::Off, "Off — no emulated device", false);
    }
    loop {
        let action = if TERMINATE.load(Ordering::Relaxed) {
            Some(9)
        } else {
            rx.recv_timeout(Duration::from_millis(20)).ok()
        };
        if let Some(action) = action {
            if action == 8 {
                log(&mut logfile, "selector opened");
                continue;
            }
            let quitting = action == 9;
            let Some(next) = (if quitting {
                Some(Device::Off)
            } else {
                Device::from_id(action)
            }) else {
                continue;
            };
            if next == selected && error.is_none() && !quitting {
                continue;
            }
            ui(
                selected,
                if quitting {
                    "Stopping…"
                } else {
                    "Switching…"
                },
                true,
            );
            // Dropping CoreMIDI before changing USB prevents two active controllers.
            drop(midi.take());
            let result = cleanup_usb();
            if let Err(e) = result {
                log(&mut logfile, &format!("cleanup failed: {e}"));
                ui(
                    selected,
                    &format!("MIDI off; USB cleanup needed: {e}"),
                    false,
                );
                error = Some(e);
                if TERMINATE.load(Ordering::Relaxed) {
                    unsafe {
                        db_ui_exit();
                    }
                    return;
                }
                continue;
            }
            selected = Device::Off;
            error = None;
            if quitting {
                log(&mut logfile, "quit; MIDI and owned USB devices removed");
                unsafe {
                    db_ui_exit();
                }
                return;
            }
            if next != Device::Off {
                let start = (|| {
                    if unsafe { db_usb_conflict() } != 0 {
                        return Err("Another supported controller is connected. Detach it before starting this emulator.".into());
                    }
                    switch_usb(next, true)?;
                    // Enumeration is confirmed before opening the named endpoints.
                    thread::sleep(Duration::from_millis(350));
                    midi = Some(midi::Midi::open(next)?);
                    Ok::<(), String>(())
                })();
                if let Err(mut e) = start {
                    if let Err(cleanup) = cleanup_usb() {
                        e.push_str(&format!(" USB cleanup needed: {cleanup}"));
                    }
                    log(&mut logfile, &format!("start {} failed: {e}", next.name()));
                    ui(Device::Off, &e, false);
                    error = Some(e);
                    continue;
                }
                selected = next;
            }
            last_status.clear();
            log(&mut logfile, &format!("selected {}", selected.name()));
            if selected == Device::Off {
                ui(selected, "Off — no emulated device", false);
            }
        }
        if let Some(session) = midi.as_mut() {
            let mut failure = session.tick().err();
            if checked.elapsed() >= Duration::from_secs(1) {
                checked = Instant::now();
                if unsafe { db_usb_present(selected as i32) } != 1 {
                    failure = Some(
                        "USB device disconnected; MIDI stopped. Select the device to reconnect."
                            .into(),
                    );
                }
            }
            if let Some(mut e) = failure {
                drop(midi.take());
                if let Err(cleanup) = cleanup_usb() {
                    e.push_str(&format!(" USB cleanup needed: {cleanup}"));
                }
                selected = Device::Off;
                log(&mut logfile, &e);
                ui(selected, &e, false);
                error = Some(e);
                continue;
            }
            let status = if session.handshake {
                "XZ handshake complete".to_owned()
            } else if session.packets > 0 {
                if selected == Device::Xz {
                    "MIDI received — awaiting XZ handshake".into()
                } else {
                    "USB + MIDI connected — messages received".into()
                }
            } else {
                "USB + MIDI connected — waiting for rekordbox".into()
            };
            if status != last_status {
                log(&mut logfile, &status);
                ui(selected, &status, false);
                last_status = status;
            }
        }
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::args().any(|a| a == "--diagnose") {
        println!("Accessibility: {}\nOwned GRV6 USB: {}\nOwned XZ USB: {}\nOther controller: {}\nProfiles: {}",
            unsafe { db_accessibility() }, unsafe { db_usb_present(1) }, unsafe { db_usb_present(2) },
            unsafe { db_usb_conflict() }, resources().display());
        return Ok(());
    }
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("HOME is missing")?);
    let support = home.join("Library/Application Support/Dubplate Devices");
    std::fs::create_dir_all(&support)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(support.join("instance.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        eprintln!("Dubplate Devices is already running.");
        return Ok(());
    }
    let logdir = home.join("Library/Logs");
    std::fs::create_dir_all(&logdir)?;
    let path = logdir.join("Dubplate Devices.log");
    if path
        .metadata()
        .map(|m| m.len() > 2_000_000)
        .unwrap_or(false)
    {
        let _ = std::fs::rename(&path, logdir.join("Dubplate Devices.previous.log"));
    }
    let logfile = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(path)?;
    let (tx, rx) = mpsc::channel();
    COMMANDS.set(tx).unwrap();
    unsafe {
        libc::signal(libc::SIGTERM, signal as libc::sighandler_t);
        libc::signal(libc::SIGINT, signal as libc::sighandler_t);
    }
    let handle = thread::spawn(move || worker(rx, logfile));
    unsafe {
        db_ui_run(command);
    }
    let _ = handle.join();
    drop(lock);
    Ok(())
}
