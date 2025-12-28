#[derive(Debug)]
pub struct SectionInfo {
    file_offset: u32,
    load_offset: u32,
    size: u32,
}

impl SectionInfo {
    pub fn contains_addr(&self, addr: u32) -> bool {
        addr >= self.load_offset && addr < self.load_offset + self.size
    }

    pub fn file_offset_of_addr(&self, addr: u32) -> u32 {
        assert!(self.contains_addr(addr));
        self.file_offset + (addr - self.load_offset)
    }
}

#[derive(Debug)]
pub struct Dol(Vec<u8>);

impl Dol {
    /*
    Start 	End 	Length 	Description
    0x0 	0x3 	4 	File offset to start of Text0
    0x04 	0x1b 	24 	File offsets for Text1..6
    0x1c 	0x47 	44 	File offsets for Data0..10
    0x48 	0x4B 	4 	Loading address for Text0
    0x4C 	0x8F 	68 	Loading addresses for Text1..6, Data0..10
    0x90 	0xD7 	72 	Section sizes for Text0..6, Data0..10
    0xD8 	0xDB 	4 	BSS address
    0xDC 	0xDF 	4 	BSS size
    0xE0 	0xE3 	4 	Entry point
    0xE4 	0xFF 		padding
    */
    const BSS_ADDR_OFF: usize = 0xD8;
    const BSS_SIZE_OFF: usize = 0xDC;
    const SECTION_OFFSET_OFF: usize = 0;
    const SECTION_ADDRESS_OFF: usize = 0x48;
    const SECTION_SIZE_OFF: usize = 0x90;
    const ENTRYPOINT_OFF: usize = 0xE0;

    /// Create a new DOL from the given bytes, validating it in the process.
    pub fn new(bytes: Vec<u8>) -> Result<Self, &'static str> {
        if bytes.len() < 0xFF {
            return Err(".dol file smaller than 255 bytes (does not contain all headers)");
        }
        Ok(Self(bytes))
    }

    fn u32(&self, off: usize) -> u32 {
        let bytes: [u8; 4] = self.0[off..][..4].try_into().unwrap();
        u32::from_be_bytes(bytes)
    }

    pub fn section(&self, section: usize) -> SectionInfo {
        assert!(section <= 17);
        SectionInfo {
            file_offset: self.u32(Self::SECTION_OFFSET_OFF + section * 4),
            load_offset: self.u32(Self::SECTION_ADDRESS_OFF + section * 4),
            size: self.u32(Self::SECTION_SIZE_OFF + section * 4),
        }
    }

    pub fn sections(&self) -> impl Iterator<Item = SectionInfo> + '_ {
        (0..18).map(|i| self.section(i))
    }

    pub fn section_of_load_addr(&self, addr: u32) -> Option<SectionInfo> {
        self.sections().find(|s| s.contains_addr(addr))
    }

    pub fn entrypoint(&self) -> u32 {
        self.u32(Self::ENTRYPOINT_OFF)
    }

    pub fn bss_address(&self) -> u32 {
        self.u32(Self::BSS_ADDR_OFF)
    }

    pub fn bss_size(&self) -> u32 {
        self.u32(Self::BSS_SIZE_OFF)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}
