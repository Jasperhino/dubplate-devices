//! XDJ-XZ message exchange ported from the local, previously tested C responder.
//! MIDI recognition is separate from USB enumeration and feature availability.
const PREFIX: [u8; 9] = [0xf0, 0, 0x40, 5, 0, 0, 2, 0x0c, 0];
const MASK: [u8; 4] = [0x68, 1, 0x31, 0xfb];

#[derive(Default)]
pub struct Xz {
    assembly: Vec<u8>,
    pending: Option<([u8; 4], [u8; 4])>,
    pub complete: bool,
}

fn hash(first: &[u8], second: &[u8]) -> [u8; 4] {
    first
        .iter()
        .chain(second)
        .fold(0x811c9dc5u32, |h, b| {
            (h ^ u32::from(*b)).wrapping_mul(0x01000193)
        })
        .to_be_bytes()
}
fn field(payload: &[u8], tag: u8) -> Option<&[u8]> {
    let mut rest = payload;
    let mut found = None;
    while !rest.is_empty() {
        let n = usize::from(*rest.get(1)?);
        if n < 2 || n > rest.len() {
            return None;
        }
        if rest[0] == tag {
            if found.is_some() {
                return None;
            }
            found = Some(&rest[2..n]);
        }
        rest = &rest[n..];
    }
    found
}
fn unpack<const N: usize>(data: &[u8]) -> Option<[u8; N]> {
    if data.len() != N * 2 || data.iter().any(|b| *b > 15) {
        return None;
    }
    let mut result = [0; N];
    for (i, pair) in data.chunks_exact(2).enumerate() {
        result[i] = pair[0] << 4 | pair[1];
    }
    Some(result)
}
fn append(payload: &mut Vec<u8>, tag: u8, bytes: &[u8], nibbles: bool) {
    payload.extend([tag, (2 + bytes.len() * if nibbles { 2 } else { 1 }) as u8]);
    for b in bytes {
        if nibbles {
            payload.extend([b >> 4, b & 15]);
        } else {
            payload.push(*b);
        }
    }
}
fn message(stage: u8, payload: &[u8]) -> Vec<u8> {
    let mut msg = PREFIX.to_vec();
    msg.extend([stage, (payload.len() + 2) as u8]);
    msg.extend(payload);
    msg.push(0xf7);
    msg
}
impl Xz {
    pub fn announcement() -> Vec<u8> {
        message(0x11, &[1, 4, 1, 0x26])
    }

