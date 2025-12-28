#[derive(Debug, Copy, Clone)]
pub struct Word(pub u32);

impl Word {
    pub fn opcode(self) -> u32 {
        self.0 >> 26
    }

    pub fn xform_opcode(self) -> u32 {
        // Bits 21-30
        self.u32::<21, 30>()
    }

    /// Extracts a u32 bit range in big endian (reversed) from this word.
    pub fn u32<const FROM: u32, const TO: u32>(self) -> u32 {
        const { assert!(TO >= FROM && TO - FROM < 32) };

        let mask = const { (!0u32) >> (FROM + (31 - TO)) << (31 - TO) };
        (self.0 & mask) >> (31 - TO)
    }

    pub fn i32<const FROM: u32, const TO: u32>(self) -> i32 {
        self.u32::<FROM, TO>() as i32
    }

    pub fn u16<const FROM: u32, const TO: u32>(self) -> u16 {
        const { assert!(TO >= FROM && TO - FROM < 16) };
        self.u32::<FROM, TO>() as u16
    }

    pub fn i16<const FROM: u32, const TO: u32>(self) -> i16 {
        self.u16::<FROM, TO>() as i16
    }

    pub fn u8<const FROM: u32, const TO: u32>(self) -> u8 {
        const { assert!(TO >= FROM && TO - FROM < 8) };
        self.u32::<FROM, TO>() as u8
    }

    pub fn i8<const FROM: u32, const TO: u32>(self) -> i8 {
        self.u8::<FROM, TO>() as i8
    }

    pub fn bit<const BIT: u32>(self) -> u32 {
        self.0 & (1 << (31 - BIT))
    }
}
