#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Device {
    Off = 0,
    Grv6 = 1,
    Xz = 2,
}
impl Device {
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            0 => Some(Self::Off),
            1 => Some(Self::Grv6),
            2 => Some(Self::Xz),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Grv6 => "DDJ-GRV6",
            Self::Xz => "XDJ-XZ",
        }
    }
    pub fn port(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Grv6 => "DDJ-GRV6",
            Self::Xz => "XDJ-XZ (1)",
        }
    }
    pub fn profile(self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Grv6 => "grv6.jigbp",
            Self::Xz => "xz.jigbp",
        }
    }
}