    /// Fragment tolerant, bounded SysEx parser. Real-time bytes may interleave.
    pub fn consume(&mut self, data: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut responses = Vec::new();
        for &b in data {
            if b >= 0xf8 {
                continue;
            }
            if b == 0xf0 {
                self.assembly.clear();
            }
            if self.assembly.is_empty() && b != 0xf0 {
                continue;
            }
            if self.assembly.len() >= 1024 {
                self.assembly.clear();
                continue;
            }
            if b >= 0x80 && b != 0xf0 && b != 0xf7 {
                self.assembly.clear();
                continue;
            }
            self.assembly.push(b);
            if b == 0xf7 {
                let msg = std::mem::take(&mut self.assembly);
                if let Some(reply) = self.handle(&msg)? {
                    responses.push(reply);
                }
            }
        }
        Ok(responses)
    }
    fn handle(&mut self, msg: &[u8]) -> Result<Option<Vec<u8>>, String> {
        if msg.len() < 12
            || !msg.starts_with(&PREFIX)
            || msg.last() != Some(&0xf7)
            || usize::from(msg[10]) + 10 != msg.len()
            || msg[10] < 2
        {
            return Ok(None);
        }
        let data = &msg[11..msg.len() - 1];
        match msg[9] {
            0x12 => {
                self.pending = None;
                self.complete = false;
                if field(data, 1) != Some(b"PioneerDJ".as_slice()) {
                    return Ok(None);
                }
                let Some(challenge) = field(data, 3).and_then(unpack::<8>) else {
                    return Ok(None);
                };
                let mut seed = [0; 4];
                getrandom::fill(&mut seed).map_err(|e| e.to_string())?;
                Ok(Some(self.respond(challenge, seed)))
            }
            0x14 => {
                let Some((seed, key)) = self.pending else {
                    return Ok(None);
                };
                if field(data, 4).and_then(unpack::<4>) != Some(hash(&seed, &key)) {
                    return Ok(None);
                }
                self.pending = None;
                self.complete = true;
                Ok(Some(message(0x15, &[])))
            }
            _ => Ok(None),
        }
    }
    fn respond(&mut self, challenge: [u8; 8], seed: [u8; 4]) -> Vec<u8> {
        let key = std::array::from_fn(|i| seed[i] ^ MASK[i]);
        let mut payload = Vec::new();
        append(&mut payload, 1, b"PioneerDJ", false);
        append(&mut payload, 2, b"XDJXZ", false);
        append(&mut payload, 4, &hash(&challenge, &key), true);
        append(&mut payload, 3, &seed, true);
        self.pending = Some((seed, key));
        message(0x13, &payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(text: &str) -> Vec<u8> {
        text.split_whitespace()
            .map(|x| u8::from_str_radix(x, 16).unwrap())
            .collect()
    }
    #[test]
    fn matches_successful_rekordbox_capture() {
        // Recorded locally on 2026-10-16 with the original C responder.
        let mut xz = Xz::default();
        let reply = xz.respond(
            [0x39, 0xbd, 0xe8, 0x20, 0x45, 9, 0x12, 0xdd],
            [0x2a, 0x56, 0x8a, 0x77],
        );
        assert_eq!(reply, hex("f0 00 40 05 00 00 02 0c 00 13 28 01 0b 50 69 6f 6e 65 65 72 44 4a 02 07 58 44 4a 58 5a 04 0a 09 0b 0c 0a 0c 08 05 00 03 0a 02 0a 05 06 08 0a 07 07 f7"));
        let host = hex("f0 00 40 05 00 00 02 0c 00 14 38 01 0b 50 69 6f 6e 65 65 72 44 4a 02 0b 72 65 6b 6f 72 64 62 6f 78 04 0a 06 0e 0d 01 0d 03 01 02 05 16 0d 0a 05 01 05 01 0b 0a 07 0e 0f 0d 06 02 01 04 00 0d 01 0c f7");
        assert_eq!(
            xz.consume(&host).unwrap(),
            vec![hex("f0 00 40 05 00 00 02 0c 00 15 02 f7")]
        );
        assert!(xz.complete);
        let challenge = hex("f0 00 40 05 00 00 02 0c 00 12 2a 01 0b 50 69 6f 6e 65 65 72 44 4a 02 0b 72 65 6b 6f 72 64 62 6f 78 03 12 03 09 0b 0d 0e 08 02 00 04 05 00 09 01 02 0d 0d f7");
        let responses = xz.consume(&challenge).unwrap();
        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0][9], 0x13);
        assert!(!xz.complete); // a fresh handshake invalidates the previous state.
    }
    #[test]
    fn known_fnv_vectors() {
        assert_eq!(hash(b"", b""), 0x811c9dc5u32.to_be_bytes());
        assert_eq!(hash(b"foo", b"bar"), 0xbf9cf968u32.to_be_bytes());
    }
    #[test]
    fn handshake_checks_host_response_and_cannot_replay() {
        let mut xz = Xz::default();
        let challenge = [0x1c, 0xd2, 0xc4, 0xd2, 0xda, 0x4d, 5, 0x73];
        let reply = xz.respond(challenge, [1, 2, 3, 4]);
        assert_eq!(reply[9], 0x13);
        assert_eq!(
            field(&reply[11..reply.len() - 1], 2),
            Some(b"XDJXZ".as_slice())
        );
        let mut bad = Vec::new();
        append(&mut bad, 4, &[0; 4], true);
        assert!(xz.consume(&message(0x14, &bad)).unwrap().is_empty());
        assert!(!xz.complete);
        let mut good = Vec::new();
        append(
            &mut good,
            4,
            &hash(&[1, 2, 3, 4], &[0x69, 3, 0x32, 0xff]),
            true,
        );
        let response = message(0x14, &good);
        assert!(xz.consume(&response[..5]).unwrap().is_empty());
        assert!(xz.consume(&[0xf8]).unwrap().is_empty());
        assert_eq!(
            xz.consume(&response[5..]).unwrap(),
            vec![message(0x15, &[])]
        );
        assert!(xz.complete);
        assert!(xz.consume(&response).unwrap().is_empty());
    }
    #[test]
    fn malformed_fields_are_rejected() {
        assert_eq!(field(&[1, 1], 1), None);
        assert_eq!(field(&[1, 4, 0], 1), None);
        assert_eq!(field(&[1, 2, 1, 2], 1), None);
        assert_eq!(unpack::<1>(&[16, 0]), None);
        let mut xz = Xz::default();
        xz.consume(&[0xf0]).unwrap();
        xz.consume(&vec![1; 8192]).unwrap();
        assert!(xz.assembly.len() <= 1024);
        assert!(!xz.complete);
    }
}
