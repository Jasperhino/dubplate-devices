use crate::{device::Device, protocol::Xz};
use midir::{
    os::unix::{VirtualInput, VirtualOutput},
    Ignore, MidiInput, MidiInputConnection, MidiOutput, MidiOutputConnection,
};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};

pub struct Midi {
    _input: MidiInputConnection<()>,
    output: MidiOutputConnection,
    incoming: Receiver<Vec<u8>>,
    overflow: Arc<AtomicBool>,
    device: Device,
    protocol: Xz,
    started: Instant,
    announce_index: usize,
    pub packets: u64,
    pub handshake: bool,
}
impl Midi {
    pub fn open(device: Device) -> Result<Self, String> {
        let mut input = MidiInput::new("Dubplate Devices").map_err(|e| e.to_string())?;
        let output = MidiOutput::new("Dubplate Devices").map_err(|e| e.to_string())?;
        for name in input
            .ports()
            .iter()
            .filter_map(|p| input.port_name(p).ok())
            .chain(
                output
                    .ports()
                    .iter()
                    .filter_map(|p| output.port_name(p).ok()),
            )
        {
            // A physical controller or another copy of an emulator must not be shadowed.
            if ["DDJ-GRV6", "XDJ-XZ", "XDJ-XZ (1)", "XDJ-XZ (2)"].contains(&name.as_str()) {
                return Err(format!("{name} is already connected. Stop the other emulator or disconnect that controller first."));
            }
        }
        input.ignore(Ignore::None); // midir otherwise filters SysEx by default.
        let (tx, incoming) = mpsc::sync_channel(4096);
        let overflow = Arc::new(AtomicBool::new(false));
        let flag = overflow.clone();
        let input = input
            .create_virtual(
                device.port(),
                move |_, bytes, _| {
                    if bytes.len() > 4096 || tx.try_send(bytes.to_vec()).is_err() {
                        flag.store(true, Ordering::Relaxed);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;
        let output = output
            .create_virtual(device.port())
            .map_err(|e| e.to_string())?;
        Ok(Self {
            _input: input,
            output,
            incoming,
            overflow,
            device,
            protocol: Xz::default(),
            started: Instant::now(),
            announce_index: 0,
            packets: 0,
            handshake: false,
        })
    }
    pub fn tick(&mut self) -> Result<(), String> {
        if self.overflow.swap(false, Ordering::Relaxed) {
            return Err("MIDI input overflow; stopped the emulator to reset the session.".into());
        }
        for bytes in self.incoming.try_iter().take(4096) {
            self.packets += 1;
            if self.device == Device::Xz {
                for reply in self.protocol.consume(&bytes)? {
                    self.output.send(&reply).map_err(|e| e.to_string())?;
                }
                self.handshake = self.protocol.complete;
            }
        }
        if self.device == Device::Xz && !self.handshake {
            if let Some(&seconds) = [5, 15, 30].get(self.announce_index) {
                if self.started.elapsed() >= Duration::from_secs(seconds) {
                    self.output
                        .send(&Xz::announcement())
                        .map_err(|e| e.to_string())?;
                    self.announce_index += 1;
                }
            }
        }
        Ok(())
    }
}
